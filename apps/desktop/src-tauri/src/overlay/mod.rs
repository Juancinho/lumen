//! The single overlay window: show/hide/focus lifecycle (T002).
//!
//! Window/OS adaptation only; no domain logic. The window is created hidden by
//! `tauri.conf.json` and is never destroyed: hiding keeps the WebView and its
//! state alive so the next show is instant (docs/PERFORMANCE.md §3.1).

mod placement;
mod policy;

use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

use placement::PhysicalRect;
use policy::ShortcutDecision;

/// Label of the overlay window in `tauri.conf.json`.
pub(crate) const WINDOW_LABEL: &str = "main";

/// Logical size of the compact overlay (search field only until T103).
/// Must match `width`/`height` in `tauri.conf.json`.
pub(crate) const LOGICAL_SIZE: (f64, f64) = (800.0, 64.0);

/// Event emitted to the UI after the overlay was shown and focus requested.
/// Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_SHOWN: &str = "lumen:overlay-shown";

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
        ShortcutDecision::Focus => focus_window(&window),
        ShortcutDecision::Hide => hide_window(&window),
    }
}

/// Shows (or re-focuses) the overlay. Used by tray, second instance and startup.
pub(crate) fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = window(app) {
        if window.is_visible().unwrap_or(false) {
            focus_window(&window);
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
    let started = std::time::Instant::now();
    place_on_active_monitor(window);
    if let Err(err) = window.show() {
        eprintln!("lumen: show overlay failed: {err}");
        return;
    }
    focus_window(window);
    if cfg!(debug_assertions) {
        eprintln!(
            "lumen: overlay shown in {:?} (native calls only)",
            started.elapsed()
        );
    }
}

fn focus_window<R: Runtime>(window: &WebviewWindow<R>) {
    // Windows only grants foreground to the process that received the input event
    // (hotkey/tray click), which is us; failures are reported, not fatal.
    if let Err(err) = window.set_focus() {
        eprintln!("lumen: focus overlay failed: {err}");
    }
    if let Err(err) = window.emit_to(window.label(), EVENT_SHOWN, ()) {
        eprintln!("lumen: emit {EVENT_SHOWN} failed: {err}");
    }
}

fn hide_window<R: Runtime>(window: &WebviewWindow<R>) {
    if let Err(err) = window.hide() {
        eprintln!("lumen: hide overlay failed: {err}");
    }
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
    let size = placement::to_physical(LOGICAL_SIZE, monitor.scale_factor());
    let (x, y) = placement::overlay_position(work_area, size);

    // Keep the logical size stable when moving between monitors with different DPI.
    if let Err(err) = window.set_size(LogicalSize::new(LOGICAL_SIZE.0, LOGICAL_SIZE.1)) {
        eprintln!("lumen: resize overlay failed: {err}");
    }
    if let Err(err) = window.set_position(PhysicalPosition::new(x, y)) {
        eprintln!("lumen: position overlay failed: {err}");
    }
}
