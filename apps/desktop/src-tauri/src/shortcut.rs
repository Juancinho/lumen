//! Configurable global shortcut with conflict handling (T003).
//!
//! - The user picks one of [`CHOICES`] in the tray menu; the choice is saved
//!   (`shortcut.toggle`) and applied immediately.
//! - Registering a combination another app already owns fails on Windows; Lumen then keeps
//!   the previous shortcut and marks the choice "(in use)".
//! - First run (nothing saved) with the default taken: the first free choice is used for this
//!   session without saving, so the default is tried again next start.
//! - No free choice at all: Lumen keeps running; the tray still opens the overlay.

use std::sync::Mutex;

use tauri::{App, AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::{overlay, settings, tray};

/// Settings key of the toggle shortcut (JSON string, one of the [`CHOICES`] labels).
pub(crate) const SETTING_KEY: &str = "shortcut.toggle";

/// Offered combinations, default first. Labels are what users see and what is stored.
pub(crate) const CHOICES: [&str; 4] = [
    "Alt+Space",
    "Ctrl+Space",
    "Alt+Shift+Space",
    "Ctrl+Alt+Space",
];

/// Shortcut for a [`CHOICES`] label.
pub(crate) fn from_label(label: &str) -> Option<Shortcut> {
    let modifiers = match label {
        "Alt+Space" => Modifiers::ALT,
        "Ctrl+Space" => Modifiers::CONTROL,
        "Alt+Shift+Space" => Modifiers::ALT | Modifiers::SHIFT,
        "Ctrl+Alt+Space" => Modifiers::CONTROL | Modifiers::ALT,
        _ => return None,
    };
    Some(Shortcut::new(Some(modifiers), Code::Space))
}

/// The registered shortcut, if any.
#[derive(Default)]
pub(crate) struct Active(pub(crate) Mutex<Option<(&'static str, Shortcut)>>);

impl Active {
    pub(crate) fn label(&self) -> Option<&'static str> {
        self.0.lock().ok().and_then(|g| g.map(|(l, _)| l))
    }

    fn is(&self, shortcut: &Shortcut) -> bool {
        self.0
            .lock()
            .ok()
            .is_some_and(|g| g.is_some_and(|(_, s)| &s == shortcut))
    }
}

fn label_static(label: &str) -> Option<&'static str> {
    CHOICES.iter().copied().find(|c| *c == label)
}

/// Why a shortcut change did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyError {
    Unknown,
    InUse(String),
}

/// Makes `label` the toggle shortcut. On failure the previous one stays registered.
pub(crate) fn apply<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), ApplyError> {
    let label = label_static(label).ok_or(ApplyError::Unknown)?;
    let new = from_label(label).ok_or(ApplyError::Unknown)?;
    let active = app.state::<Active>();
    let previous = active.0.lock().ok().and_then(|g| *g);
    if previous.is_some_and(|(_, s)| s == new) {
        return Ok(());
    }
    let gs = app.global_shortcut();
    gs.register(new)
        .map_err(|e| ApplyError::InUse(e.to_string()))?;
    if let Some((old_label, old)) = previous
        && let Err(err) = gs.unregister(old)
    {
        eprintln!("lumen: could not release {old_label}: {err}");
    }
    if let Ok(mut g) = active.0.lock() {
        *g = Some((label, new));
    }
    Ok(())
}

/// Whether `label` could be registered right now (probe: register then release). The
/// active shortcut counts as available.
pub(crate) fn available<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    let Some(shortcut) = from_label(label) else {
        return false;
    };
    if app.state::<Active>().is(&shortcut) {
        return true;
    }
    let gs = app.global_shortcut();
    match gs.register(shortcut) {
        Ok(()) => {
            let _ = gs.unregister(shortcut);
            true
        }
        Err(_) => false,
    }
}

/// Order in which to try shortcuts at start-up: the saved choice alone (an explicit user
/// choice is never silently replaced), or — with nothing saved — the default, then the
/// other choices.
pub(crate) fn startup_order(saved: Option<&str>) -> Vec<&'static str> {
    match saved.and_then(label_static) {
        Some(label) => vec![label],
        None => CHOICES.to_vec(),
    }
}

/// Installs the plugin and registers the saved (or first free) shortcut.
pub(crate) fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    app.manage(Active::default());
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event: ShortcutEvent| {
                if event.state() == ShortcutState::Pressed && app.state::<Active>().is(shortcut) {
                    overlay::toggle(app);
                }
            })
            .build(),
    )?;
    let saved = settings::get_string(&app.state::<settings::Settings>(), SETTING_KEY);
    for label in startup_order(saved.as_deref()) {
        match apply(app.handle(), label) {
            Ok(()) => break,
            Err(err) => eprintln!("lumen: could not register {label}: {err:?}"),
        }
    }
    Ok(())
}

/// Tray menu choice: apply, save on success, refresh the tray.
pub(crate) fn choose<R: Runtime>(app: &AppHandle<R>, label: &str) {
    match apply(app, label) {
        Ok(()) => settings::set_string(&app.state::<settings::Settings>(), SETTING_KEY, label),
        Err(err) => eprintln!("lumen: shortcut {label} not applied: {err:?}"),
    }
    tray::refresh(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_maps_to_a_distinct_shortcut() {
        let shortcuts: Vec<Shortcut> = CHOICES.iter().map(|c| from_label(c).unwrap()).collect();
        for (i, a) in shortcuts.iter().enumerate() {
            assert!(shortcuts[i + 1..].iter().all(|b| a != b));
        }
        assert_eq!(from_label("Win+Q"), None);
        assert_eq!(CHOICES[0], "Alt+Space", "default");
    }

    #[test]
    fn saved_choice_is_never_silently_replaced() {
        assert_eq!(startup_order(Some("Ctrl+Space")), ["Ctrl+Space"]);
        assert_eq!(startup_order(None), CHOICES);
        assert_eq!(
            startup_order(Some("garbage")),
            CHOICES,
            "invalid saved value = unset"
        );
    }
}
