//! Lumen desktop shell entry point.
//!
//! Dependency direction (ADR-002): this crate depends on `lumen-core`, never the
//! reverse. Everything here is presentation/adaptation: Tauri wiring, IPC
//! commands and DTO mapping. Domain rules belong in `crates/`.

// Release builds are GUI-subsystem executables (no console window on Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod commands;
mod dto;

fn main() {
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app_info::core_info])
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("Lumen failed to start: {err}");
        std::process::exit(1);
    }
}
