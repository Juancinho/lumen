//! Lumen's canonical metadata store (ADR-003): SQLite in WAL mode with FTS5.
//!
//! Connection roles (docs/PERFORMANCE.md §8):
//! - one **writer** ([`Store::open_writer`]) owned by the indexer: runs migrations, batches
//!   writes in short transactions;
//! - any number of **readers** ([`Store::open_reader`]) for search: read-only, never blocked
//!   by the writer thanks to WAL snapshot isolation.
//!
//! The ANN index is derived data rebuilt from `chunk_vectors` (ADR-016/029); this store is
//! the truth.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod content;
mod fts;
pub mod generations;
pub mod migrations;
mod settings;
pub mod usage;

use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use lumen_core::CancellationToken;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

pub use catalog::{CatalogEntry, CatalogItem, NameHit, Source, UpsertStats};
pub use content::{
    ContentCandidate, ContentCounts, ContentOutcome, ContentWrite, GenerationSpec, PendingChunk,
    QueueCounts, VectorWrite,
};
pub use fts::{FtsQuery, MIN_PREFIX_CHARS};
pub use generations::{AnnFileRecord, GenerationInfo, GenerationState, SeqVector};
pub use migrations::{MIGRATIONS, Migration, latest_version};
pub use usage::{UsageSignal, UseKind};

/// Marks the start/end of a matched term in [`ChunkHit::snippet`] (Unicode private-use
/// characters, so they never collide with real text; the UI maps them to highlight spans).
pub const HIGHLIGHT_START: char = '\u{E000}';
pub const HIGHLIGHT_END: char = '\u{E001}';

/// How long a connection waits for a lock before failing (writers only contend with
/// checkpoints; readers never wait in WAL mode).
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
#[non_exhaustive]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    /// The database was written by a newer Lumen; refuse to touch it.
    SchemaTooNew {
        found: u32,
        supported: u32,
    },
    Migration {
        version: u32,
        name: &'static str,
        message: String,
    },
    /// WAL could not be enabled (e.g. network share); Lumen requires it.
    WalUnavailable(String),
    Corrupt(String),
    /// The query exceeded its [`SearchBudget`] or was cancelled (stale keystroke).
    Interrupted,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "database schema v{found} is newer than supported v{supported}"
            ),
            Self::Migration {
                version,
                name,
                message,
            } => write!(f, "migration {version} ({name}) failed: {message}"),
            Self::WalUnavailable(mode) => write!(f, "WAL journal unavailable (got `{mode}`)"),
            Self::Corrupt(what) => write!(f, "corrupt database: {what}"),
            Self::Interrupted => f.write_str("query interrupted (budget exceeded or cancelled)"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::OperationInterrupted) => Self::Interrupted,
            _ => Self::Sqlite(e),
        }
    }
}

/// Bounds an interactive query: a deadline and/or a cancellation token (superseded query).
/// Checked by SQLite's progress handler every few thousand VM steps, so a query that would
/// rank most of the corpus (very common or very short terms) stops instead of delaying the
/// next keystroke (ADR-009: lexical results must never stall the surface).
#[derive(Debug, Clone, Default)]
pub struct SearchBudget {
    pub deadline: Option<Instant>,
    pub cancel: Option<CancellationToken>,
}

impl SearchBudget {
    #[must_use]
    pub fn unbounded() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn within(limit: Duration) -> Self {
        Self {
            deadline: Some(Instant::now() + limit),
            cancel: None,
        }
    }

    #[must_use]
    pub fn with_cancel(mut self, token: CancellationToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Whether the cancellation token (if any) fired; a deadline alone never cancels.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    fn is_bounded(&self) -> bool {
        self.deadline.is_some() || self.cancel.is_some()
    }
}

/// SQLite VM instructions between progress-handler checks (~tens of µs).
const PROGRESS_STEPS: i32 = 4_000;

pub type Result<T> = std::result::Result<T, StorageError>;

/// Item kinds (`items.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Folder,
    Application,
}

impl ItemKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
            Self::Application => "application",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "file" => Some(Self::File),
            "folder" => Some(Self::Folder),
            "application" => Some(Self::Application),
            _ => None,
        }
    }
}

/// Insert payload for `items`.
#[derive(Debug, Clone)]
pub struct NewItem<'a> {
    pub kind: ItemKind,
    pub volume_id: Option<&'a str>,
    pub file_id: Option<&'a str>,
    pub canonical_path: &'a str,
    pub display_name: &'a str,
    pub extension: Option<&'a str>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<i64>,
}

impl<'a> NewItem<'a> {
    #[must_use]
    pub fn file(path: &'a str, name: &'a str) -> Self {
        Self {
            kind: ItemKind::File,
            volume_id: None,
            file_id: None,
            canonical_path: path,
            display_name: name,
            extension: name.rsplit_once('.').map(|(_, e)| e),
            size_bytes: None,
            modified_at: None,
        }
    }
}

/// Insert payload for `chunks`.
#[derive(Debug, Clone)]
pub struct NewChunk<'a> {
    pub item_id: i64,
    pub ordinal: i64,
    pub chunk_kind: &'a str,
    pub text: &'a str,
    pub symbol_name: Option<&'a str>,
    pub page_number: Option<i64>,
    /// Byte range in the extracted text (T201 chunk offsets).
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
}

/// One lexical hit. `rank` is FTS5 bm25 (lower = better; negative values).
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub chunk_id: i64,
    pub item_id: i64,
    pub rank: f64,
    /// Excerpt with matches wrapped in [`HIGHLIGHT_START`]/[`HIGHLIGHT_END`].
    pub snippet: String,
}

/// A database connection with Lumen's pragmas applied.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database for writing, enables WAL and migrates.
    ///
    /// # Errors
    /// I/O, WAL unavailable, schema newer than this binary, migration failure.
    pub fn open_writer(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StorageError::WalUnavailable(mode));
        }
        // NORMAL is durable at checkpoints and safe against corruption in WAL mode; the
        // index is rebuildable, so losing the last transactions on power loss is acceptable.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        migrations::apply(&mut conn, MIGRATIONS)?;
        Ok(Self { conn })
    }

    /// Opens a read-only connection for search. The writer must have created the database.
    ///
    /// # Errors
    /// Missing database or a schema this binary does not know.
    pub fn open_reader(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "query_only", "ON")?;
        let found = migrations::current_version(&conn)?;
        if found != latest_version() {
            return Err(if found > latest_version() {
                StorageError::SchemaTooNew {
                    found,
                    supported: latest_version(),
                }
            } else {
                StorageError::Corrupt(format!("schema v{found} not migrated"))
            });
        }
        Ok(Self { conn })
    }

    /// Current `user_version`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn schema_version(&self) -> Result<u32> {
        migrations::current_version(&self.conn)
    }

    /// Inserts an item, returning its id.
    ///
    /// # Errors
    /// Constraint violations (duplicate path or file identity).
    pub fn insert_item(&self, item: &NewItem<'_>) -> Result<i64> {
        self.conn
            .prepare_cached(
                "INSERT INTO items (kind, volume_id, file_id, canonical_path, display_name,
                                    extension, size_bytes, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?
            .execute(params![
                item.kind.as_str(),
                item.volume_id,
                item.file_id,
                item.canonical_path,
                item.display_name,
                item.extension,
                item.size_bytes,
                item.modified_at
            ])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Item id by path (case-insensitive, like Windows).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn item_id_by_path(&self, path: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .prepare_cached("SELECT id FROM items WHERE canonical_path = ?1 COLLATE NOCASE")?
            .query_row([path], |r| r.get(0))
            .optional()?)
    }

    /// Deletes an item and (by cascade) its chunks, FTS rows and usage events.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn delete_item(&self, item_id: i64) -> Result<bool> {
        Ok(self
            .conn
            .prepare_cached("DELETE FROM items WHERE id = ?1")?
            .execute([item_id])?
            > 0)
    }

    /// Inserts chunks in one transaction (the indexer's batch unit).
    ///
    /// # Errors
    /// Constraint violations roll back the whole batch.
    pub fn insert_chunks(&mut self, chunks: &[NewChunk<'_>]) -> Result<Vec<i64>> {
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(chunks.len());
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO chunks (item_id, ordinal, chunk_kind, text, symbol_name, page_number,
                                     start_offset, end_offset)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for c in chunks {
                stmt.execute(params![
                    c.item_id,
                    c.ordinal,
                    c.chunk_kind,
                    c.text,
                    c.symbol_name,
                    c.page_number,
                    c.start_offset,
                    c.end_offset
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Replaces a chunk's text (FTS kept in sync by trigger).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn update_chunk_text(&self, chunk_id: i64, text: &str) -> Result<bool> {
        Ok(self
            .conn
            .prepare_cached("UPDATE chunks SET text = ?2 WHERE id = ?1")?
            .execute(params![chunk_id, text])?
            > 0)
    }

    /// Lexical search over chunk text, best bm25 first, capped at `limit` (fusion only needs
    /// the top candidates, docs/PERFORMANCE.md §8), bounded by `budget`.
    ///
    /// # Errors
    /// [`StorageError::Interrupted`] when the budget ran out or the query was cancelled;
    /// SQLite failure otherwise.
    pub fn search_chunks(
        &self,
        query: &FtsQuery,
        limit: usize,
        budget: &SearchBudget,
    ) -> Result<Vec<ChunkHit>> {
        self.bounded(budget, |store| store.search_chunks_inner(query, limit))
    }

    /// Runs `f` with SQLite's progress handler enforcing `budget`.
    pub(crate) fn bounded<T>(
        &self,
        budget: &SearchBudget,
        f: impl FnOnce(&Self) -> Result<T>,
    ) -> Result<T> {
        if budget.is_bounded() {
            let deadline = budget.deadline;
            let cancel = budget.cancel.clone();
            self.conn.progress_handler(
                PROGRESS_STEPS,
                Some(move || {
                    deadline.is_some_and(|d| Instant::now() >= d)
                        || cancel.as_ref().is_some_and(CancellationToken::is_cancelled)
                }),
            )?;
        }
        let result = f(self);
        if budget.is_bounded() {
            self.conn.progress_handler(0, None::<fn() -> bool>)?;
        }
        result
    }

    fn search_chunks_inner(&self, query: &FtsQuery, limit: usize) -> Result<Vec<ChunkHit>> {
        let sql = format!(
            "SELECT c.id, c.item_id, bm25(chunks_fts) AS rank,
                    snippet(chunks_fts, 0, '{HIGHLIGHT_START}', '{HIGHLIGHT_END}', '…', 16)
             FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.rowid
             WHERE chunks_fts MATCH ?1
             ORDER BY rank LIMIT ?2"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(
            params![query.as_str(), i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| {
                Ok(ChunkHit {
                    chunk_id: r.get(0)?,
                    item_id: r.get(1)?,
                    rank: r.get(2)?,
                    snippet: r.get(3)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Writes the WAL back into the main file (call when idle; keeps the WAL small).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn checkpoint(&self) -> Result<()> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }

    /// `EXPLAIN QUERY PLAN` lines for `sql` (dev/test diagnostics, PERFORMANCE.md §8).
    ///
    /// # Errors
    /// Invalid SQL.
    pub fn query_plan(&self, sql: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(3))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Escape hatch for tests and benchmarks inside the workspace.
    #[doc(hidden)]
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    pub(crate) struct TempDb(PathBuf);

    impl TempDb {
        pub(crate) fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "lumen-storage-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        pub(crate) fn path(&self) -> PathBuf {
            self.0.join("lumen.db")
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn seeded(db: &TempDb) -> (Store, i64, i64) {
        let mut store = Store::open_writer(&db.path()).unwrap();
        let notes = store
            .insert_item(&NewItem::file(
                r"C:\Users\Joao\Notas\reunion.md",
                "reunion.md",
            ))
            .unwrap();
        let code = store
            .insert_item(&NewItem::file(r"C:\src\http_client.py", "http_client.py"))
            .unwrap();
        store
            .insert_chunks(&[
                NewChunk {
                    item_id: notes,
                    ordinal: 0,
                    chunk_kind: "text",
                    text: "Notas de la reunión con el cliente: enviar el contrato firmado.",
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                },
                NewChunk {
                    item_id: code,
                    ordinal: 0,
                    chunk_kind: "code",
                    text: "def fetch_with_backoff(url): retry failed requests with exponential backoff",
                    symbol_name: Some("fetch_with_backoff"),
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                },
                NewChunk {
                    item_id: code,
                    ordinal: 1,
                    chunk_kind: "code",
                    text: "connection refused errors are retried; other HTTP errors are raised",
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                },
            ])
            .unwrap();
        (store, notes, code)
    }

    fn search(store: &Store, q: &str) -> Vec<ChunkHit> {
        store
            .search_chunks(
                &FtsQuery::from_user(q, true).unwrap(),
                10,
                &SearchBudget::unbounded(),
            )
            .unwrap()
    }

    #[test]
    fn bundled_sqlite_has_fts5_and_wal() {
        let db = TempDb::new("caps");
        let store = Store::open_writer(&db.path()).unwrap();
        let fts5: i64 = store
            .connection()
            .query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(fts5, 1);
        let mode: String = store
            .connection()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(store.schema_version().unwrap(), latest_version());
    }

    #[test]
    fn fts_is_accent_and_case_insensitive_with_highlights() {
        let db = TempDb::new("accents");
        let (store, notes, _) = seeded(&db);
        let hits = search(&store, "REUNION cliente");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item_id, notes);
        assert!(
            hits[0]
                .snippet
                .contains(&format!("{HIGHLIGHT_START}reunión{HIGHLIGHT_END}")),
            "{:?}",
            hits[0].snippet
        );
    }

    #[test]
    fn prefix_phrase_and_symbol_matching() {
        let db = TempDb::new("prefix");
        let (store, _, code) = seeded(&db);
        // Prefix while typing.
        assert_eq!(search(&store, "backo").len(), 1);
        // Phrase must be contiguous.
        assert_eq!(search(&store, "\"connection refused\"").len(), 1);
        assert!(search(&store, "\"refused connection\"").is_empty());
        // Symbol column is searchable (underscores split into tokens).
        let hits = search(&store, "fetch with backoff");
        assert!(hits.iter().all(|h| h.item_id == code) && !hits.is_empty());
        // Ranking: the chunk mentioning both terms more specifically ranks first.
        let hits = search(&store, "retry backoff ");
        assert_eq!(hits[0].item_id, code);
        assert!(hits.windows(2).all(|w| w[0].rank <= w[1].rank));
    }

    #[test]
    fn hostile_input_never_errors() {
        let db = TempDb::new("hostile");
        let (store, _, _) = seeded(&db);
        for q in [
            "NOT AND OR",
            "a:b",
            "\"",
            "(-^*)",
            "NEAR(x y",
            "' OR 1=1 --",
        ] {
            if let Some(query) = FtsQuery::from_user(q, true) {
                store
                    .search_chunks(&query, 5, &SearchBudget::unbounded())
                    .unwrap();
            }
        }
    }

    #[test]
    fn updates_and_deletes_keep_fts_in_sync() {
        let db = TempDb::new("sync");
        let (store, notes, code) = seeded(&db);
        let chunk = search(&store, "contrato")[0].chunk_id;
        store
            .update_chunk_text(chunk, "presupuesto revisado")
            .unwrap();
        assert!(search(&store, "contrato").is_empty());
        assert_eq!(search(&store, "presupuesto").len(), 1);
        assert!(store.delete_item(code).unwrap());
        assert!(search(&store, "backoff").is_empty());
        let chunks: i64 = store
            .connection()
            .query_row(
                "SELECT count(*) FROM chunks WHERE item_id = ?1",
                [code],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(chunks, 0, "cascade delete");
        assert_eq!(search(&store, "presupuesto")[0].item_id, notes);
    }

    #[test]
    fn paths_are_unique_exactly_and_found_case_insensitively() {
        let db = TempDb::new("paths");
        let (store, notes, _) = seeded(&db);
        assert_eq!(
            store
                .item_id_by_path(r"c:\users\joao\notas\REUNION.md")
                .unwrap(),
            Some(notes)
        );
        let exact_dup = store.insert_item(&NewItem::file(
            r"C:\Users\Joao\Notas\reunion.md",
            "reunion.md",
        ));
        assert!(exact_dup.is_err(), "duplicate path accepted");
        // A case-sensitive directory can hold both; neither may be dropped.
        store
            .insert_item(&NewItem::file(
                r"C:\USERS\JOAO\NOTAS\reunion.md",
                "reunion.md",
            ))
            .unwrap();
        let plan = store
            .query_plan("SELECT id FROM items WHERE canonical_path = 'x' COLLATE NOCASE")
            .unwrap()
            .join(" | ");
        assert!(plan.contains("items_path_nocase"), "{plan}");
        let plan = store
            .query_plan("SELECT 1 FROM chunk_vectors WHERE generation = 1 AND chunk_id = 2")
            .unwrap()
            .join(" | ");
        assert!(plan.contains("PRIMARY KEY"), "{plan}");
    }

    #[test]
    fn batch_insert_is_atomic() {
        let db = TempDb::new("atomic");
        let (mut store, notes, _) = seeded(&db);
        let err = store.insert_chunks(&[
            NewChunk {
                item_id: notes,
                ordinal: 5,
                chunk_kind: "text",
                text: "fresh chunk",
                symbol_name: None,
                page_number: None,
                start_offset: None,
                end_offset: None,
            },
            NewChunk {
                item_id: notes,
                ordinal: 0, // duplicate (item_id, ordinal)
                chunk_kind: "text",
                text: "duplicate",
                symbol_name: None,
                page_number: None,
                start_offset: None,
                end_offset: None,
            },
        ]);
        assert!(err.is_err());
        assert!(
            search(&store, "fresh").is_empty(),
            "partial batch committed"
        );
    }

    #[test]
    fn readers_are_not_blocked_by_an_open_write_transaction() {
        let db = TempDb::new("wal");
        let (store, notes, _) = seeded(&db);
        let reader = Store::open_reader(&db.path()).unwrap();
        let count = |s: &Store| -> i64 {
            s.connection()
                .query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))
                .unwrap()
        };
        let before = count(&reader);
        store.connection().execute_batch("BEGIN IMMEDIATE").unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO chunks (item_id, ordinal, chunk_kind, text) VALUES (?1, 9, 'text', 'uncommitted')",
                [notes],
            )
            .unwrap();
        // Reader sees the last committed snapshot immediately, without waiting.
        let started = Instant::now();
        assert_eq!(count(&reader), before);
        assert!(started.elapsed() < Duration::from_millis(500));
        store.connection().execute_batch("COMMIT").unwrap();
        assert_eq!(count(&reader), before + 1);
        // Readers cannot write.
        assert!(reader.insert_item(&NewItem::file("x", "x")).is_err());
        store.checkpoint().unwrap();
    }

    #[test]
    fn budget_and_cancellation_interrupt_queries() {
        let db = TempDb::new("budget");
        let (store, _, _) = seeded(&db);
        let query = FtsQuery::from_user("retry", false).unwrap();
        // Already expired deadline / cancelled token: interrupted, never a wrong answer.
        let expired = SearchBudget {
            deadline: Some(Instant::now()),
            cancel: None,
        };
        // Tiny corpora may finish before the first progress check; both outcomes are fine,
        // but an error must be `Interrupted`.
        match store.search_chunks(&query, 5, &expired) {
            Ok(_) | Err(StorageError::Interrupted) => {}
            Err(e) => panic!("unexpected {e}"),
        }
        let token = CancellationToken::new();
        token.cancel();
        match store.search_chunks(&query, 5, &SearchBudget::unbounded().with_cancel(token)) {
            Ok(_) | Err(StorageError::Interrupted) => {}
            Err(e) => panic!("unexpected {e}"),
        }
        // The handler is removed afterwards: unbounded queries still work.
        assert_eq!(search(&store, "retry").len(), 1);
        let generous = SearchBudget::within(Duration::from_secs(5));
        assert_eq!(store.search_chunks(&query, 5, &generous).unwrap().len(), 1);
    }

    #[test]
    fn budget_interrupts_a_genuinely_slow_query() {
        let db = TempDb::new("slow");
        let mut store = Store::open_writer(&db.path()).unwrap();
        let item = store
            .insert_item(&NewItem::file("c:/big.txt", "big.txt"))
            .unwrap();
        let text = "the common word appears everywhere ".repeat(40);
        let chunks: Vec<NewChunk<'_>> = (0..20_000)
            .map(|i| NewChunk {
                item_id: item,
                ordinal: i,
                chunk_kind: "text",
                text: &text,
                symbol_name: None,
                page_number: None,
                start_offset: None,
                end_offset: None,
            })
            .collect();
        store.insert_chunks(&chunks).unwrap();
        let query = FtsQuery::from_user("common", false).unwrap();
        let started = Instant::now();
        let result =
            store.search_chunks(&query, 10, &SearchBudget::within(Duration::from_millis(1)));
        assert!(
            matches!(result, Err(StorageError::Interrupted)),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn reader_refuses_unmigrated_database() {
        let db = TempDb::new("unmigrated");
        Connection::open(db.path()).unwrap();
        assert!(Store::open_reader(&db.path()).is_err());
    }
}
