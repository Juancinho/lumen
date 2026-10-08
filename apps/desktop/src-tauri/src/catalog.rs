//! Keeps the app/file catalog current (T107): a low-key background thread syncs the
//! Start-menu apps, then the user's standard folders, at start-up and every
//! [`RESYNC_EVERY`]. The UI re-runs its query when a pass changed something
//! (`lumen:catalog-changed`). Incremental watching is T207; configurable roots come with
//! the indexing settings.

use std::path::PathBuf;
use std::time::Duration;

use lumen_catalog::apps::start_menu_dirs;
use lumen_catalog::{sync_apps, sync_files};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::Store;
use tauri::{App, AppHandle, Emitter, Manager, Runtime};

use crate::{overlay, settings};

/// Event (no payload) after a sync pass wrote changes. Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_CHANGED: &str = "lumen:catalog-changed";

pub(crate) const RESYNC_EVERY: Duration = Duration::from_secs(30 * 60);

/// Wait before the first pass so start-up and the first show stay quiet.
const START_DELAY: Duration = Duration::from_secs(2);

/// Default inventory roots: the user's standard folders that exist, without duplicates
/// (Documents may already live inside OneDrive).
pub(crate) fn default_roots(candidates: Vec<Option<PathBuf>>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for dir in candidates.into_iter().flatten() {
        if !dir.is_dir() {
            continue;
        }
        // Nested roots would be scanned twice; keep the outermost.
        if roots.iter().any(|r| dir.starts_with(r)) {
            continue;
        }
        roots.retain(|r| !r.starts_with(&dir));
        roots.push(dir);
    }
    roots
}

pub(crate) fn start<R: Runtime>(app: &App<R>) {
    let Some(db) = settings::db_path(app) else {
        return;
    };
    let path = app.path();
    let roots = default_roots(vec![
        path.desktop_dir().ok(),
        path.document_dir().ok(),
        path.download_dir().ok(),
        path.picture_dir().ok(),
        path.audio_dir().ok(),
        path.video_dir().ok(),
    ]);
    let handle = app.handle().clone();
    let spawned = std::thread::Builder::new()
        .name("lumen-catalog".into())
        .spawn(move || {
            std::thread::sleep(START_DELAY);
            loop {
                pass(&handle, &db, &roots);
                std::thread::sleep(RESYNC_EVERY);
            }
        });
    if let Err(err) = spawned {
        eprintln!("lumen: catalog thread failed to start: {err}");
    }
}

fn pass<R: Runtime>(app: &AppHandle<R>, db: &std::path::Path, roots: &[PathBuf]) {
    let started = std::time::Instant::now();
    let mut store = match Store::open_writer(db) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("lumen: catalog sync skipped: {err}");
            return;
        }
    };
    // `updated` counts every item seen again (metadata refresh), so it is not a change.
    match sync_apps(&mut store, &start_menu_dirs()) {
        Ok(report) => {
            let w = report.written;
            notify(app, w.inserted + w.moved + report.removed > 0);
        }
        Err(err) => eprintln!("lumen: app catalog sync failed: {err}"),
    }
    crate::diag::record("catalog_apps_ms", started.elapsed().as_secs_f64() * 1000.0);
    if !roots.is_empty() {
        let opts = ScanOptions {
            roots: roots.to_vec(),
            exclusions: Exclusions::default(),
            identity: true,
        };
        match sync_files(&mut store, &opts, None) {
            Ok(report) => {
                let w = report.written;
                notify(app, w.inserted + w.moved + report.removed > 0);
            }
            Err(err) => eprintln!("lumen: file catalog sync failed: {err}"),
        }
    }
    let _ = store.checkpoint();
    crate::diag::record("catalog_pass_ms", started.elapsed().as_secs_f64() * 1000.0);
}

fn notify<R: Runtime>(app: &AppHandle<R>, changed: bool) {
    if changed && let Err(err) = app.emit_to(overlay::WINDOW_LABEL, EVENT_CHANGED, ()) {
        eprintln!("lumen: emit {EVENT_CHANGED} failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_exist_and_do_not_nest() {
        let base = std::env::temp_dir().join(format!("lumen-roots-{}", std::process::id()));
        let docs = base.join("OneDrive").join("Documents");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(base.join("Desktop")).unwrap();
        let roots = default_roots(vec![
            Some(docs.clone()),
            Some(base.join("Desktop")),
            None,
            Some(base.join("missing")),
            Some(base.join("OneDrive")), // contains Documents: replaces it
            Some(base.join("Desktop")),  // duplicate
        ]);
        assert_eq!(roots, [base.join("Desktop"), base.join("OneDrive")]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
