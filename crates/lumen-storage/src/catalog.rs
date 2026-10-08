//! The item catalog (T101): inventory passes write every entry, apps join the same table,
//! and name lookups serve the instant provider.
//!
//! Coverage (ADR-018): an inventory pass upserts every emitted entry — including ones whose
//! metadata failed (`status = 'error'`) — and only afterwards removes items it did not see,
//! and only where the pass could actually look (the caller passes the directories that
//! failed to list; nothing under them is removed).

use rusqlite::{OptionalExtension, params};

use crate::{ItemKind, Result, SearchBudget, Store};

/// Where an item comes from (`items.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Inventory of an indexed root.
    Files,
    /// Application catalog (Start menu / AppsFolder).
    Apps,
}

impl Source {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Apps => "apps",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "files" => Some(Self::Files),
            "apps" => Some(Self::Apps),
            _ => None,
        }
    }
}

/// One entry of an inventory pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry<'a> {
    pub kind: ItemKind,
    pub source: Source,
    /// Lookup/display form (see `items.canonical_path`).
    pub path: &'a str,
    /// Exact OS path when `path` is not lossless.
    pub raw_path: Option<&'a [u8]>,
    pub name: &'a str,
    /// Normalized search key of `name` (case-folded, diacritics removed).
    pub name_key: &'a str,
    /// Token text of the name and of its nearest parent folders (`names_fts`).
    pub name_parts: &'a str,
    pub path_parts: &'a str,
    pub extension: Option<&'a str>,
    pub volume_id: Option<&'a str>,
    pub file_id: Option<&'a str>,
    pub launch_target: Option<&'a str>,
    pub attributes: i64,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<i64>,
    pub created_at: Option<i64>,
    /// Set when the entry was emitted without full metadata (`status = 'error'`).
    pub error: Option<&'a str>,
}

/// What an upsert batch did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpsertStats {
    pub inserted: u64,
    /// Same path, metadata refreshed.
    pub updated: u64,
    /// Not at this path yet, but its identity, size and mtime match an item this pass has
    /// not seen: a rename/move — the item keeps its id (and later its chunks/vectors).
    pub moved: u64,
}

/// A stored item as the catalog provider and action executors need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogItem {
    pub id: i64,
    pub kind: ItemKind,
    pub source: Source,
    pub path: String,
    pub raw_path: Option<Vec<u8>>,
    pub name: String,
    pub name_key: String,
    pub name_parts: String,
    pub path_parts: String,
    pub extension: Option<String>,
    pub launch_target: Option<String>,
    pub attributes: i64,
    pub modified_at: Option<i64>,
}

/// A name lookup hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameHit {
    pub item: CatalogItem,
    /// `name_key` equals the query key.
    pub exact: bool,
}

/// Upper bound for `name_key` prefix ranges (largest scalar value).
const KEY_MAX: char = '\u{10FFFF}';

const ITEM_COLUMNS: &str = "items.id, items.kind, items.source, items.canonical_path, \
     items.raw_path, items.display_name, items.name_key, items.name_parts, items.path_parts, \
     items.extension, items.launch_target, items.attributes, items.modified_at";

fn item_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CatalogItem> {
    let kind: String = r.get(1)?;
    let source: String = r.get(2)?;
    Ok(CatalogItem {
        id: r.get(0)?,
        kind: ItemKind::parse(&kind).unwrap_or(ItemKind::File),
        source: Source::parse(&source).unwrap_or(Source::Files),
        path: r.get(3)?,
        raw_path: r.get(4)?,
        name: r.get(5)?,
        name_key: r.get(6)?,
        name_parts: r.get(7)?,
        path_parts: r.get(8)?,
        extension: r.get(9)?,
        launch_target: r.get(10)?,
        attributes: r.get(11)?,
        modified_at: r.get(12)?,
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl Store {
    /// Starts an inventory pass and returns its id (`items.seen_scan`).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn begin_scan(&self, source: Source) -> Result<i64> {
        self.conn
            .prepare_cached("INSERT INTO scans (source, started_at) VALUES (?1, ?2)")?
            .execute(params![source.as_str(), now_ms()])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Records the outcome of a pass.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn finish_scan(
        &self,
        scan: i64,
        complete: bool,
        seen: u64,
        removed: u64,
        issues: u64,
    ) -> Result<()> {
        let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        self.conn
            .prepare_cached(
                "UPDATE scans SET finished_at = ?2, complete = ?3, seen = ?4, removed = ?5,
                 issues = ?6 WHERE id = ?1",
            )?
            .execute(params![
                scan,
                now_ms(),
                complete,
                n(seen),
                n(removed),
                n(issues)
            ])?;
        Ok(())
    }

    /// Writes a batch of entries seen by pass `scan`, in one transaction.
    ///
    /// Per entry: same exact path → refresh; otherwise an unseen item with the same
    /// identity, size and mtime → move it here; otherwise insert.
    ///
    /// # Errors
    /// SQLite failure (the whole batch rolls back).
    pub fn upsert_entries(
        &mut self,
        scan: i64,
        entries: &[CatalogEntry<'_>],
    ) -> Result<UpsertStats> {
        let tx = self.conn.transaction()?;
        let mut stats = UpsertStats::default();
        {
            let mut by_path =
                tx.prepare_cached("SELECT id FROM items WHERE canonical_path = ?1")?;
            let mut by_identity = tx.prepare_cached(
                // Without statistics the planner prefers items_modified, and many files share
                // an mtime (package installs): force the identity index.
                "SELECT id FROM items INDEXED BY items_identity
                 WHERE volume_id = ?1 AND file_id = ?2 AND source = ?3
                   AND (seen_scan IS NULL OR seen_scan < ?4)
                   AND size_bytes IS ?5 AND modified_at IS ?6
                 LIMIT 1",
            )?;
            let mut update = tx.prepare_cached(
                "UPDATE items SET kind = ?2, source = ?3, volume_id = ?4, file_id = ?5,
                    canonical_path = ?6, raw_path = ?7, display_name = ?8, name_key = ?9,
                    extension = ?10, launch_target = ?11, attributes = ?12, size_bytes = ?13,
                    modified_at = ?14, created_at = ?15, seen_scan = ?16,
                    status = CASE WHEN ?17 IS NULL THEN
                                 CASE status WHEN 'error' THEN 'pending' ELSE status END
                             ELSE 'error' END,
                    error_code = ?17, name_parts = ?18, path_parts = ?19
                 WHERE id = ?1",
            )?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO items (kind, source, volume_id, file_id, canonical_path, raw_path,
                    display_name, name_key, extension, launch_target, attributes, size_bytes,
                    modified_at, created_at, seen_scan, status, error_code, name_parts, path_parts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    CASE WHEN ?16 IS NULL THEN 'pending' ELSE 'error' END, ?16, ?17, ?18)",
            )?;
            for e in entries {
                let existing: Option<i64> = by_path.query_row([e.path], |r| r.get(0)).optional()?;
                let target = match (existing, e.volume_id, e.file_id) {
                    (Some(id), ..) => Some((id, false)),
                    (None, Some(volume), Some(file)) => by_identity
                        .query_row(
                            params![
                                volume,
                                file,
                                e.source.as_str(),
                                scan,
                                e.size_bytes,
                                e.modified_at
                            ],
                            |r| r.get::<_, i64>(0),
                        )
                        .optional()?
                        .map(|id| (id, true)),
                    _ => None,
                };
                match target {
                    Some((id, moved)) => {
                        update.execute(params![
                            id,
                            e.kind.as_str(),
                            e.source.as_str(),
                            e.volume_id,
                            e.file_id,
                            e.path,
                            e.raw_path,
                            e.name,
                            e.name_key,
                            e.extension,
                            e.launch_target,
                            e.attributes,
                            e.size_bytes,
                            e.modified_at,
                            e.created_at,
                            scan,
                            e.error,
                            e.name_parts,
                            e.path_parts
                        ])?;
                        if moved {
                            stats.moved += 1;
                        } else {
                            stats.updated += 1;
                        }
                    }
                    None => {
                        insert.execute(params![
                            e.kind.as_str(),
                            e.source.as_str(),
                            e.volume_id,
                            e.file_id,
                            e.path,
                            e.raw_path,
                            e.name,
                            e.name_key,
                            e.extension,
                            e.launch_target,
                            e.attributes,
                            e.size_bytes,
                            e.modified_at,
                            e.created_at,
                            scan,
                            e.error,
                            e.name_parts,
                            e.path_parts
                        ])?;
                        stats.inserted += 1;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    /// Items of `source` that pass `scan` did not see: `(id, path)`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn unseen_items(&self, scan: i64, source: Source) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, canonical_path FROM items
             WHERE source = ?1 AND (seen_scan IS NULL OR seen_scan < ?2)",
        )?;
        let rows = stmt.query_map(params![source.as_str(), scan], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Deletes items (and by cascade their chunks/usage) in one transaction.
    ///
    /// # Errors
    /// SQLite failure (nothing is deleted).
    pub fn delete_items(&mut self, ids: &[i64]) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let mut removed = 0;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM items WHERE id = ?1")?;
            for id in ids {
                removed += stmt.execute([id])? as u64;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Items whose `name_key` equals or starts with `key`: exact matches first, then up to
    /// `limit` prefix matches in key order. Ranking beyond that is the provider's job.
    ///
    /// # Errors
    /// [`crate::StorageError::Interrupted`] when `budget` ran out; SQLite failure.
    pub fn search_names(
        &self,
        key: &str,
        limit: usize,
        budget: &SearchBudget,
    ) -> Result<Vec<NameHit>> {
        if key.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.bounded(budget, |store| {
            let mut hits = Vec::new();
            let mut exact = store.conn.prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM items WHERE name_key = ?1 LIMIT ?2"
            ))?;
            for item in exact.query_map(params![key, limit], item_from_row)? {
                hits.push(NameHit {
                    item: item?,
                    exact: true,
                });
            }
            let upper = format!("{key}{KEY_MAX}");
            let mut prefix = store.conn.prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM items WHERE name_key > ?1 AND name_key < ?2
                 ORDER BY name_key LIMIT ?3"
            ))?;
            for item in prefix.query_map(params![key, upper, limit], item_from_row)? {
                hits.push(NameHit {
                    item: item?,
                    exact: false,
                });
            }
            Ok(hits)
        })
    }

    /// Items whose name or parent-folder tokens match the FTS5 expression `matcher`
    /// (built by the caller from quoted prefix terms), best bm25 first — names weigh four
    /// times more than folders — capped at `limit`.
    ///
    /// # Errors
    /// [`crate::StorageError::Interrupted`] when `budget` ran out; SQLite failure (including
    /// an invalid expression).
    pub fn search_name_tokens(
        &self,
        matcher: &str,
        limit: usize,
        budget: &SearchBudget,
    ) -> Result<Vec<CatalogItem>> {
        if matcher.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.bounded(budget, |store| {
            let mut stmt = store.conn.prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM names_fts JOIN items ON items.id = names_fts.rowid
                 WHERE names_fts MATCH ?1 ORDER BY bm25(names_fts, 1.0, 0.25) LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![matcher, limit], item_from_row)?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Up to `limit` items with `lo <= name_key < hi`, in key order (fuzzy-match candidates).
    ///
    /// # Errors
    /// [`crate::StorageError::Interrupted`] when `budget` ran out; SQLite failure.
    pub fn name_key_range(
        &self,
        lo: &str,
        hi: &str,
        limit: usize,
        budget: &SearchBudget,
    ) -> Result<Vec<CatalogItem>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.bounded(budget, |store| {
            let mut stmt = store.conn.prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM items WHERE name_key >= ?1 AND name_key < ?2
                 ORDER BY name_key LIMIT ?3"
            ))?;
            let rows = stmt.query_map(params![lo, hi, limit], item_from_row)?;
            Ok(rows.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// One item by id.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn catalog_item(&self, id: i64) -> Result<Option<CatalogItem>> {
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM items WHERE items.id = ?1"
            ))?
            .query_row([id], item_from_row)
            .optional()?)
    }

    /// Number of items of `source`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn count_items(&self, source: Source) -> Result<u64> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT count(*) FROM items WHERE source = ?1")?
            .query_row([source.as_str()], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct TempDb(PathBuf);

    impl TempDb {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("lumen-catalog-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn store(&self) -> Store {
            Store::open_writer(&self.0.join("t.db")).unwrap()
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry<'a>(path: &'a str, name: &'a str, key: &'a str) -> CatalogEntry<'a> {
        CatalogEntry {
            kind: ItemKind::File,
            source: Source::Files,
            path,
            raw_path: None,
            name,
            name_key: key,
            name_parts: key,
            path_parts: "",
            extension: None,
            volume_id: Some("v1"),
            file_id: None,
            launch_target: None,
            attributes: 0,
            size_bytes: Some(10),
            modified_at: Some(1000),
            created_at: None,
            error: None,
        }
    }

    #[test]
    fn upsert_inserts_refreshes_and_tracks_moves() {
        let db = TempDb::new("upsert");
        let mut store = db.store();
        let s1 = store.begin_scan(Source::Files).unwrap();
        let a = CatalogEntry {
            file_id: Some("f1"),
            ..entry("/r/a.txt", "a.txt", "a.txt")
        };
        let b = entry("/r/b.txt", "b.txt", "b.txt");
        let st = store.upsert_entries(s1, &[a.clone(), b.clone()]).unwrap();
        assert_eq!(st.inserted, 2);
        let a_id = store.item_id_by_path("/r/a.txt").unwrap().unwrap();

        // Pass 2: a.txt was moved (same identity/size/mtime), b.txt unchanged.
        let s2 = store.begin_scan(Source::Files).unwrap();
        let moved = CatalogEntry {
            path: "/r/sub/a.txt",
            ..a.clone()
        };
        let st = store.upsert_entries(s2, &[moved, b.clone()]).unwrap();
        assert_eq!((st.moved, st.updated, st.inserted), (1, 1, 0));
        assert_eq!(store.item_id_by_path("/r/sub/a.txt").unwrap(), Some(a_id));
        assert!(store.unseen_items(s2, Source::Files).unwrap().is_empty());

        // A hard link (same identity, seen in this pass already) is a separate item.
        let link = CatalogEntry {
            path: "/r/link.txt",
            ..a.clone()
        };
        let st = store.upsert_entries(s2, &[link]).unwrap();
        assert_eq!(st.inserted, 1);

        // Identity match with a different mtime is not a move.
        let s3 = store.begin_scan(Source::Files).unwrap();
        let edited = CatalogEntry {
            path: "/r/elsewhere.txt",
            modified_at: Some(2000),
            ..a
        };
        assert_eq!(store.upsert_entries(s3, &[edited]).unwrap().inserted, 1);
    }

    #[test]
    fn move_detection_uses_the_identity_index() {
        // Regression (T101): without the hint SQLite picked items_modified and inventory
        // inserts became O(n) each (2.2k/s instead of 40k/s at 245k entries).
        let db = TempDb::new("plan");
        let store = db.store();
        let plan = store
            .query_plan(
                "SELECT id FROM items INDEXED BY items_identity WHERE volume_id = 'v' AND \
                 file_id = 'f' AND source = 'files' AND (seen_scan IS NULL OR seen_scan < 2) \
                 AND size_bytes IS 1 AND modified_at IS 2 LIMIT 1",
            )
            .unwrap()
            .join(" | ");
        assert!(plan.contains("items_identity"), "{plan}");
    }

    #[test]
    fn unseen_items_and_delete() {
        let db = TempDb::new("stale");
        let mut store = db.store();
        let s1 = store.begin_scan(Source::Files).unwrap();
        store
            .upsert_entries(s1, &[entry("/r/a", "a", "a"), entry("/r/b", "b", "b")])
            .unwrap();
        let s2 = store.begin_scan(Source::Files).unwrap();
        store
            .upsert_entries(s2, &[entry("/r/a", "a", "a")])
            .unwrap();
        let unseen = store.unseen_items(s2, Source::Files).unwrap();
        assert_eq!(unseen.len(), 1);
        assert_eq!(unseen[0].1, "/r/b");
        assert_eq!(store.delete_items(&[unseen[0].0]).unwrap(), 1);
        assert_eq!(store.count_items(Source::Files).unwrap(), 1);
        store.finish_scan(s2, true, 1, 1, 0).unwrap();
    }

    #[test]
    fn errors_are_stored_and_cleared() {
        let db = TempDb::new("errors");
        let mut store = db.store();
        let s1 = store.begin_scan(Source::Files).unwrap();
        let bad = CatalogEntry {
            error: Some("metadata: access denied"),
            ..entry("/r/x", "x", "x")
        };
        store.upsert_entries(s1, &[bad]).unwrap();
        let status: (String, Option<String>) = store
            .connection()
            .query_row("SELECT status, error_code FROM items", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status.0, "error");
        // Still findable by name (coverage).
        assert_eq!(
            store
                .search_names("x", 5, &SearchBudget::unbounded())
                .unwrap()
                .len(),
            1
        );
        let s2 = store.begin_scan(Source::Files).unwrap();
        store
            .upsert_entries(s2, &[entry("/r/x", "x", "x")])
            .unwrap();
        let status: String = store
            .connection()
            .query_row("SELECT status FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn name_search_exact_then_prefix_and_uses_the_index() {
        let db = TempDb::new("names");
        let mut store = db.store();
        let s = store.begin_scan(Source::Apps).unwrap();
        let app = |path: &'static str, name: &'static str, key: &'static str| CatalogEntry {
            kind: ItemKind::Application,
            source: Source::Apps,
            launch_target: Some(path),
            ..entry(path, name, key)
        };
        store
            .upsert_entries(
                s,
                &[
                    app("apps:spotify", "Spotify", "spotify"),
                    app("apps:spotify-tools", "Spotify Tools", "spotify tools"),
                    app("apps:notepad", "Notepad", "notepad"),
                ],
            )
            .unwrap();
        let hits = store
            .search_names("spotify", 10, &SearchBudget::unbounded())
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(hits[0].exact && hits[0].item.name == "Spotify");
        assert!(!hits[1].exact && hits[1].item.source == Source::Apps);
        assert_eq!(
            store
                .search_names("spo", 1, &SearchBudget::unbounded())
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .search_names("", 5, &SearchBudget::unbounded())
                .unwrap()
                .is_empty()
        );
        let plan = store
            .query_plan("SELECT id FROM items WHERE name_key > 'a' AND name_key < 'b' ORDER BY name_key LIMIT 5")
            .unwrap()
            .join(" | ");
        assert!(
            plan.contains("items_name_key") && !plan.contains("TEMP B-TREE"),
            "{plan}"
        );
        let item = store.catalog_item(hits[0].item.id).unwrap().unwrap();
        assert_eq!(item.launch_target.as_deref(), Some("apps:spotify"));
    }

    #[test]
    fn token_search_matches_name_and_folder_tokens_and_stays_in_sync() {
        let db = TempDb::new("tokens");
        let mut store = db.store();
        let s1 = store.begin_scan(Source::Files).unwrap();
        let code = CatalogEntry {
            name_parts: "visual studio code vsc",
            ..entry("/apps/code", "Visual Studio Code", "visual studio code")
        };
        let notes = CatalogEntry {
            name_parts: "notas md",
            path_parts: "proyectos lumen",
            ..entry("/p/lumen/notas.md", "notas.md", "notas.md")
        };
        store.upsert_entries(s1, &[code, notes.clone()]).unwrap();
        let find = |store: &Store, q: &str| -> Vec<String> {
            store
                .search_name_tokens(q, 10, &SearchBudget::unbounded())
                .unwrap()
                .into_iter()
                .map(|i| i.name)
                .collect()
        };
        assert_eq!(find(&store, "\"stu\"*"), ["Visual Studio Code"]);
        assert_eq!(find(&store, "\"vsc\"*"), ["Visual Studio Code"]);
        assert_eq!(
            find(&store, "\"lumen\"* \"not\"*"),
            ["notas.md"],
            "folder + name"
        );

        // Renamed: the index follows; a metadata-only refresh leaves it alone.
        let s2 = store.begin_scan(Source::Files).unwrap();
        let renamed = CatalogEntry {
            name_parts: "apuntes md",
            ..notes
        };
        store.upsert_entries(s2, &[renamed]).unwrap();
        assert!(find(&store, "\"notas\"*").is_empty());
        assert_eq!(find(&store, "\"apun\"*"), ["notas.md"]);
        let id = store.item_id_by_path("/p/lumen/notas.md").unwrap().unwrap();
        store.delete_items(&[id]).unwrap();
        assert!(find(&store, "\"apun\"*").is_empty());
        let range = store
            .name_key_range("v", "w", 5, &SearchBudget::unbounded())
            .unwrap();
        assert_eq!(range.len(), 1);
    }

    #[test]
    fn name_search_respects_the_budget() {
        let db = TempDb::new("budget");
        let store = db.store();
        let cancel = lumen_core::CancellationToken::new();
        cancel.cancel();
        let budget = SearchBudget::unbounded().with_cancel(cancel);
        // An empty table finishes before the first progress check; cancelled tokens still
        // must not break a fast query.
        assert!(store.search_names("a", 5, &budget).is_ok());
    }
}
