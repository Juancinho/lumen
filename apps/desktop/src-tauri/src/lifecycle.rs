//! What the single WebView does while the overlay is hidden (T012, ADR-020).
//!
//! Hiding the window alone keeps the WebView2 instance "visible" to Chromium, so it keeps a
//! renderer ready (fastest re-show, most memory). The other modes trade re-show latency for
//! memory, using WebView2's own APIs:
//!
//! | mode         | on hide                                         | on show          |
//! |--------------|-------------------------------------------------|------------------|
//! | `keep`       | nothing                                         | nothing          |
//! | `invisible`  | `SetIsVisible(false)` (throttles rendering)      | `SetIsVisible(true)` |
//! | `low-memory` | invisible + `MemoryUsageTargetLevel = Low`       | Normal + visible |
//! | `suspend`    | low-memory, then `TrySuspend` after [`SUSPEND_AFTER`] hidden | as above (resumes) |
//! | `idle-low-memory` (default) | nothing; `low-memory` after [`IDLE_TRIM_AFTER`] hidden | Normal + visible |
//!
//! The mode comes from `LUMEN_WEBVIEW_HIDDEN` (diagnostics/benchmarks) and defaults to
//! [`DEFAULT_MODE`]. JS state (query, React tree) survives every mode.
//!
//! T012 on joao-pc (private working set of lumen.exe + WebView2, show -> painted frame):
//! `keep` 72 MiB hidden, 22.6/26.0 ms p50/p95; `low-memory` 7.4 MiB hidden, 27.1/32.0 ms;
//! `invisible` saves nothing; `suspend` used more memory (93-160 MiB). `idle-low-memory`
//! keeps `keep`'s latency for quick re-opens and `low-memory`'s footprint when idle.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{Runtime, WebviewWindow};

pub(crate) const ENV_MODE: &str = "LUMEN_WEBVIEW_HIDDEN";

/// How long the overlay must stay hidden before `suspend` mode suspends the WebView.
pub(crate) const SUSPEND_AFTER: Duration = Duration::from_secs(5);

/// How long the overlay must stay hidden before `idle-low-memory` trims the WebView.
pub(crate) const IDLE_TRIM_AFTER: Duration = Duration::from_secs(30);

/// ADR-020 (T012).
pub(crate) const DEFAULT_MODE: HiddenMode = HiddenMode::IdleLowMemory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HiddenMode {
    Keep,
    Invisible,
    LowMemory,
    Suspend,
    IdleLowMemory,
}

impl HiddenMode {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "keep" => Some(Self::Keep),
            "invisible" => Some(Self::Invisible),
            "low-memory" => Some(Self::LowMemory),
            "suspend" => Some(Self::Suspend),
            "idle-low-memory" => Some(Self::IdleLowMemory),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Invisible => "invisible",
            Self::LowMemory => "low-memory",
            Self::Suspend => "suspend",
            Self::IdleLowMemory => "idle-low-memory",
        }
    }

    /// `None`/unknown values fall back to [`DEFAULT_MODE`] (with a warning for unknown).
    pub(crate) fn from_value(value: Option<&str>) -> Self {
        match value {
            None => DEFAULT_MODE,
            Some(v) => Self::parse(v).unwrap_or_else(|| {
                eprintln!(
                    "lumen: unknown {ENV_MODE}={v:?}, using {}",
                    DEFAULT_MODE.as_str()
                );
                DEFAULT_MODE
            }),
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn lowers_memory(self) -> bool {
        matches!(self, Self::LowMemory | Self::Suspend | Self::IdleLowMemory)
    }
}

static MODE: OnceLock<HiddenMode> = OnceLock::new();
/// Bumped on every show and hide. Delayed work (trim, suspend) checks it on the UI thread
/// right before acting, so it can never run after a newer show.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn still_hidden(generation: u64) -> bool {
    GENERATION.load(Ordering::Acquire) == generation
}

pub(crate) fn mode() -> HiddenMode {
    *MODE.get_or_init(|| HiddenMode::from_value(std::env::var(ENV_MODE).ok().as_deref()))
}

/// Call right before `window.show()`; the WebView work is queued ahead of the show.
pub(crate) fn before_show<R: Runtime>(window: &WebviewWindow<R>) {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    let mode = mode();
    if mode != HiddenMode::Keep {
        platform::set_visible(window, mode, true, None);
    }
}

/// Call right after `window.hide()`.
pub(crate) fn after_hide<R: Runtime>(window: &WebviewWindow<R>) {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let mode = mode();
    if mode == HiddenMode::Keep {
        return;
    }
    let delayed = |after: Duration, action: fn(&WebviewWindow<R>, HiddenMode, u64)| {
        let window = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            if still_hidden(generation) {
                action(&window, mode, generation);
            }
        });
    };
    match mode {
        HiddenMode::Keep => {}
        HiddenMode::IdleLowMemory => delayed(IDLE_TRIM_AFTER, |w, m, g| {
            platform::set_visible(w, m, false, Some(g));
        }),
        HiddenMode::Suspend => {
            platform::set_visible(window, mode, false, None);
            delayed(SUSPEND_AFTER, |w, _, g| platform::try_suspend(w, g));
        }
        HiddenMode::Invisible | HiddenMode::LowMemory => {
            platform::set_visible(window, mode, false, None);
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // WebView2 COM calls (see the SAFETY notes).
mod platform {
    use tauri::{Runtime, WebviewWindow};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_3, ICoreWebView2_19, ICoreWebView2Controller,
    };
    use webview2_com::TrySuspendCompletedHandler;
    use windows_core::Interface;

    use super::HiddenMode;
    use crate::diag;

    fn apply(
        controller: &ICoreWebView2Controller,
        mode: HiddenMode,
        visible: bool,
    ) -> windows_core::Result<()> {
        // SAFETY: called from `with_webview`, i.e. on the UI thread that owns the WebView2
        // controller; every interface pointer comes from that live controller and is used
        // only during this call.
        unsafe {
            if !visible {
                controller.SetIsVisible(false)?;
            }
            if mode.lowers_memory() {
                let core: ICoreWebView2_19 = controller.CoreWebView2()?.cast()?;
                core.SetMemoryUsageTargetLevel(if visible {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                })?;
            }
            if visible {
                // Also resumes a suspended WebView.
                controller.SetIsVisible(true)?;
            }
        }
        Ok(())
    }

    /// `only_if_hidden_since`: a delayed hide skips itself if the overlay was shown since.
    pub(super) fn set_visible<R: Runtime>(
        window: &WebviewWindow<R>,
        mode: HiddenMode,
        visible: bool,
        only_if_hidden_since: Option<u64>,
    ) {
        let queued = window.with_webview(move |webview| {
            if only_if_hidden_since.is_some_and(|g| !super::still_hidden(g)) {
                return;
            }
            if !visible {
                diag::record("webview_trimmed_ms", diag::since_start_ms());
            }
            if let Err(err) = apply(&webview.controller(), mode, visible) {
                eprintln!(
                    "lumen: webview visible={visible} ({}) failed: {err}",
                    mode.as_str()
                );
            }
        });
        if let Err(err) = queued {
            eprintln!("lumen: with_webview failed: {err}");
        }
    }

    fn suspend(controller: &ICoreWebView2Controller) -> windows_core::Result<()> {
        let handler = TrySuspendCompletedHandler::create(Box::new(|result, suspended| {
            let event = if result.is_ok() && suspended {
                "webview_suspended_ms"
            } else {
                "webview_suspend_refused_ms"
            };
            diag::record(event, diag::since_start_ms());
            Ok(())
        }));
        // SAFETY: as in `apply` (UI thread, live controller). `handler` is a COM object the
        // runtime keeps alive until it has been invoked.
        unsafe {
            let core: ICoreWebView2_3 = controller.CoreWebView2()?.cast()?;
            core.TrySuspend(&handler)
        }
    }

    pub(super) fn try_suspend<R: Runtime>(window: &WebviewWindow<R>, hidden_since: u64) {
        let queued = window.with_webview(move |webview| {
            if !super::still_hidden(hidden_since) {
                return;
            }
            if let Err(err) = suspend(&webview.controller()) {
                eprintln!("lumen: webview suspend failed: {err}");
            }
        });
        if let Err(err) = queued {
            eprintln!("lumen: with_webview failed: {err}");
        }
    }
}

#[cfg(not(windows))]
mod platform {
    //! WebView2 APIs are Windows-only; elsewhere every mode behaves like `keep`.
    use tauri::{Runtime, WebviewWindow};

    use super::HiddenMode;

    pub(super) fn set_visible<R: Runtime>(
        _: &WebviewWindow<R>,
        _: HiddenMode,
        _: bool,
        _: Option<u64>,
    ) {
    }

    pub(super) fn try_suspend<R: Runtime>(_: &WebviewWindow<R>, _: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_and_unknown_falls_back() {
        for m in [
            HiddenMode::Keep,
            HiddenMode::Invisible,
            HiddenMode::LowMemory,
            HiddenMode::Suspend,
            HiddenMode::IdleLowMemory,
        ] {
            assert_eq!(HiddenMode::parse(m.as_str()), Some(m));
            assert_eq!(HiddenMode::from_value(Some(m.as_str())), m);
        }
        assert_eq!(HiddenMode::from_value(None), DEFAULT_MODE);
        assert_eq!(HiddenMode::from_value(Some("turbo")), DEFAULT_MODE);
        assert!(HiddenMode::Suspend.lowers_memory() && !HiddenMode::Invisible.lowers_memory());
        assert!(
            DEFAULT_MODE.lowers_memory(),
            "ADR-020: idle trim by default"
        );
        assert!(IDLE_TRIM_AFTER > SUSPEND_AFTER);
    }
}
