//! Application catalog → `items` (`source = 'apps'`).
//!
//! Primary source on Windows: the shell's AppsFolder (every Start-menu app, packaged and
//! desktop; [`lumen_windows::start_apps`]). Fallback (enumeration failure, other platforms,
//! tests): shortcuts (`.lnk`, `.url`, `.appref-ms`) under the Start-menu folders.
//! A failed discovery never removes known apps.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lumen_indexer::{EntryKind, Exclusions, ScanOptions, scan};
use lumen_storage::{CatalogEntry, ItemKind, Source, StorageError, Store, UpsertStats};

use crate::path::encode;
use crate::text::fold;

/// Shortcut extensions treated as applications in Start-menu folders.
pub const SHORTCUT_EXTENSIONS: &[&str] = &["lnk", "url", "appref-ms"];

/// Where an application was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSource {
    AppsFolder,
    StartMenuShortcut,
}

/// One discovered application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredApp {
    pub name: String,
    /// `items.canonical_path`: the shortcut path, or `shell:AppsFolder\<parsing name>`.
    pub location: String,
    /// What the launcher opens (same as `location` for both sources today).
    pub launch_target: String,
    pub source: AppSource,
}

/// Outcome of [`sync_apps`].
#[derive(Debug, Clone, Default)]
pub struct AppsReport {
    pub scan_id: i64,
    pub discovered: u64,
    pub source: Option<AppSourceUsed>,
    pub written: UpsertStats,
    pub removed: u64,
    /// Why the AppsFolder enumeration was not used (logs only).
    pub apps_folder_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSourceUsed {
    AppsFolder,
    StartMenuShortcuts,
}

/// The per-machine and per-user Start-menu program folders (Windows), from the environment.
#[must_use]
pub fn start_menu_dirs() -> Vec<PathBuf> {
    let rel = Path::new("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join(&rel))
        .filter(|p| p.is_dir())
        .collect()
}

/// Applications from shortcut files under `dirs`.
#[must_use]
pub fn shortcuts_in(dirs: &[PathBuf]) -> Vec<DiscoveredApp> {
    let opts = ScanOptions {
        roots: dirs.to_vec(),
        exclusions: Exclusions::default(),
        identity: false,
    };
    let mut apps = Vec::new();
    scan(
        &opts,
        |e| {
            if e.kind != EntryKind::File {
                return;
            }
            let ext = e
                .path
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase());
            if !ext.is_some_and(|x| SHORTCUT_EXTENSIONS.contains(&x.as_str())) {
                return;
            }
            let Some(name) = e.path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                return;
            };
            let location = encode(&e.path).text;
            apps.push(DiscoveredApp {
                name,
                launch_target: location.clone(),
                location,
                source: AppSource::StartMenuShortcut,
            });
        },
        None,
    );
    apps
}

fn from_apps_folder() -> Result<Vec<DiscoveredApp>, String> {
    let apps = lumen_windows::start_apps()?;
    if apps.is_empty() {
        return Err("AppsFolder returned no applications".into());
    }
    Ok(apps
        .into_iter()
        .map(|a| {
            let uri = a.shell_uri();
            DiscoveredApp {
                name: a.name,
                location: uri.clone(),
                launch_target: uri,
                source: AppSource::AppsFolder,
            }
        })
        .collect())
}

/// Discovers applications: AppsFolder first, Start-menu shortcuts in `fallback_dirs` otherwise.
/// Returns the apps, which source was used, and the AppsFolder error if it was skipped.
#[must_use]
pub fn discover(fallback_dirs: &[PathBuf]) -> (Vec<DiscoveredApp>, AppSourceUsed, Option<String>) {
    match from_apps_folder() {
        Ok(apps) => (apps, AppSourceUsed::AppsFolder, None),
        Err(err) => (
            shortcuts_in(fallback_dirs),
            AppSourceUsed::StartMenuShortcuts,
            Some(err),
        ),
    }
}

/// Writes `apps` as the application catalog (pass of `source = 'apps'`), removing apps no
/// longer present — unless `apps` is empty (a failed discovery must not wipe the catalog).
///
/// # Errors
/// Storage failures.
pub fn write_apps(
    store: &mut Store,
    apps: &[DiscoveredApp],
) -> Result<(i64, UpsertStats, u64), StorageError> {
    let scan_id = store.begin_scan(Source::Apps)?;
    // The same app can be listed twice (per-user + per-machine shortcut): keep one location.
    let mut seen = HashSet::new();
    let unique: Vec<&DiscoveredApp> = apps
        .iter()
        .filter(|a| seen.insert(a.location.clone()))
        .collect();
    let keys: Vec<String> = unique.iter().map(|a| fold(&a.name)).collect();
    let entries: Vec<CatalogEntry<'_>> = unique
        .iter()
        .zip(&keys)
        .map(|(a, key)| CatalogEntry {
            kind: ItemKind::Application,
            source: Source::Apps,
            path: &a.location,
            raw_path: None,
            name: &a.name,
            name_key: key,
            extension: None,
            volume_id: None,
            file_id: None,
            launch_target: Some(&a.launch_target),
            attributes: 0,
            size_bytes: None,
            modified_at: None,
            created_at: None,
            error: None,
        })
        .collect();
    let written = store.upsert_entries(scan_id, &entries)?;
    let removed = if entries.is_empty() {
        0
    } else {
        let doomed: Vec<i64> = store
            .unseen_items(scan_id, Source::Apps)?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        store.delete_items(&doomed)?
    };
    store.finish_scan(
        scan_id,
        !entries.is_empty(),
        entries.len() as u64,
        removed,
        0,
    )?;
    Ok((scan_id, written, removed))
}

/// Discovers and writes the application catalog.
///
/// # Errors
/// Storage failures.
pub fn sync_apps(store: &mut Store, fallback_dirs: &[PathBuf]) -> Result<AppsReport, StorageError> {
    let (apps, used, err) = discover(fallback_dirs);
    let (scan_id, written, removed) = write_apps(store, &apps)?;
    Ok(AppsReport {
        scan_id,
        discovered: apps.len() as u64,
        source: Some(used),
        written,
        removed,
        apps_folder_error: err,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lumen_storage::SearchBudget;

    use super::*;

    struct Tmp(PathBuf);

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(tag: &str) -> Tmp {
        let p =
            std::env::temp_dir().join(format!("lumen-catalog-apps-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }

    #[test]
    fn shortcuts_become_apps_and_vanished_ones_are_removed() {
        let t = tmp("lnk");
        let menu = t.0.join("Programs");
        fs::create_dir_all(menu.join("Accessories")).unwrap();
        fs::write(menu.join("Spotify.lnk"), b"").unwrap();
        fs::write(menu.join("Accessories/Notepad.lnk"), b"").unwrap();
        fs::write(menu.join("desktop.ini"), b"").unwrap();
        fs::write(menu.join("Docs.url"), b"").unwrap();
        let found = shortcuts_in(std::slice::from_ref(&menu));
        let mut names: Vec<_> = found.iter().map(|a| a.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["Docs", "Notepad", "Spotify"]);

        let mut store = Store::open_writer(&t.0.join("c.db")).unwrap();
        let (_, written, _) = write_apps(&mut store, &found).unwrap();
        assert_eq!(written.inserted, 3);
        let hits = store
            .search_names("spotify", 5, &SearchBudget::unbounded())
            .unwrap();
        assert_eq!(hits[0].item.kind, ItemKind::Application);
        assert!(
            hits[0]
                .item
                .launch_target
                .as_deref()
                .is_some_and(|t| t.ends_with("Spotify.lnk"))
        );

        fs::remove_file(menu.join("Docs.url")).unwrap();
        let (_, _, removed) = write_apps(&mut store, &shortcuts_in(&[menu])).unwrap();
        assert_eq!(removed, 1);
        // A failed discovery (nothing found) keeps the catalog.
        let (_, _, removed) = write_apps(&mut store, &[]).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(store.count_items(Source::Apps).unwrap(), 2);
    }

    #[test]
    fn duplicate_locations_are_written_once() {
        let t = tmp("dup");
        let app = DiscoveredApp {
            name: "Calculator".into(),
            location: "shell:AppsFolder\\Calc!App".into(),
            launch_target: "shell:AppsFolder\\Calc!App".into(),
            source: AppSource::AppsFolder,
        };
        let mut store = Store::open_writer(&t.0.join("c.db")).unwrap();
        let (_, written, _) = write_apps(&mut store, &[app.clone(), app]).unwrap();
        assert_eq!(written.inserted, 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn discovery_falls_back_to_shortcuts_off_windows() {
        let t = tmp("fallback");
        fs::write(t.0.join("Tool.lnk"), b"").unwrap();
        let (apps, used, err) = discover(std::slice::from_ref(&t.0));
        assert_eq!(used, AppSourceUsed::StartMenuShortcuts);
        assert!(err.is_some());
        assert_eq!(apps.len(), 1);
    }
}
