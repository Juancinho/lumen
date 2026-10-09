//! Inventory pass → `items` (T101, ADR-018 coverage guarantee).
//!
//! Every entry the scan emits is written, including ones whose metadata could not be read
//! (stored with `status = 'error'`, still findable by name). After the walk, items the pass
//! did not see are removed — except under directories that failed to list, and never after
//! a cancelled pass, so a transient failure can never make files disappear from search.

use std::collections::HashSet;
use std::path::Path;

use lumen_core::CancellationToken;
use lumen_indexer::{EntryKind, ScanEntry, ScanOptions, ScanReport, scan};
use lumen_storage::{CatalogEntry, ItemKind, Source, StorageError, Store, UpsertStats};

use crate::path::{decode, encode};
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
    sync_files_with_progress(store, opts, cancel, &mut |_| {})
}

/// [`sync_files`], calling `progress` with the running totals after every written batch
/// (so a caller can tell the UI that new entries are searchable during a long pass).
///
/// # Errors
/// Storage failures.
pub fn sync_files_with_progress(
    store: &mut Store,
    opts: &ScanOptions,
    cancel: Option<&CancellationToken>,
    progress: &mut dyn FnMut(&UpsertStats),
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
            if batch.len() >= BATCH {
                match flush(store, scan_id, &mut batch, &mut total) {
                    Ok(()) => progress(&total),
                    Err(err) => failure = Some(err),
                }
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

/// Applies a bounded batch of filesystem hints without removing unrelated roots/items.
/// Writes precede removals so file/folder moves retain item, chunk and vector identities.
/// Offline roots, failed listings and cancellation never justify deletion (ADR-037).
///
/// # Errors
/// Storage failures. Native hints and filesystem failures remain in the report.
pub fn sync_changes(
    store: &mut Store,
    opts: &ScanOptions,
    changes: &[lumen_indexer::watch::Change],
    cancel: Option<&CancellationToken>,
) -> Result<FilesReport, StorageError> {
    sync_changes_with_content_scope(store, opts, changes, cancel, &|_| true)
}

/// [`sync_changes`] with the caller's content-consent scope. Ambiguous Windows rename +
/// modify hints may read supported text up to the extractor limit only inside this scope.
/// # Errors
/// Storage failure.
pub fn sync_changes_with_content_scope(
    store: &mut Store,
    opts: &ScanOptions,
    changes: &[lumen_indexer::watch::Change],
    cancel: Option<&CancellationToken>,
    content_scope: &dyn Fn(&str) -> bool,
) -> Result<FilesReport, StorageError> {
    use lumen_indexer::watch::{Change, is_marker};
    let within_roots = |path: &Path| {
        opts.roots
            .iter()
            .any(|r| is_within(&encode(path).text, &encode(r).text))
    };
    let dirty: HashSet<String> = changes
        .iter()
        .filter(|c| c.content && within_roots(&c.path))
        .map(|c| encode(&c.path).text)
        .collect();
    let mut scopes = Vec::<Change>::new();
    let contains_scope = |parent: &Change, child: &Change| {
        let p = encode(&parent.path).text;
        let c = encode(&child.path).text;
        parent.recursive
            && is_within(&c, &p)
            && !(cfg!(windows) && p != c && p.to_lowercase() == c.to_lowercase())
    };
    for change in changes.iter().filter(|c| within_roots(&c.path)) {
        let mut scope = change.clone();
        if is_marker(&scope.path)
            && let Some(parent) = scope.path.parent().filter(|p| within_roots(p))
        {
            if scope.path.file_name().is_some_and(|n| n == ".git") {
                store.invalidate_code_under(&encode(parent).text)?;
            }
            scope.path = parent.to_path_buf();
            scope.recursive = true;
            scope.content = false;
        }
        if scopes.iter().any(|c| contains_scope(c, &scope)) {
            continue;
        }
        if scope.recursive {
            scopes.retain(|c| !contains_scope(&scope, c));
        }
        scopes.push(scope);
    }
    let scan_id = store.begin_scan(Source::Files)?;
    let mut total = UpsertStats::default();
    let mut batch = Vec::<Owned>::with_capacity(BATCH);
    let mut failure = None;
    let flush_changed = |store: &mut Store,
                         batch: &mut Vec<Owned>,
                         total: &mut UpsertStats|
     -> Result<(), StorageError> {
        if batch.is_empty() {
            return Ok(());
        }
        let entries: Vec<_> = batch.iter().map(Owned::entry).collect();
        let mut absent = HashSet::new();
        let mut effective_dirty = dirty.clone();
        // Filesystem checks run before the short SQLite transaction. A surviving hard
        // link or an unavailable root is never a move candidate.
        for e in &entries {
            if e.volume_id.is_none() || store.exact_item_id_by_path(e.path)?.is_some() {
                continue;
            }
            for old in store.identity_candidates(e)? {
                let old_path = decode(&old.path, old.raw_path.as_deref());
                let online = opts
                    .roots
                    .iter()
                    .filter(|r| is_within(&old.path, &encode(r).text))
                    .any(|r| std::fs::metadata(r).is_ok());
                let case_rename = cfg!(windows)
                    && old.path != e.path
                    && old.path.to_lowercase() == e.path.to_lowercase()
                    && opts
                        .roots
                        .iter()
                        .any(|r| scan::spelling_exists(&old_path, r) == Some(false));
                if online
                    && (case_rename
                        || std::fs::symlink_metadata(&old_path)
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound))
                {
                    absent.insert(old.id);
                    if dirty.contains(e.path)
                        && e.kind == ItemKind::File
                        && e.attributes & 4 == 0
                        && content_scope(e.path)
                        && matching_move_content(store, old.id, e)?
                    {
                        effective_dirty.remove(e.path);
                    }
                }
            }
        }
        add(
            total,
            store.upsert_changed_entries(scan_id, &entries, &absent, &effective_dirty)?,
        );
        batch.clear();
        Ok(())
    };
    let scan = scan::scan_changed(
        opts,
        &scopes,
        |e| {
            if failure.is_some() {
                return;
            }
            batch.push(Owned::from_scan(&e));
            if batch.len() >= BATCH
                && let Err(err) = flush_changed(store, &mut batch, &mut total)
            {
                failure = Some(err);
            }
        },
        cancel,
    );
    if let Some(err) = failure {
        return Err(err);
    }
    flush_changed(store, &mut batch, &mut total)?;
    let unverified: Vec<_> = scan
        .blocking_issues()
        .map(|i| encode(&i.path).text)
        .collect();
    let mut removed = 0;
    let mut kept_unverified = 0;
    if !scan.cancelled {
        for scope in &scopes {
            if !scope.recursive && scope.path.is_dir() {
                continue;
            }
            let mut after = String::new();
            loop {
                if cancel.is_some_and(CancellationToken::is_cancelled) {
                    break;
                }
                let rows = store.unseen_under(scan_id, &encode(&scope.path).text, &after, BATCH)?;
                if rows.is_empty() {
                    break;
                }
                let mut doomed = Vec::new();
                for row in rows {
                    after = row.path.clone();
                    if unverified.iter().any(|p| is_within(&row.path, p)) {
                        kept_unverified += 1;
                    } else {
                        doomed.push(row.id);
                    }
                }
                removed += store.delete_items(&doomed)?;
            }
        }
    }
    store.finish_scan(
        scan_id,
        scan.is_complete(),
        scan.emitted(),
        removed,
        scan.issues.len() as u64,
    )?;
    Ok(FilesReport {
        scan_id,
        scan,
        written: total,
        removed,
        kept_unverified,
    })
}

fn matching_move_content(
    store: &Store,
    id: i64,
    entry: &CatalogEntry<'_>,
) -> Result<bool, StorageError> {
    use lumen_extract::{
        ChunkConfig, DEFAULT_MAX_BYTES, EXTRACTOR_VERSION, EstimateTokens, chunk, extract_file,
    };
    let path = decode(entry.path, entry.raw_path);
    let Ok(doc) = extract_file(&path, DEFAULT_MAX_BYTES) else {
        return Ok(false);
    };
    let chunks = chunk(&doc, &ChunkConfig::default(), &EstimateTokens);
    let expected: Vec<_> = chunks
        .iter()
        .map(|c| lumen_storage::NewChunk {
            item_id: id,
            ordinal: i64::from(c.ordinal),
            chunk_kind: c.kind.as_str(),
            text: c.text(&doc.text),
            symbol_name: c.symbol.as_deref(),
            page_number: None,
            start_offset: i64::try_from(c.start).ok(),
            end_offset: i64::try_from(c.end).ok(),
        })
        .collect();
    store.content_matches(id, &expected, EXTRACTOR_VERSION)
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

    #[test]
    fn incremental_missing_root_and_cancel_keep_items() {
        use lumen_indexer::watch::Change;
        let t = Tmp::new("incremental-offline");
        let r = t.root();
        fs::write(r.join("file.txt"), "preserved").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        let before = store.count_items(Source::Files).unwrap();
        fs::rename(&r, t.0.join("offline")).unwrap();
        let changes = [Change {
            path: r.clone(),
            recursive: true,
            content: false,
            renamed_from: false,
        }];
        let report = sync_changes(&mut store, &opts(&r), &changes, None).unwrap();
        assert!(!report.scan.is_complete());
        assert_eq!(report.removed, 0);
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
        fs::rename(t.0.join("offline"), &r).unwrap();
        fs::remove_file(r.join("file.txt")).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let report = sync_changes(&mut store, &opts(&r), &changes, Some(&token)).unwrap();
        assert!(report.scan.cancelled);
        assert_eq!(report.removed, 0);
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
    }

    #[test]
    fn markers_reconcile_excluded_siblings_and_ordinary_directory_writes_keep_children() {
        use lumen_indexer::watch::Change;
        let t = Tmp::new("incremental-marker");
        let r = t.root();
        fs::create_dir_all(r.join("target/nested")).unwrap();
        fs::write(r.join("target/nested/keep.txt"), "kept").unwrap();
        let mut options = opts(&r);
        options.exclusions.build_dirs_next_to_markers = true;
        let mut store = t.store();
        sync_files(&mut store, &options, None).unwrap();
        let before = store.count_items(Source::Files).unwrap();
        let changes = [Change {
            path: r.join("target"),
            recursive: false,
            content: true,
            renamed_from: false,
        }];
        let report = sync_changes(&mut store, &options, &changes, None).unwrap();
        assert_eq!(report.scan.emitted(), 1);
        assert_eq!(report.removed, 0);
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
        let marker = r.join("Cargo.toml");
        let changes = [Change {
            path: marker.clone(),
            recursive: true,
            content: false,
            renamed_from: false,
        }];
        fs::write(&marker, "[package]").unwrap();
        let report = sync_changes(&mut store, &options, &changes, None).unwrap();
        assert_eq!(report.removed, 3);
        assert_eq!(report.scan.excluded[0].rule, "build:target");
        fs::remove_file(&marker).unwrap();
        sync_changes(&mut store, &options, &changes, None).unwrap();
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
    }

    #[cfg(windows)]
    #[test]
    fn case_only_rename_preserves_identity_without_duplicate_paths() {
        use lumen_indexer::watch::Change;
        let t = Tmp::new("incremental-case");
        let r = t.root();
        let old = r.join("lower.txt");
        let new = r.join("LOWER.txt");
        let old_dir = r.join("lower-dir");
        let new_dir = r.join("LOWER-DIR");
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join("child.txt"), "same child").unwrap();
        fs::write(&old, "same").unwrap();
        let mut store = t.store();
        sync_files(&mut store, &opts(&r), None).unwrap();
        let id = store
            .item_id_by_path(old.to_str().unwrap())
            .unwrap()
            .unwrap();
        let before = store.count_items(Source::Files).unwrap();
        let child_id = store
            .exact_item_id_by_path(old_dir.join("child.txt").to_str().unwrap())
            .unwrap();
        fs::rename(&old, &new).unwrap();
        let changes = [
            Change {
                path: old,
                recursive: true,
                content: false,
                renamed_from: true,
            },
            Change {
                path: new.clone(),
                recursive: true,
                content: false,
                renamed_from: false,
            },
        ];
        let report = sync_changes(&mut store, &opts(&r), &changes, None).unwrap();
        assert_eq!(report.written.moved, 1, "{report:?}");
        assert_eq!(
            store.item_id_by_path(new.to_str().unwrap()).unwrap(),
            Some(id)
        );
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
        fs::rename(&old_dir, &new_dir).unwrap();
        let changes = [
            Change {
                path: old_dir,
                recursive: true,
                content: false,
                renamed_from: true,
            },
            Change {
                path: new_dir.clone(),
                recursive: true,
                content: false,
                renamed_from: false,
            },
        ];
        sync_changes(&mut store, &opts(&r), &changes, None).unwrap();
        assert_eq!(
            store
                .exact_item_id_by_path(new_dir.join("child.txt").to_str().unwrap())
                .unwrap(),
            child_id
        );
        assert_eq!(store.count_items(Source::Files).unwrap(), before);
    }
}
