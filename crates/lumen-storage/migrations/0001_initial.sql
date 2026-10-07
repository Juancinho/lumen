-- 0001 initial schema (T007). Forward-only; never edit after release, add a new migration.
-- Times are Unix epoch milliseconds (UTC). Paths keep display form; comparisons are
-- case-insensitive like Windows file systems.

CREATE TABLE items (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT    NOT NULL CHECK (kind IN ('file', 'folder', 'application')),
    -- Stable identity where available (T009): volume serial + file id.
    volume_id           TEXT,
    file_id             TEXT,
    canonical_path      TEXT    NOT NULL,
    display_name        TEXT    NOT NULL,
    extension           TEXT,
    size_bytes          INTEGER,
    modified_at         INTEGER,
    created_at          INTEGER,
    indexed_at          INTEGER,
    extractor_version   INTEGER,
    content_fingerprint TEXT,
    status              TEXT    NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'indexed', 'error', 'excluded')),
    error_code          TEXT,
    UNIQUE (volume_id, file_id)
) STRICT;

CREATE UNIQUE INDEX items_path ON items (canonical_path COLLATE NOCASE);
CREATE INDEX items_name ON items (display_name COLLATE NOCASE);
CREATE INDEX items_modified ON items (modified_at);
CREATE INDEX items_status ON items (status) WHERE status <> 'indexed';

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
