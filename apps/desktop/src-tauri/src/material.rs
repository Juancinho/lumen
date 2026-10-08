//! Overlay window material (T004, ADR-024): applies the plan from
//! `lumen_windows::material` to the overlay window and tells the UI which surface to paint.
//!
//! - Choice: `LUMEN_MATERIAL` (benchmarks/diagnostics) or the saved `appearance.material`
//!   setting (tray → Window material), default `auto` (Acrylic where supported).
//! - The window is transparent (`tauri.conf.json`); Acrylic/Mica are DWM system backdrops
//!   applied through Tauri's window effects, and the UI paints a tint over them
//!   (`src/app/material.css`). `solid` clears the backdrop and the UI paints opaque.
//! - System settings (transparency effects, high contrast) are re-read before every show,
//!   so a change applies on the next show without a restart. The check costs two WinRT
//!   property reads; re-applying only happens when the plan changed.

use std::sync::Mutex;

use lumen_windows::material::{self, Corners, Material, MaterialChoice, Plan};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{App, AppHandle, Emitter, Manager, Runtime, WebviewWindow};

use crate::dto::AppearanceDto;
use crate::{overlay, settings, tray};

/// Settings key (JSON string, a [`MaterialChoice`] name).
pub(crate) const SETTING_KEY: &str = "appearance.material";

/// Environment override of the choice for this run (not saved).
pub(crate) const ENV_MATERIAL: &str = "LUMEN_MATERIAL";

/// Event carrying an [`AppearanceDto`] whenever the applied material changes.
/// Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_APPEARANCE: &str = "lumen:appearance";

#[derive(Default)]
pub(crate) struct State {
    choice: Mutex<MaterialChoice>,
    applied: Mutex<Option<Plan>>,
}

impl State {
    pub(crate) fn choice(&self) -> MaterialChoice {
        self.choice.lock().map(|c| *c).unwrap_or_default()
    }

    pub(crate) fn applied(&self) -> Option<Plan> {
        self.applied.lock().ok().and_then(|p| *p)
    }
}

/// Choice at start-up: a valid `LUMEN_MATERIAL`, else the saved setting, else `auto`.
pub(crate) fn initial_choice(env: Option<&str>, saved: Option<&str>) -> MaterialChoice {
    env.and_then(MaterialChoice::parse)
        .or_else(|| saved.and_then(MaterialChoice::parse))
        .unwrap_or_default()
}

pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let env = std::env::var(ENV_MATERIAL).ok();
    let saved = settings::get_string(&app.state::<settings::Settings>(), SETTING_KEY);
    let choice = initial_choice(env.as_deref(), saved.as_deref());
    app.manage(State {
        choice: Mutex::new(choice),
        applied: Mutex::new(None),
    });
    if let Some(window) = app.get_webview_window(overlay::WINDOW_LABEL) {
        apply(&window, true);
    }
}

/// Before each show: re-plan from the current system settings; re-apply only on change.
pub(crate) fn before_show<R: Runtime>(window: &WebviewWindow<R>) {
    let started = std::time::Instant::now();
    apply(window, false);
    crate::diag::record(
        "material_check_ms",
        started.elapsed().as_secs_f64() * 1000.0,
    );
}

/// Tray choice: save, apply now, update the menu.
pub(crate) fn choose<R: Runtime>(app: &AppHandle<R>, choice: MaterialChoice) {
    let state = app.state::<State>();
    if let Ok(mut c) = state.choice.lock() {
        *c = choice;
    }
    settings::set_string(
        &app.state::<settings::Settings>(),
        SETTING_KEY,
        choice.as_str(),
    );
    if let Some(window) = app.get_webview_window(overlay::WINDOW_LABEL) {
        apply(&window, true);
    }
}

/// What the UI should paint now (`overlay_appearance` command).
pub(crate) fn current<R: Runtime>(app: &AppHandle<R>) -> AppearanceDto {
    let state = app.state::<State>();
    let plan = state
        .applied()
        .unwrap_or_else(|| material::plan(state.choice(), material::system_appearance()));
    AppearanceDto::from(plan)
}

fn apply<R: Runtime>(window: &WebviewWindow<R>, force: bool) {
    let app = window.app_handle();
    let state = app.state::<State>();
    let plan = material::plan(state.choice(), material::system_appearance());
    let previous = state.applied();
    if !force && previous == Some(plan) {
        return;
    }
    let started = std::time::Instant::now();
    if previous.is_none_or(|p| p.material != plan.material) {
        let effects = match plan.material {
            Material::Acrylic => Some(EffectsBuilder::new().effect(Effect::Acrylic).build()),
            Material::Mica => Some(EffectsBuilder::new().effect(Effect::Mica).build()),
            Material::Solid => None,
        };
        if let Err(err) = window.set_effects(effects) {
            eprintln!("lumen: window material failed: {err}");
        }
    }
    if previous.is_none() && plan.corners == Corners::Round {
        round_corners(window);
    }
    if let Ok(mut applied) = state.applied.lock() {
        *applied = Some(plan);
    }
    if crate::diag::enabled() {
        // e.g. `material_applied_acrylic_as-requested 0.412` (scripts/t004).
        crate::diag::record(
            &format!(
                "material_applied_{}_{}",
                plan.material.as_str(),
                plan.reason.as_str()
            ),
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }
    if crate::diag::enabled() || cfg!(debug_assertions) {
        eprintln!(
            "lumen: material {} (asked {}, {}), corners {}",
            plan.material.as_str(),
            state.choice().as_str(),
            plan.reason.as_str(),
            plan.corners.as_str()
        );
    }
    if let Err(err) = window.emit_to(window.label(), EVENT_APPEARANCE, AppearanceDto::from(plan)) {
        eprintln!("lumen: emit {EVENT_APPEARANCE} failed: {err}");
    }
    tray::refresh_material(app);
}

#[cfg(windows)]
fn round_corners<R: Runtime>(window: &WebviewWindow<R>) {
    match window.hwnd() {
        Ok(hwnd) => {
            if let Err(err) = material::round_corners(hwnd.0 as isize) {
                eprintln!("lumen: rounded corners unavailable: {err}");
            }
        }
        Err(err) => eprintln!("lumen: no window handle for corners: {err}"),
    }
}

#[cfg(not(windows))]
fn round_corners<R: Runtime>(_window: &WebviewWindow<R>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_beats_saved_beats_default() {
        assert_eq!(
            initial_choice(Some("mica"), Some("solid")),
            MaterialChoice::Mica
        );
        assert_eq!(
            initial_choice(Some("bogus"), Some("solid")),
            MaterialChoice::Solid
        );
        assert_eq!(initial_choice(None, Some("nope")), MaterialChoice::Auto);
        assert_eq!(initial_choice(None, None), MaterialChoice::Auto);
    }
}
