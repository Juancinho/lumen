//! Inventory pass → `items` (T101, ADR-018 coverage guarantee).
//!
//! Every entry the scan emits is written, including ones whose metadata could not be read
//! (stored with `status = 'error'`, still findable by name). After the walk, items the pass
//! did not see are removed — except under directories that failed to list, and never after
//! a cancelled pass, so a transient failure can never make files disappear from search.

use std::path::Path;

use lumen_core::CancellationToken;
use lumen_indexer::{EntryKind, ScanEntry, ScanOptions, ScanReport, scan};
use lumen_storage::{CatalogEntry, ItemKind, Source, StorageError, Store, UpsertStats};

use crate::path::encode;
use crate::text::{fold, name_parts, path_parts};

/// Entries written per transaction.
pub const BATCH: usize = 2_000;

/// Parent folders whose names are searchable with an item (`items.path_parts`).
pub const PATH_SEGMENTS: usize = 3;

/// Outcome of [`sync_files`].
#[derive(Debug, Clone, Default)]
pub struct FilesReport {
    pub scan_id: i64,
    pub scan: ScanReport,
    pub written: UpsertStats,
    /// Items removed because this pass did not see them.
    pub removed: u64,
    /// Unseen items kept because they lie under a directory this pass could not list.
    pub kept_unverified: u64,
}

/// Owned form of one entry until its batch is written.
struct Owned {
    kind: ItemKind,
    path: String,
    raw: Option<Vec<u8>>,
    name: String,
    key: String,
    name_parts: String,
    path_parts: String,
    extension: Option<String>,
    volume: Option<String>,
    file: Option<String>,
    attributes: i64,
    size: Option<i64>,
    modified: Option<i64>,
    created: Option<i64>,
    error: Option<&'static str>,
}

impl Owned {
    fn from_scan(e: &ScanEntry) -> Self {
        let encoded = encode(&e.path);
        let name = e.name();
        let kind = match e.kind {
            EntryKind::Dir => ItemKind::Folder,
            // A link to a directory (junction) is navigated like a folder.
            EntryKind::Symlink if e.path.is_dir() => ItemKind::Folder,
            _ => ItemKind::File,
        };
        let extension = (kind == ItemKind::File)
            .then(|| {
                Path::new(&name)
                    .extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
            })
            .flatten();
        let attributes = i64::from(e.flags.hidden)
            | (i64::from(e.flags.system) << 1)
            | (i64::from(e.flags.cloud_placeholder) << 2);
        Self {
            kind,
            key: fold(&name),
            name_parts: name_parts(&name),
            path_parts: path_parts(&encoded.text, PATH_SEGMENTS),
            name,
            path: encoded.text,
            raw: encoded.raw,
            extension,
            volume: e.identity.map(|i| i.volume_key()),
            file: e.identity.map(|i| i.file_key()),
            attributes,
            size: e.size.and_then(|s| i64::try_from(s).ok()),
            modified: e.modified_ms,
            created: e.created_ms,
            // Metadata unreadable: no timestamps at all (ADR-018 keeps the entry anyway).
            error: (e.modified_ms.is_none() && e.created_ms.is_none())
                .then_some("metadata unavailable"),
        }
    }

    fn entry(&self) -> CatalogEntry<'_> {
        CatalogEntry {
            kind: self.kind,
            source: Source::Files,
            path: &self.path,
            raw_path: self.raw.as_deref(),
            name: &self.name,
            name_key: &self.key,
            name_parts: &self.name_parts,
            path_parts: &self.path_parts,
            extension: self.extension.as_deref(),
            volume_id: self.volume.as_deref(),
            file_id: self.file.as_deref(),
            launch_target: None,
            attributes: self.attributes,
            size_bytes: self.size,
            modified_at: self.modified,
            created_at: self.created,
            error: self.error,
        }
    }
}

fn add(total: &mut UpsertStats, s: UpsertStats) {
    total.inserted += s.inserted;
    total.updated += s.updated;
    total.moved += s.moved;
}

fn flush(
    store: &mut Store,
    scan_id: i64,
    batch: &mut Vec<Owned>,
    total: &mut UpsertStats,
) -> Result<(), StorageError> {
    if batch.is_empty() {
        return Ok(());
    }
    let entries: Vec<CatalogEntry<'_>> = batch.iter().map(Owned::entry).collect();
    let stats = store.upsert_entries(scan_id, &entries)?;
    add(total, stats);
    batch.clear();
    Ok(())
}

/// Whether `path` is `dir` or inside it (case-insensitive on Windows, like the file system).
#[must_use]
pub fn is_within(path: &str, dir: &str) -> bool {
    let sep = std::path::MAIN_SEPARATOR;
    let (p, d) = if cfg!(windows) {
        (path.to_lowercase(), dir.to_lowercase())
    } else {
        (path.to_owned(), dir.to_owned())
    };
    let d = d.trim_end_matches(sep);
    p == d || p.strip_prefix(d).is_some_and(|rest| rest.starts_with(sep))
}

/// Inventories `opts.roots` into `store`. Call with the **complete** set of indexed roots:
/// items of roots no longer listed are removed as unseen.
///
/// # Errors
/// Storage failures. Scan problems are not errors: they are in `FilesReport::scan`.
pub fn sync_files(
    store: &mut Store,
    opts: &ScanOptions,
    cancel: Option<&CancellationToken>,
) -> Result<FilesReport, StorageError> {
    let scan_id = store.begin_scan(Source::Files)?;
    let mut total = UpsertStats::default();
    let mut batch: Vec<Owned> = Vec::with_capacity(BATCH);
    let mut failure: Option<StorageError> = None;

    let report = scan(
        opts,
        |e| {
            if failure.is_some() {
                return;
            }
            batch.push(Owned::from_scan(&e));
            if batch.len() >= BATCH
                && let Err(err) = flush(store, scan_id, &mut batch, &mut total)
            {
                failure = Some(err);
            }
        },
        cancel,
    );
    if let Some(err) = failure {
        return Err(err);
    }
    flush(store, scan_id, &mut batch, &mut total)?;

    // Removal: only after a pass that ran to the end, and never under unlistable dirs.
    let mut removed = 0;
    let mut kept = 0;
    if !report.cancelled {
        // Directories that failed to list, and roots that did not open at all (an unplugged
        // drive): what was there before stays.
        let unverified: Vec<String> = report
            .blocking_issues()
            .map(|i| encode(&i.path).text)
            .collect();
        let mut doomed = Vec::new();
        for (id, path) in store.unseen_items(scan_id, Source::Files)? {
            if unverified.iter().any(|dir| is_within(&path, dir)) {
                kept += 1;
            } else {
                doomed.push(id);
            }
        }
        removed = store.delete_items(&doomed)?;
    }
    let seen = report.emitted();
    let issues = report.issues.len() as u64;
    store.finish_scan(scan_id, report.is_complete(), seen, removed, issues)?;
    Ok(FilesReport {
        scan_id,
        scan: report,
        written: total,
        removed,
        kept_unverified: kept,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use lumen_indexer::Exclusions;
    use lumen_storage::SearchBudget;

    use super::*;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("lumen-catalog-files-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(p.join("root")).unwrap();
            Self(p)
        }

        fn root(&self) -> PathBuf {
            self.0.join("root")
        }

        fn store(&self) -> Store {
            Store::open_writer(&self.0.join("catalog.db")).unwrap()
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn opts(root: &Path) -> ScanOptions {
        ScanOptions {
            roots: vec![root.to_path_buf()],
            exclusions: Exclusions::default(),
            identity: true,
        }
    }

    fn names(store: &Store, key: &str) -> Vec<String> {
        store
            .search_names(key, 50, &SearchBudget::unbounded())
            .unwrap()
            .into_iter()
            .map(|h| h.item.name)
            .collect()
    }

    #[test]
    fn every_entry_becomes_an_item_and_resync_is_idempotent() {
        let t = Tmp::new("all");
        let r = t.root();
        fs::create_dir_all(r.join("Proyectos/lumen")).unwrap();
        fs::write(r.join("Proyectos/lumen/Reunión.md"), b"x").unwrap();
        fs::write(r.join("notas.txt"), b"x").unwrap();
        let mut store = t.store();

        let first = sync_files(&mut store, &opts(&r), None).unwrap();
        assert!(first.scan.is_complete());
        assert_eq!(first.written.inserted, first.scan.emitted());
        assert_eq!(
            store.count_items(Source::Files).unwrap(),
            first.scan.emitted()
        );
        assert_eq!(names(&store, "reunion"), ["Reunión.md"]);
        let folder = store
            .search_names("proyectos", 5, &SearchBudget::unbounded())
            .unwrap();
        assert_eq!(folder[0].item.kind, ItemKind::Folder);

        let second = sync_files(&mut store, &opts(&r), None).unwrap();
        assert_eq!(second.written.inserted, 0);
        assert_eq!(second.written.updated, first.scan.emitted());
        assert_eq!(second.removed, 0);
    }

    #[test]
    fn deleted_files_are_removed_and_renames_keep_their_item() {
        let t = Tmp::new("changes");
        let r = t.root();
        fs::write(r.join("keep.txt"), b"1").unwrap();
        fs::write(r.join("gone.txt"), b"2").unwrap();
        fs::write(r.join("old-name.txt"), b"3").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        let old_id = store
            .item_id_by_path(&encode(&r.join("old-name.txt")).text)
            .unwrap()
            .unwrap();

        fs::remove_file(r.join("gone.txt")).unwrap();
        fs::rename(r.join("old-name.txt"), r.join("new-name.txt")).unwrap();
        let rep = sync_files(&mut store, &opts(&r), None).unwrap();
        assert_eq!(rep.removed, 1, "gone.txt");
        assert_eq!(rep.written.moved, 1, "rename detected by identity");
        assert_eq!(
            store
                .item_id_by_path(&encode(&r.join("new-name.txt")).text)
                .unwrap(),
            Some(old_id)
        );
        assert!(names(&store, "gone").is_empty());
    }

    #[test]
    fn cancelled_pass_removes_nothing() {
        let t = Tmp::new("cancel");
        let r = t.root();
        fs::create_dir_all(r.join("a")).unwrap();
        fs::write(r.join("a/f.txt"), b"x").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        let before = store.count_items(Source::Files).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let rep = sync_files(&mut store, &opts(&r), Some(&token)).unwrap();
        assert!(rep.scan.cancelled);
        assert_eq!(rep.removed, 0);
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
    }

    #[test]
    fn missing_root_keeps_its_items() {
        let t = Tmp::new("missing");
        let r = t.root();
        fs::write(r.join("f.txt"), b"x").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        let before = store.count_items(Source::Files).unwrap();
        // The drive is unplugged: the root no longer opens.
        fs::rename(&r, t.0.join("elsewhere")).unwrap();
        let rep = sync_files(&mut store, &opts(&r), None).unwrap();
        assert!(!rep.scan.is_complete());
        assert_eq!(rep.removed, 0);
        assert_eq!(rep.kept_unverified, before);
    }

    #[cfg(unix)]
    #[test]
    fn unlistable_directory_keeps_its_items() {
        use std::os::unix::fs::PermissionsExt;
        let t = Tmp::new("locked");
        let r = t.root();
        fs::create_dir_all(r.join("locked")).unwrap();
        fs::write(r.join("locked/inside.txt"), b"x").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        fs::set_permissions(r.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(r.join("locked")).is_ok(); // root ignores modes
        let rep = sync_files(&mut store, &opts(&r), None).unwrap();
        fs::set_permissions(r.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(rep.removed, 0);
        if !readable_anyway {
            assert_eq!(rep.kept_unverified, 1, "inside.txt");
        }
        assert_eq!(names(&store, "inside"), ["inside.txt"]);
    }

    #[test]
    fn within_is_component_aware() {
        let s = std::path::MAIN_SEPARATOR;
        let dir = format!("{s}r{s}docs");
        assert!(is_within(&format!("{s}r{s}docs{s}a.txt"), &dir));
        assert!(is_within(&dir, &dir));
        assert!(!is_within(&format!("{s}r{s}docs2{s}a.txt"), &dir));
    }
}
