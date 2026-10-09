//! Lumen desktop shell entry point.
//!
//! Dependency direction (ADR-002): this crate depends on `lumen-core`, never the
//! reverse. Everything here is presentation/adaptation: Tauri wiring, IPC
//! commands, window/tray/shortcut lifecycle and DTO mapping. Domain rules belong
//! in `crates/`.

// Release builds are GUI-subsystem executables (no console window on Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Unsafe only in `lifecycle` (WebView2 COM), under an explicit allow with SAFETY notes.
#![deny(unsafe_code)]

mod actions;
mod catalog;
mod commands;
mod diag;
mod dto;
mod gpu;
mod gpu_probe;
mod indexing;
mod instance;
mod lifecycle;
mod material;
mod overlay;
mod pdf_preview;
mod preview;
mod progress;
mod provisioning;
mod search;
mod settings;
mod shortcut;
mod tray;

use std::sync::atomic::AtomicBool;

use tauri::{Manager, WindowEvent};

use commands::overlay::ShowWhenReady;

/// Pass to start resident without showing the overlay (e.g. launch at login).
const BACKGROUND_ARG: &str = "--background";

fn main() {
    if let Some(code) = gpu_probe::child_mode() {
        std::process::exit(code);
    }
    diag::init();
    let start_hidden = std::env::args().any(|a| a == BACKGROUND_ARG);

    let result = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running instance
        // instead of creating another tray icon and a conflicting shortcut.
        .plugin(tauri_plugin_single_instance::init(
            |app, args, _cwd| match instance::parse(&args) {
                instance::InstanceCommand::Show => overlay::show(app),
                instance::InstanceCommand::Hide => overlay::hide(app),
                instance::InstanceCommand::Toggle => overlay::toggle(app),
                instance::InstanceCommand::Quit => app.exit(0),
            },
        ))
        // Native folder picker for tray → Indexed locations / Exclusions (Rust API only).
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            app.manage(settings::open(app));
            material::install(app);
            // Provisioning, then indexing, then search: each reads the one before.
            provisioning::install_state(app);
            gpu::install(app);
            indexing::install(app);
            search::install(app);
            pdf_preview::install(app.handle());
            catalog::start(app);
            shortcut::install(app)?;
            tray::install(app)?;
            gpu::discover(app.handle());
            // First show happens when the UI reports ready (`overlay_ready`).
            app.manage(ShowWhenReady(AtomicBool::new(!start_hidden)));
            diag::record("setup_ms", diag::since_start_ms());
            if diag::enabled() {
                eprintln!("lumen: webview hidden mode {}", lifecycle::mode().as_str());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != overlay::WINDOW_LABEL {
                return;
            }
            match event {
                // Launcher behaviour: clicking elsewhere dismisses.
                WindowEvent::Focused(false) => overlay::hide(window.app_handle()),
                // Alt+F4 hides; quitting is explicit (tray → Quit).
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    overlay::hide(window.app_handle());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            progress::indexing_progress,
            commands::app_info::core_info,
            commands::overlay::hide_overlay,
            commands::overlay::overlay_ready,
            commands::overlay::overlay_appearance,
            commands::overlay::resize_overlay,
            commands::search::search,
            commands::actions::list_actions,
            commands::actions::run_action,
            commands::preview::preview_result,
            commands::preview::preview_pdf_page,
            commands::preview::cancel_pdf_preview,
            commands::overlay::overlay_painted
        ])
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("Lumen failed to start: {err}");
        std::process::exit(1);
    }
}
