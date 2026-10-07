//! Global shortcut (fixed for T002; configurable with conflict UX in T003).

use tauri::{App, Runtime};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::overlay;

/// Human-readable form, shown in the tray tooltip.
pub(crate) const DEFAULT_LABEL: &str = "Alt+Space";

fn default_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::ALT), Code::Space)
}

/// Installs the plugin and registers the toggle shortcut. A failed registration
/// (e.g. another launcher owns Alt+Space) is reported and the app keeps running:
/// the tray still opens the overlay.
pub(crate) fn install<R: Runtime>(app: &App<R>) -> tauri::Result<bool> {
    let toggle = default_shortcut();
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event: ShortcutEvent| {
                if event.state() == ShortcutState::Pressed && *shortcut == toggle {
                    overlay::toggle(app);
                }
            })
            .build(),
    )?;
    match app.global_shortcut().register(toggle) {
        Ok(()) => Ok(true),
        Err(err) => {
            eprintln!("lumen: could not register {DEFAULT_LABEL}: {err}");
            Ok(false)
        }
    }
}
