-- 0001 initial schema (T007, catalog columns T101). Forward-only once released: until the
-- first release it may still change (ADR-017); afterwards add a new migration.
-- Times are Unix epoch milliseconds (UTC). Paths keep display form; comparisons are
-- case-insensitive like Windows file systems.

CREATE TABLE items (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT    NOT NULL CHECK (kind IN ('file', 'folder', 'application')),
    -- 'files' = inventory of an indexed root (T009/T101); 'apps' = application catalog.
    source              TEXT    NOT NULL DEFAULT 'files' CHECK (source IN ('files', 'apps')),
    -- Stable identity where available (ADR-018): volume serial + file id. Not unique:
    -- hard links give several paths one identity.
    volume_id           TEXT,
    file_id             TEXT,
    -- Display/lookup form. Lossless unless `raw_path` is set (non-Unicode names, escaped).
    canonical_path      TEXT    NOT NULL,
    -- Exact OS path when `canonical_path` is not (Windows UTF-16LE, Unix bytes).
    raw_path            BLOB,
    display_name        TEXT    NOT NULL,
    -- Search key: display_name case-folded with diacritics removed (T101/T102).
    name_key            TEXT    NOT NULL DEFAULT '',
    extension           TEXT,
    -- Applications: what to launch (AppsFolder parsing name, shortcut path).
    launch_target       TEXT,
    -- Bit flags: 1 hidden, 2 system, 4 cloud placeholder (ADR-018).
    attributes          INTEGER NOT NULL DEFAULT 0,
    size_bytes          INTEGER,
    modified_at         INTEGER,
    created_at          INTEGER,
    indexed_at          INTEGER,
    extractor_version   INTEGER,
    content_fingerprint TEXT,
    status              TEXT    NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'indexed', 'error', 'excluded')),
    error_code          TEXT,
    -- Last inventory pass (`scans.id`) that saw the item; older = candidate for removal.
    seen_scan           INTEGER
) STRICT;

-- Exact path is the item key (two entries differing only in case stay two items on
-- case-sensitive directories); lookups fold case like Windows.
CREATE UNIQUE INDEX items_path ON items (canonical_path);
CREATE INDEX items_path_nocase ON items (canonical_path COLLATE NOCASE);
CREATE INDEX items_identity ON items (volume_id, file_id) WHERE file_id IS NOT NULL;
CREATE INDEX items_name_key ON items (name_key);
CREATE INDEX items_modified ON items (modified_at);
CREATE INDEX items_status ON items (status) WHERE status <> 'indexed';
CREATE INDEX items_seen ON items (source, seen_scan);

-- One inventory pass (T101). `complete` = no cancellation and no directory failed to list.
CREATE TABLE scans (
    id          INTEGER PRIMARY KEY,
    source      TEXT    NOT NULL CHECK (source IN ('files', 'apps')),
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    complete    INTEGER NOT NULL DEFAULT 0,
    seen        INTEGER NOT NULL DEFAULT 0,
    removed     INTEGER NOT NULL DEFAULT 0,
    issues      INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE chunks (
    id                   INTEGER PRIMARY KEY,
    item_id              INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    ordinal              INTEGER NOT NULL,
    chunk_kind           TEXT    NOT NULL,
    start_offset         INTEGER,
    end_offset           INTEGER,
    page_number          INTEGER,
    media_start_ms       INTEGER,
    media_end_ms         INTEGER,
    symbol_name          TEXT,
    text                 TEXT    NOT NULL,
    -- Index generation that holds this chunk's vector (T203); NULL = not embedded yet.
    embedding_generation INTEGER,
    UNIQUE (item_id, ordinal)
) STRICT;

CREATE INDEX chunks_unembedded ON chunks (item_id) WHERE embedding_generation IS NULL;

-- Lexical search over chunk text. External content: text lives once, in `chunks`.
-- unicode61 + remove_diacritics 2: case- and accent-insensitive ("reunion" ~ "reunión").
CREATE VIRTUAL TABLE chunks_fts USING fts5 (
    text,
    symbol_name,
    content = 'chunks',
    content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    -- Prefix indexes make `abc*` term lookups O(log n) while typing (T007 benchmark).
    prefix = '3 4'
);

CREATE TRIGGER chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts (rowid, text, symbol_name) VALUES (new.id, new.text, new.symbol_name);
END;

CREATE TRIGGER chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text, symbol_name)
    VALUES ('delete', old.id, old.text, old.symbol_name);
END;

CREATE TRIGGER chunks_fts_update AFTER UPDATE OF text, symbol_name ON chunks BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text, symbol_name)
    VALUES ('delete', old.id, old.text, old.symbol_name);
    INSERT INTO chunks_fts (rowid, text, symbol_name) VALUES (new.id, new.text, new.symbol_name);
END;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL  -- JSON
) STRICT;

CREATE TABLE usage_events (
    id          INTEGER PRIMARY KEY,
    item_id     INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    event_kind  TEXT    NOT NULL,
    occurred_at INTEGER NOT NULL
) STRICT;

CREATE INDEX usage_item_time ON usage_events (item_id, occurred_at);
