//! Tray icon: the always-available way to reach a hidden, resident Lumen, and (T003) where
//! the keyboard shortcut is chosen.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime};

use crate::{overlay, shortcut};

const TRAY_ID: &str = "lumen";
const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";
const SHORTCUT_PREFIX: &str = "shortcut:";

/// The shortcut menu entries, kept to update checks/labels.
struct ShortcutItems<R: Runtime>(Vec<(&'static str, CheckMenuItem<R>)>);

/// Menu label of a shortcut choice.
pub(crate) fn choice_text(label: &str, available: bool) -> String {
    if available {
        label.to_owned()
    } else {
        format!("{label} (in use by another app)")
    }
}

/// Tray tooltip for the active shortcut.
pub(crate) fn tooltip(active: Option<&str>) -> String {
    match active {
        Some(label) => format!("Lumen ({label})"),
        None => "Lumen - no keyboard shortcut available; right-click to choose one".to_owned(),
    }
}

pub(crate) fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Lumen", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit Lumen", true, None::<&str>)?;
    let mut items = Vec::new();
    for label in shortcut::CHOICES {
        let item = CheckMenuItem::with_id(
            app,
            format!("{SHORTCUT_PREFIX}{label}"),
            label,
            true,
            false,
            None::<&str>,
        )?;
        items.push((label, item));
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    let shortcuts = Submenu::with_items(app, "Keyboard shortcut", true, &refs)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &shortcuts, &separator, &quit])?;
    app.manage(ShortcutItems(items));

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if id == MENU_SHOW {
                overlay::show(app);
            } else if id == MENU_QUIT {
                app.exit(0);
            } else if let Some(label) = id.strip_prefix(SHORTCUT_PREFIX) {
                shortcut::choose(app, label);
            }
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
    refresh(app.handle());
    Ok(())
}

/// Updates check marks, "(in use)" labels and the tooltip from the current state.
pub(crate) fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let active = app.state::<shortcut::Active>().label();
    if let Some(items) = app.try_state::<ShortcutItems<R>>() {
        for (label, item) in &items.0 {
            let _ = item.set_checked(active == Some(*label));
            let _ = item.set_text(choice_text(label, shortcut::available(app, label)));
        }
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(active)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_tooltips() {
        assert_eq!(choice_text("Ctrl+Space", true), "Ctrl+Space");
        assert_eq!(
            choice_text("Alt+Space", false),
            "Alt+Space (in use by another app)"
        );
        assert_eq!(tooltip(Some("Alt+Space")), "Lumen (Alt+Space)");
        assert!(tooltip(None).contains("right-click"));
    }
}
