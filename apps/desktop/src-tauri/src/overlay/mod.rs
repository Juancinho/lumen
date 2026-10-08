//! The single overlay window: show/hide/focus lifecycle (T002).
//!
//! Window/OS adaptation only; no domain logic. The window is created hidden by
//! `tauri.conf.json` and is never destroyed: hiding keeps the WebView and its
//! state alive so the next show is instant (docs/PERFORMANCE.md §3.1).

mod placement;
mod policy;

use std::sync::atomic::{AtomicU64, Ordering};

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

/// Content height last requested by the UI (`resize_overlay`), as `f64` bits.
static REQUESTED_HEIGHT: AtomicU64 = AtomicU64::new(COMPACT_HEIGHT.to_bits());

fn requested_height() -> f64 {
    f64::from_bits(REQUESTED_HEIGHT.load(Ordering::Relaxed))
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
    focus_window(window, seq);
    crate::diag::record("show_native_ms", started.elapsed().as_secs_f64() * 1000.0);
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

    let area = monitor.work_area();
    let work_area = PhysicalRect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    };
    let scale = monitor.scale_factor();
    let height =
        placement::clamp_height(requested_height(), COMPACT_HEIGHT, area.size.height, scale);
    let size = placement::to_physical((LOGICAL_WIDTH, height), scale);
    let (x, y) = placement::overlay_position(work_area, size);

    // Keep the logical size stable when moving between monitors with different DPI.
    if let Err(err) = window.set_size(LogicalSize::new(LOGICAL_WIDTH, height)) {
        eprintln!("lumen: resize overlay failed: {err}");
    }
    if let Err(err) = window.set_position(PhysicalPosition::new(x, y)) {
        eprintln!("lumen: position overlay failed: {err}");
    }
}

/// Sizes the overlay for `requested` logical px of content (T103), keeping the top edge
/// where it is; returns the height applied after clamping to the window's monitor.
pub(crate) fn resize<R: Runtime>(window: &WebviewWindow<R>, requested: f64) -> f64 {
    let requested = if requested.is_finite() {
        requested
    } else {
        COMPACT_HEIGHT
    };
    REQUESTED_HEIGHT.store(requested.to_bits(), Ordering::Relaxed);
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let height = monitor.map_or(requested.max(COMPACT_HEIGHT), |m| {
        placement::clamp_height(
            requested,
            COMPACT_HEIGHT,
            m.work_area().size.height,
            m.scale_factor(),
        )
    });
    let current = window
        .inner_size()
        .ok()
        .zip(window.scale_factor().ok())
        .map(|(size, scale)| f64::from(size.height) / scale);
    if current.is_none_or(|h| (h - height).abs() >= 0.5)
        && let Err(err) = window.set_size(LogicalSize::new(LOGICAL_WIDTH, height))
    {
        eprintln!("lumen: resize overlay failed: {err}");
    }
    height
}
