//! Tray icon: the always-available way to reach a hidden, resident Lumen.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Runtime};

use crate::{overlay, shortcut};

const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";

pub(crate) fn install<R: Runtime>(app: &App<R>, shortcut_registered: bool) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Lumen", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Lumen", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let tooltip = if shortcut_registered {
        format!("Lumen ({})", shortcut::DEFAULT_LABEL)
    } else {
        format!("Lumen ({} unavailable)", shortcut::DEFAULT_LABEL)
    };

    let mut builder = TrayIconBuilder::with_id("lumen")
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_SHOW => overlay::show(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click always shows (never toggles): clicking the tray first blurs
            // the overlay, which already hides it.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                overlay::show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
