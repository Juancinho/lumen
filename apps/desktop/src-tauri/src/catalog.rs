//! Keeps the app/file catalog current (T107, T111): a background thread syncs the
//! Start-menu apps, then the user's indexed locations, at start-up, every [`RESYNC_EVERY`],
//! and right away when the locations or exclusions change (the running pass is cancelled —
//! nothing is removed by a cancelled pass). The UI re-runs its query on
//! `lumen:catalog-changed`, sent during long passes too so a small location is not held back
//! by a big drive. Native change hints are reconciled by this same writer (T207).
//!
//! The same thread then runs content indexing (`indexing.rs`, T202): the content pass after
//! every catalog pass, and embedding-queue slices while chunks are pending — one SQLite
//! writer for all of it (ADR-025/029).
//!
//! Locations live in one versioned setting (`index.locations`, `lumen_catalog::locations`).
//! Until the user edits them they are the standard folders and nothing is saved; a value
//! written by a newer Lumen is used read-only and never overwritten.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use lumen_catalog::apps::start_menu_dirs;
use lumen_catalog::locations::{IndexLocations, LocationsError, SETTING_KEY};
use lumen_catalog::{
    LocationState, location_states, sync_apps, sync_changes_with_content_scope,
    sync_files_with_progress,
};
use lumen_core::CancellationToken;
use lumen_indexer::watch::{Batch, NativeWatch, Notification, Pending};
use lumen_storage::Store;
use tauri::{App, AppHandle, Emitter, Manager, Runtime};

use crate::indexing::{self, Next};
use crate::{overlay, settings, tray};

/// Event (no payload) when a sync pass made new entries searchable or removed some.
/// Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_CHANGED: &str = "lumen:catalog-changed";

pub(crate) const RESYNC_EVERY: Duration = Duration::from_secs(30 * 60);

/// Wait before the first pass so start-up and the first show stay quiet.
const START_DELAY: Duration = Duration::from_secs(2);

/// Minimum spacing of `lumen:catalog-changed` during a pass.
const PROGRESS_EVERY: Duration = Duration::from_millis(750);

/// Standard folders that exist, without duplicates or nesting (Documents may already
/// live inside OneDrive).
pub(crate) fn default_roots(candidates: Vec<Option<PathBuf>>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for dir in candidates.into_iter().flatten() {
        if !dir.is_dir() {
            continue;
        }
        if roots.iter().any(|r| dir.starts_with(r)) {
            continue;
        }
        roots.retain(|r| !r.starts_with(&dir));
        roots.push(dir);
    }
    roots
}

/// System folders pre-excluded when the system drive itself becomes a location (shown to
/// the user, removable). Apps keep coming from the Start menu.
pub(crate) fn system_drive_prefill(location: &Path) -> Vec<PathBuf> {
    let Some(drive) = std::env::var_os("SystemDrive") else {
        return Vec::new();
    };
    // `%SystemDrive%` is `C:`; its root is `C:\`.
    let root = PathBuf::from(format!("{}\\", drive.to_string_lossy()));
    let same = location
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
        == root
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase();
    if !same {
        return Vec::new();
    }
    let mut out: Vec<PathBuf> = [
        "Windows",
        "Program Files",
        "Program Files (x86)",
        "ProgramData",
    ]
    .iter()
    .map(|d| root.join(d))
    .collect();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("Temp"));
    }
    out
}

/// What the tray shows for one location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocationView {
    pub(crate) path: String,
    pub(crate) state: Option<LocationState>,
}

#[derive(Default)]
struct Control {
    /// A pass should start now (an edit or start-up).
    wanted: bool,
    /// Indexing work may be possible now (resume): wake without a catalog pass.
    kick: bool,
    running: Option<CancellationToken>,
    /// Only embedding slices are preempted by filesystem events; initial/full inventories
    /// must finish even while another application is writing continuously.
    embedding: bool,
    pending: Pending,
}

/// Managed state: the locations model and the sync thread's control.
pub(crate) struct Catalog {
    model: Mutex<IndexLocations>,
    /// The stored value came from a newer Lumen (or the store is unavailable): no saving.
    read_only: bool,
    /// `true` once the user edited the list (so it is saved and authoritative).
    saved: Mutex<bool>,
    states: Mutex<Vec<(String, LocationState)>>,
    control: Mutex<Control>,
    wake: Condvar,
}

impl Catalog {
    pub(crate) fn locations(&self) -> IndexLocations {
        self.model
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn views(&self) -> Vec<LocationView> {
        let model = self.locations();
        let states = self.states.lock().unwrap_or_else(PoisonError::into_inner);
        model
            .locations
            .iter()
            .map(|l| LocationView {
                path: l.path.clone(),
                state: states.iter().find(|(p, _)| p == &l.path).map(|(_, s)| *s),
            })
            .collect()
    }

    fn request_pass(&self) {
        let mut c = self.control.lock().unwrap_or_else(PoisonError::into_inner);
        c.wanted = true;
        if let Some(running) = &c.running {
            running.cancel();
        }
        drop(c);
        self.wake.notify_all();
    }

    fn request_work(&self) {
        self.control
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kick = true;
        self.wake.notify_all();
    }
}

/// Wakes the indexing thread without a catalog pass (e.g. indexing resumed).
pub(crate) fn request_work<R: Runtime>(app: &AppHandle<R>) {
    if let Some(catalog) = app.try_state::<Catalog>() {
        catalog.request_work();
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Loads the locations model (stored value, else the standard folders).
fn load<R: Runtime>(app: &App<R>, standard: &[PathBuf]) -> (IndexLocations, bool, bool) {
    let settings = app.state::<settings::Settings>();
    let store_ok = settings.0.lock().is_ok_and(|g| g.is_some());
    match settings::get_raw(&settings, SETTING_KEY).map(|raw| IndexLocations::parse(&raw)) {
        Some(Ok(model)) => (model, true, !store_ok),
        Some(Err(LocationsError::NewerVersion(v))) => {
            eprintln!("lumen: index.locations version {v} is newer; using it read-only");
            (IndexLocations::standard(standard, 0), false, true)
        }
        Some(Err(err)) => {
            eprintln!("lumen: {err}; using the standard folders until you edit them");
            (IndexLocations::standard(standard, 0), false, !store_ok)
        }
        None => (IndexLocations::standard(standard, 0), false, !store_ok),
    }
}

pub(crate) fn start<R: Runtime>(app: &App<R>) {
    let path = app.path();
    let standard = default_roots(vec![
        path.desktop_dir().ok(),
        path.document_dir().ok(),
        path.download_dir().ok(),
        path.picture_dir().ok(),
        path.audio_dir().ok(),
        path.video_dir().ok(),
    ]);
    let (model, saved, read_only) = load(app, &standard);
    app.manage(Catalog {
        model: Mutex::new(model),
        read_only,
        saved: Mutex::new(saved),
        states: Mutex::new(Vec::new()),
        control: Mutex::new(Control {
            wanted: true,
            ..Control::default()
        }),
        wake: Condvar::new(),
    });
    let Some(db) = settings::db_path(app) else {
        return;
    };
    let handle = app.handle().clone();
    let spawned = std::thread::Builder::new()
        .name("lumen-catalog".into())
        .spawn(move || {
            std::thread::sleep(START_DELAY);
            let mut next = Next::Idle;
            let mut last_full: Option<Instant> = None;
            let mut watch = None;
            let mut content = indexing::ContentProgress::default();
            loop {
                let (token, full, batch) = wait_for_work(&handle, next, last_full);
                if full {
                    content = indexing::ContentProgress::default();
                    // Re-register before scanning: restore offline/deleted roots and close
                    // the registration gap with this inventory. Old events can only add hints.
                    watch = install_watch(&handle, &db);
                    if !batch.changes.is_empty() {
                        incremental_pass(&handle, &db, &batch, &token);
                    }
                    pass(&handle, &db, &token);
                    if !token.is_cancelled() {
                        last_full = Some(Instant::now());
                    }
                    tray::refresh_locations(&handle);
                } else if !batch.changes.is_empty() {
                    content = indexing::ContentProgress::default();
                    incremental_pass(&handle, &db, &batch, &token);
                }
                let model = handle.state::<Catalog>().locations();
                let more_content = !token.is_cancelled()
                    && indexing::content_pass(&handle, &db, &model, &token, &mut content);
                let catalog = handle.state::<Catalog>();
                let pending = {
                    let mut c = catalog
                        .control
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    c.embedding = true;
                    c.pending.delay(Instant::now()).is_some()
                };
                next = if token.is_cancelled() || pending {
                    Next::More
                } else {
                    let embedding = indexing::embed_slice(&handle, &db, &token);
                    content.next(more_content, embedding)
                };
                handle
                    .state::<Catalog>()
                    .control
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .running = None;
                tray::refresh_indexing(&handle);
                // Keep the native handle alive across all content/embedding slices.
                let _ = &watch;
            }
        });
    if let Err(err) = spawned {
        eprintln!("lumen: catalog thread failed to start: {err}");
    }
}

/// How long the thread may sleep before its next round.
pub(crate) fn wait_time(next: Next, since_full: Option<Duration>) -> Duration {
    let due = since_full.map_or(Duration::ZERO, |d| RESYNC_EVERY.saturating_sub(d));
    match next {
        Next::More => Duration::ZERO,
        Next::RetryIn(d) => d.min(due),
        Next::Idle => due,
    }
}

/// Blocks until catalog work is wanted, indexing work may be possible, or the wait for
/// `next` ends. Returns the round's token and whether it starts with a catalog pass.
fn wait_for_work<R: Runtime>(
    app: &AppHandle<R>,
    next: Next,
    last_full: Option<Instant>,
) -> (CancellationToken, bool, Batch) {
    let catalog = app.state::<Catalog>();
    let mut c = catalog
        .control
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    loop {
        if c.wanted || c.kick || last_full.is_none_or(|t| t.elapsed() >= RESYNC_EVERY) {
            break;
        }
        let timeout = c
            .pending
            .delay(Instant::now())
            .unwrap_or_else(|| wait_time(next, last_full.map(|t| t.elapsed())));
        if timeout.is_zero() {
            break;
        }
        let (guard, result) = catalog
            .wake
            .wait_timeout(c, timeout)
            .unwrap_or_else(PoisonError::into_inner);
        c = guard;
        if result.timed_out() && c.pending.delay(Instant::now()).is_none() {
            break;
        }
    }
    let batch = if c.wanted || c.pending.delay(Instant::now()).is_some_and(|d| d.is_zero()) {
        c.pending.take()
    } else {
        Batch::default()
    };
    let full = c.wanted || batch.rescan || last_full.is_none_or(|t| t.elapsed() >= RESYNC_EVERY);
    c.wanted = false;
    c.kick = false;
    let token = CancellationToken::new();
    c.running = Some(token.clone());
    c.embedding = false;
    (token, full, batch)
}

fn install_watch<R: Runtime>(app: &AppHandle<R>, db: &Path) -> Option<NativeWatch> {
    let opts = app.state::<Catalog>().locations().scan_options(true);
    let roots = opts.roots.clone();
    let internal = db.parent().map(Path::to_path_buf);
    let handle = app.clone();
    let mut watch = match NativeWatch::new(move |mut event: Notification| {
        if let Ok(e) = &mut event {
            let root_changed = lumen_indexer::watch::root_lifecycle(e)
                && e.paths.iter().any(|p| opts.roots.contains(p));
            e.paths.retain(|p| {
                if internal.as_ref().is_some_and(|d| p.starts_with(d)) {
                    return false;
                }
                if !opts.roots.iter().any(|r| p.starts_with(r)) {
                    return false;
                }
                if opts.exclusions.user_paths.iter().any(|d| p.starts_with(d)) {
                    return false;
                }
                // Cheap ancestor filtering keeps excluded dependency/.git write storms
                // out of the bounded queue. Conditional venv/build checks stay in scan.
                !p.ancestors()
                    .skip(1)
                    .take_while(|a| !opts.roots.iter().any(|r| r == a))
                    .any(|a| {
                        let name = a
                            .file_name()
                            .map(|n| n.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        opts.exclusions
                            .user_names
                            .iter()
                            .any(|n| n.to_lowercase() == name)
                            || opts
                                .exclusions
                                .default_names
                                .iter()
                                .any(|n| n.eq_ignore_ascii_case(&name) && name != "venv")
                            || (opts.exclusions.system_defaults
                                && lumen_indexer::SYSTEM_EXCLUSIONS
                                    .iter()
                                    .any(|n| n.eq_ignore_ascii_case(&name)))
                    })
            });
            if e.paths.is_empty() && !e.need_rescan() {
                return;
            }
            if root_changed {
                let catalog = handle.state::<Catalog>();
                let mut c = catalog
                    .control
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                c.pending.require_rescan(Instant::now());
                if c.embedding
                    && let Some(running) = &c.running
                {
                    running.cancel();
                }
                drop(c);
                catalog.wake.notify_all();
                return;
            }
        }
        let catalog = handle.state::<Catalog>();
        let mut c = catalog
            .control
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let accepted = c.pending.push(event, Instant::now());
        if accepted
            && c.embedding
            && let Some(running) = &c.running
        {
            running.cancel();
        }
        drop(c);
        if accepted {
            catalog.wake.notify_all();
        }
    }) {
        Ok(watch) => watch,
        Err(_) => {
            eprintln!("lumen: native watching unavailable; periodic inventory remains enabled");
            return None;
        }
    };
    let failed = watch.set_roots(&roots);
    if !failed.is_empty() {
        eprintln!(
            "lumen: {} watch registrations unavailable; periodic inventory will retry",
            failed.len()
        );
    }
    Some(watch)
}

fn incremental_pass<R: Runtime>(
    app: &AppHandle<R>,
    db: &Path,
    batch: &Batch,
    token: &CancellationToken,
) {
    let started = Instant::now();
    let model = app.state::<Catalog>().locations();
    let result = Store::open_writer(db).and_then(|mut store| {
        sync_changes_with_content_scope(
            &mut store,
            &model.scan_options(true),
            &batch.changes,
            Some(token),
            &|path| model.indexes_content(path),
        )
    });
    match result {
        Ok(report) => {
            notify(
                app,
                report.written.inserted
                    + report.written.updated
                    + report.written.moved
                    + report.removed
                    > 0,
            );
            crate::diag::record(
                "catalog_incremental_ms",
                started.elapsed().as_secs_f64() * 1000.0,
            );
            // Failed reads retain catalog entries and retry during the recovery inventory.
            if !report.scan.is_complete() {
                app.state::<Catalog>()
                    .control
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .pending
                    .require_rescan(Instant::now());
            }
        }
        Err(_) => {
            eprintln!("lumen: incremental inventory failed; scheduling recovery");
            app.state::<Catalog>()
                .control
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pending
                .require_rescan(Instant::now());
        }
    }
}

fn pass<R: Runtime>(app: &AppHandle<R>, db: &Path, token: &CancellationToken) {
    let started = Instant::now();
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

    let model = app.state::<Catalog>().locations();
    let opts = model.scan_options(true);
    match lumen_catalog::exclusions::prune_user_exclusions(&mut store, &model, token) {
        Ok(removed) => notify(app, removed > 0),
        Err(err) => eprintln!("lumen: exclusion cleanup failed: {err}"),
    }
    if token.is_cancelled() {
        return;
    }
    let mut last_emit = Instant::now();
    let mut last_new = 0;
    let mut progress = |w: &lumen_storage::UpsertStats| {
        let new = w.inserted + w.moved;
        if new > last_new && last_emit.elapsed() >= PROGRESS_EVERY {
            last_new = new;
            last_emit = Instant::now();
            notify(app, true);
        }
    };
    match sync_files_with_progress(&mut store, &opts, Some(token), &mut progress) {
        Ok(report) => {
            let w = report.written;
            notify(app, w.inserted + w.moved + report.removed > 0);
            let states = location_states(&opts.roots, &report.scan);
            if !report.scan.cancelled {
                *app.state::<Catalog>()
                    .states
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = model
                    .locations
                    .iter()
                    .map(|l| l.path.clone())
                    .zip(states)
                    .collect();
            }
            crate::diag::record(
                "catalog_entries",
                f64::from(u32::try_from(report.scan.emitted()).unwrap_or(u32::MAX)),
            );
            crate::diag::record(
                "catalog_excluded",
                f64::from(u32::try_from(report.scan.excluded.len()).unwrap_or(u32::MAX)),
            );
        }
        Err(err) => eprintln!("lumen: file catalog sync failed: {err}"),
    }
    let _ = store.checkpoint();
    crate::diag::record("catalog_pass_ms", started.elapsed().as_secs_f64() * 1000.0);
}

pub(crate) fn notify<R: Runtime>(app: &AppHandle<R>, changed: bool) {
    if changed
        && overlay::is_shown()
        && let Err(err) = app.emit_to(overlay::WINDOW_LABEL, EVENT_CHANGED, ())
    {
        eprintln!("lumen: emit {EVENT_CHANGED} failed: {err}");
    }
}

/// Applies an edit to the locations model; saves it, refreshes the tray and starts a pass.
/// Returns whether anything changed (`false` also when read-only).
pub(crate) fn edit<R: Runtime>(
    app: &AppHandle<R>,
    change: impl FnOnce(&mut IndexLocations) -> bool,
) -> bool {
    let catalog = app.state::<Catalog>();
    if catalog.read_only {
        eprintln!("lumen: indexed locations are read-only (newer settings or no store)");
        return false;
    }
    let json = {
        let mut model = catalog.model.lock().unwrap_or_else(PoisonError::into_inner);
        if !change(&mut model) {
            return false;
        }
        model.to_json()
    };
    if settings::set_raw(&app.state::<settings::Settings>(), SETTING_KEY, &json) {
        *catalog.saved.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
    catalog.request_pass();
    tray::refresh_locations(app);
    true
}

pub(crate) fn add_location<R: Runtime>(app: &AppHandle<R>, path: &Path) -> bool {
    let prefill = system_drive_prefill(path);
    edit(app, |m| m.add_location(path, now_ms(), &prefill))
}

pub(crate) fn remove_location<R: Runtime>(app: &AppHandle<R>, path: &str) -> bool {
    edit(app, |m| m.remove_location(path))
}

pub(crate) fn exclude_path<R: Runtime>(app: &AppHandle<R>, path: &Path) -> bool {
    edit(app, |m| m.exclude_path(path))
}

pub(crate) fn unexclude_path<R: Runtime>(app: &AppHandle<R>, path: &str) -> bool {
    edit(app, |m| m.unexclude_path(path))
}

pub(crate) fn set_extension<R: Runtime>(
    app: &AppHandle<R>,
    extension: &str,
    excluded: bool,
) -> bool {
    edit(app, |m| m.set_extension_excluded(extension, excluded))
}

/// Tray toggle: index the contents of one location, or only its names.
pub(crate) fn set_content<R: Runtime>(app: &AppHandle<R>, path: &str, enabled: bool) -> bool {
    edit(app, |m| m.set_content(path, enabled))
}

pub(crate) fn set_default<R: Runtime>(app: &AppHandle<R>, rule: &str, enabled: bool) -> bool {
    edit(app, |m| {
        if m.default_enabled(rule) == enabled {
            return false;
        }
        if enabled {
            // Re-enabling one rule of a switched-off group turns the group back on.
            if rule == lumen_catalog::locations::BUILD_DIRS_RULE {
                m.default_rules.build_next_to_marker = true;
            } else {
                m.default_rules.dev_noise = true;
            }
        }
        m.set_default_enabled(rule, enabled);
        true
    })
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

    #[test]
    fn the_thread_sleeps_until_the_next_due_work() {
        let min = Duration::from_secs(60);
        assert_eq!(wait_time(Next::More, Some(min)), Duration::ZERO);
        assert_eq!(wait_time(Next::Idle, None), Duration::ZERO, "never synced");
        assert_eq!(wait_time(Next::Idle, Some(min)), RESYNC_EVERY - min);
        assert_eq!(wait_time(Next::RetryIn(min), Some(min)), min);
        assert_eq!(
            wait_time(Next::RetryIn(RESYNC_EVERY), Some(RESYNC_EVERY - min)),
            min,
            "the resync comes first"
        );
    }

    #[test]
    fn only_the_system_drive_gets_prefilled_exclusions() {
        // Off Windows there is no %SystemDrive%: nothing is prefilled.
        if std::env::var_os("SystemDrive").is_none() {
            assert!(system_drive_prefill(Path::new("/")).is_empty());
        } else {
            assert!(!system_drive_prefill(Path::new("C:\\")).is_empty());
            assert!(system_drive_prefill(Path::new("C:\\Users")).is_empty());
        }
    }
}
