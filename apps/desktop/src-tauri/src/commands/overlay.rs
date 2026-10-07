use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, State, WebviewWindow};

/// Whether the overlay should appear once the UI has loaded (normal launch), as
/// opposed to staying resident in the tray (`--background`).
pub(crate) struct ShowWhenReady(pub(crate) AtomicBool);

/// Hides the overlay (Escape in the UI). The window is kept alive for instant re-show.
#[tauri::command]
pub(crate) async fn hide_overlay(window: WebviewWindow) {
    crate::overlay::hide(window.app_handle());
}

/// Called once by the UI after its first render. Showing only then avoids a blank
/// or white first frame and guarantees the UI is listening for focus events.
#[tauri::command]
pub(crate) async fn overlay_ready(
    window: WebviewWindow,
    show: State<'_, ShowWhenReady>,
) -> Result<(), ()> {
    if show.0.swap(false, Ordering::AcqRel) {
        crate::overlay::show(window.app_handle());
    }
    Ok(())
}
