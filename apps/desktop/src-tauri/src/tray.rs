//! Tray icon: the always-available way to reach a hidden, resident Lumen, and (T003) where
//! the keyboard shortcut and (T004) the window material are chosen.

use lumen_windows::material::{Material, MaterialChoice, Plan, Reason};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime};

use crate::{material, overlay, shortcut};

const TRAY_ID: &str = "lumen";
const MENU_SHOW: &str = "show";
const MENU_QUIT: &str = "quit";
const SHORTCUT_PREFIX: &str = "shortcut:";
const MATERIAL_PREFIX: &str = "material:";

/// The shortcut menu entries, kept to update checks/labels.
struct ShortcutItems<R: Runtime>(Vec<(&'static str, CheckMenuItem<R>)>);

/// The material menu entries.
struct MaterialItems<R: Runtime>(Vec<(MaterialChoice, CheckMenuItem<R>)>);

fn material_name(m: Material) -> &'static str {
    match m {
        Material::Acrylic => "Acrylic",
        Material::Mica => "Mica",
        Material::Solid => "Solid",
    }
}

/// Menu label of a material choice; the chosen one says what is really used when the
/// system forced a fallback (and `Automatic` always names what it resolved to).
pub(crate) fn material_text(choice: MaterialChoice, chosen: bool, plan: Option<Plan>) -> String {
    let base = match choice {
        MaterialChoice::Auto => "Automatic",
        MaterialChoice::Acrylic => "Acrylic",
        MaterialChoice::Mica => "Mica",
        MaterialChoice::Solid => "Solid",
    };
    let Some(plan) = plan.filter(|_| chosen) else {
        return base.to_owned();
    };
    let why = match plan.reason {
        Reason::AsRequested => None,
        Reason::HighContrast => Some("high contrast is on"),
        Reason::TransparencyOff => Some("transparency effects are off"),
        Reason::UnsupportedBuild => Some("needs Windows 11 22H2"),
    };
    match (choice, why) {
        (_, Some(why)) => format!("{base} (using {}: {why})", material_name(plan.material)),
        (MaterialChoice::Auto, None) => format!("{base} ({})", material_name(plan.material)),
        (_, None) => base.to_owned(),
    }
}

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
    let mut material_items = Vec::new();
    for choice in MaterialChoice::ALL {
        let item = CheckMenuItem::with_id(
            app,
            format!("{MATERIAL_PREFIX}{}", choice.as_str()),
            material_text(choice, false, None),
            true,
            false,
            None::<&str>,
        )?;
        material_items.push((choice, item));
    }
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = material_items
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<R>)
        .collect();
    let materials = Submenu::with_items(app, "Window material", true, &refs)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &shortcuts, &materials, &separator, &quit])?;
    app.manage(ShortcutItems(items));
    app.manage(MaterialItems(material_items));

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
            } else if let Some(choice) = id
                .strip_prefix(MATERIAL_PREFIX)
                .and_then(MaterialChoice::parse)
            {
                material::choose(app, choice);
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
    refresh_material(app.handle());
    Ok(())
}

/// Updates the material check marks and labels (no-op before the tray exists).
pub(crate) fn refresh_material<R: Runtime>(app: &AppHandle<R>) {
    let (Some(items), Some(state)) = (
        app.try_state::<MaterialItems<R>>(),
        app.try_state::<material::State>(),
    ) else {
        return;
    };
    let chosen = state.choice();
    let plan = state.applied();
    for (choice, item) in &items.0 {
        let _ = item.set_checked(*choice == chosen);
        let _ = item.set_text(material_text(*choice, *choice == chosen, plan));
    }
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

    #[test]
    fn material_labels_explain_fallbacks() {
        use lumen_windows::material::Corners;
        let plan = |material, reason| {
            Some(Plan {
                material,
                corners: Corners::Round,
                reason,
            })
        };
        let acrylic = plan(Material::Acrylic, Reason::AsRequested);
        assert_eq!(
            material_text(MaterialChoice::Auto, true, acrylic),
            "Automatic (Acrylic)"
        );
        assert_eq!(
            material_text(MaterialChoice::Auto, false, acrylic),
            "Automatic"
        );
        assert_eq!(
            material_text(MaterialChoice::Acrylic, true, acrylic),
            "Acrylic"
        );
        assert_eq!(
            material_text(
                MaterialChoice::Mica,
                true,
                plan(Material::Solid, Reason::TransparencyOff)
            ),
            "Mica (using Solid: transparency effects are off)"
        );
        assert_eq!(
            material_text(
                MaterialChoice::Auto,
                true,
                plan(Material::Solid, Reason::UnsupportedBuild)
            ),
            "Automatic (using Solid: needs Windows 11 22H2)"
        );
        assert_eq!(material_text(MaterialChoice::Solid, true, None), "Solid");
    }
}
