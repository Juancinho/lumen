//! The shell's handle on Lumen's settings store (SQLite `settings` table, ADR-017).
//!
//! The database lives in the per-user app-data directory. If it cannot be opened Lumen still
//! runs with defaults; changes are then kept for the session only.

use std::path::PathBuf;
use std::sync::Mutex;

use lumen_storage::Store;
use tauri::{App, Manager, Runtime};

/// Database file name inside the app-data directory.
pub(crate) const DB_FILE: &str = "lumen.db";

/// Managed state: `None` when the store could not be opened.
pub(crate) struct Settings(pub(crate) Mutex<Option<Store>>);

pub(crate) fn db_path<R: Runtime>(app: &App<R>) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(DB_FILE))
}

pub(crate) fn open<R: Runtime>(app: &App<R>) -> Settings {
    let store = db_path(app).and_then(|path| {
        if let Some(dir) = path.parent()
            && let Err(err) = std::fs::create_dir_all(dir)
        {
            eprintln!("lumen: cannot create {}: {err}", dir.display());
            return None;
        }
        Store::open_writer(&path)
            .map_err(|err| eprintln!("lumen: settings store unavailable: {err}"))
            .ok()
    });
    Settings(Mutex::new(store))
}

/// JSON string value of `key` (e.g. `"Alt+Space"` → `Alt+Space`).
pub(crate) fn get_string(settings: &Settings, key: &str) -> Option<String> {
    let guard = settings.0.lock().ok()?;
    let raw = guard.as_ref()?.setting(key).ok()??;
    serde_json::from_str::<String>(&raw).ok()
}

pub(crate) fn set_string(settings: &Settings, key: &str, value: &str) {
    let Ok(guard) = settings.0.lock() else { return };
    let Some(store) = guard.as_ref() else { return };
    let json = serde_json::to_string(value).unwrap_or_default();
    if let Err(err) = store.set_setting(key, &json) {
        eprintln!("lumen: could not save {key}: {err}");
    }
}
