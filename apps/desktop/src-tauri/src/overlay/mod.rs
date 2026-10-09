//! The single overlay window: show/hide/focus lifecycle (T002).
//!
//! Window/OS adaptation only; no domain logic. The window is created hidden by
//! `tauri.conf.json` and is never destroyed: hiding keeps the WebView and its
//! state alive so the next show is instant (docs/PERFORMANCE.md §3.1).

mod placement;
mod policy;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

use placement::PhysicalRect;
use policy::ShortcutDecision;

/// Label of the overlay window in `tauri.conf.json`.
pub(crate) const WINDOW_LABEL: &str = "main";

/// Logical width of the overlay and its compact height (search bar only).
/// Must match `width`/`height` in `tauri.conf.json`.
pub(crate) const LOGICAL_WIDTH: f64 = 800.0;
pub(crate) const COMPACT_HEIGHT: f64 = 64.0;

/// Content size last requested by the UI (`resize_overlay`), as `f64` bits.
static REQUESTED_HEIGHT: AtomicU64 = AtomicU64::new(COMPACT_HEIGHT.to_bits());
static REQUESTED_WIDTH: AtomicU64 = AtomicU64::new(LOGICAL_WIDTH.to_bits());
// The overlay owns show/hide. Background catalog events must not wake hidden search or
// query inference; the next EVENT_SHOWN already refreshes from the current store.
static SHOWN: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_shown() -> bool {
    SHOWN.load(Ordering::Acquire)
}

fn requested() -> (f64, f64) {
    (
        f64::from_bits(REQUESTED_WIDTH.load(Ordering::Relaxed)),
        f64::from_bits(REQUESTED_HEIGHT.load(Ordering::Relaxed)),
    )
}

/// Size actually applied on a monitor, and the window's left edge for it.
fn fit(requested: (f64, f64), area: PhysicalRect, scale: f64) -> ((f64, f64), i32) {
    let width = placement::clamp_width(requested.0, LOGICAL_WIDTH, area.width, scale);
    let height = placement::clamp_height(requested.1, COMPACT_HEIGHT, area.height, scale);
    let compact = placement::to_physical((LOGICAL_WIDTH, height), scale);
    let (base_x, _) = placement::overlay_position(area, compact);
    let physical = placement::to_physical((width, height), scale);
    (
        (width, height),
        placement::expand_x(base_x, physical.0, area),
    )
}

fn rect(area: tauri::PhysicalRect<i32, u32>) -> PhysicalRect {
    PhysicalRect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    }
}

/// Event emitted to the UI after the overlay was shown and focus requested.
/// Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_SHOWN: &str = "lumen:overlay-shown";

/// Payload of [`EVENT_SHOWN`]. `seq` is set only when timing diagnostics are on
/// (`LUMEN_DIAG_LOG`): the UI then calls `overlay_painted(seq)` after its next frame.
#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct ShownPayload {
    pub(crate) seq: Option<u64>,
}

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    let window = app.get_webview_window(WINDOW_LABEL);
    if window.is_none() {
        eprintln!("lumen: overlay window `{WINDOW_LABEL}` missing");
    }
    window
}

/// Global-shortcut entry point.
pub(crate) fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = window(app) else { return };
    let visible = window.is_visible().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    match policy::on_shortcut(visible, focused) {
        ShortcutDecision::Show => show_window(&window),
        ShortcutDecision::Focus => focus_window(&window, None),
        ShortcutDecision::Hide => hide_window(&window),
    }
}

/// Shows (or re-focuses) the overlay. Used by tray, second instance and startup.
pub(crate) fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = window(app) {
        if window.is_visible().unwrap_or(false) {
            focus_window(&window, None);
        } else {
            show_window(&window);
        }
    }
}

pub(crate) fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = window(app) {
        hide_window(&window);
    }
}

fn show_window<R: Runtime>(window: &WebviewWindow<R>) {
    let seq = crate::diag::begin_show();
    let started = std::time::Instant::now();
    place_on_active_monitor(window);
    crate::material::before_show(window);
    crate::lifecycle::before_show(window);
    if let Err(err) = window.show() {
        eprintln!("lumen: show overlay failed: {err}");
        return;
    }
    SHOWN.store(true, Ordering::Release);
    focus_window(window, seq);
    crate::diag::record("show_native_ms", started.elapsed().as_secs_f64() * 1000.0);
    crate::indexing::on_overlay_shown(window.app_handle());
    if cfg!(debug_assertions) {
        eprintln!(
            "lumen: overlay shown in {:?} (native calls only)",
            started.elapsed()
        );
    }
}

fn focus_window<R: Runtime>(window: &WebviewWindow<R>, seq: Option<u64>) {
    // Windows only grants foreground to the process that received the input event
    // (hotkey/tray click), which is us; failures are reported, not fatal.
    if let Err(err) = window.set_focus() {
        eprintln!("lumen: focus overlay failed: {err}");
    }
    if let Err(err) = window.emit_to(window.label(), EVENT_SHOWN, ShownPayload { seq }) {
        eprintln!("lumen: emit {EVENT_SHOWN} failed: {err}");
    }
}

fn hide_window<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(err) = window.hide() {
        eprintln!("lumen: hide overlay failed: {err}");
        return;
    }
    SHOWN.store(false, Ordering::Release);
    crate::lifecycle::after_hide(window);
}

/// Moves the window to the monitor under the cursor (falling back to the
/// window's current/primary monitor), sized for that monitor's scale factor.
fn place_on_active_monitor<R: Runtime>(window: &WebviewWindow<R>) {
    let monitor = window
        .cursor_position()
        .ok()
        .and_then(|p| window.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let work_area = rect(*monitor.work_area());
    let scale = monitor.scale_factor();
    let ((width, height), x) = fit(requested(), work_area, scale);
    // The top edge comes from the compact window: the same line whatever the content.
    let (_, y) = placement::overlay_position(
        work_area,
        placement::to_physical((LOGICAL_WIDTH, height), scale),
    );

    // Keep the logical size stable when moving between monitors with different DPI.
    if let Err(err) = window.set_size(LogicalSize::new(width, height)) {
        eprintln!("lumen: resize overlay failed: {err}");
    }
    if let Err(err) = window.set_position(PhysicalPosition::new(x, y)) {
        eprintln!("lumen: position overlay failed: {err}");
    }
}

/// Sizes the overlay for its content (logical px; T103 height, T105 width), keeping the
/// top edge, and the left edge unless a wider window would overflow the monitor; returns
/// the size applied after clamping to the window's monitor.
pub(crate) fn resize<R: Runtime>(window: &WebviewWindow<R>, width: f64, height: f64) -> (f64, f64) {
    let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
    let want = (finite(width, LOGICAL_WIDTH), finite(height, COMPACT_HEIGHT));
    REQUESTED_WIDTH.store(want.0.to_bits(), Ordering::Relaxed);
    REQUESTED_HEIGHT.store(want.1.to_bits(), Ordering::Relaxed);
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return (want.0.max(LOGICAL_WIDTH), want.1.max(COMPACT_HEIGHT));
    };
    let scale = monitor.scale_factor();
    let ((w, h), x) = fit(want, rect(*monitor.work_area()), scale);
    let current = window.inner_size().ok().map(|size| {
        (
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )
    });
    let unchanged = current.is_some_and(|(cw, ch)| (cw - w).abs() < 0.5 && (ch - h).abs() < 0.5);
    if !unchanged {
        if let Err(err) = window.set_size(LogicalSize::new(w, h)) {
            eprintln!("lumen: resize overlay failed: {err}");
        }
        if let Ok(pos) = window.outer_position()
            && pos.x != x
            && let Err(err) = window.set_position(PhysicalPosition::new(x, pos.y))
        {
            eprintln!("lumen: reposition overlay failed: {err}");
        }
    }
    (w, h)
}
