-- 0002 content indexing + embedding queue (T202, ADR-029).
--
-- Content state of a file item, separate from the inventory `status` (which says whether
-- its metadata could be read). NULL = content never processed.
--   indexed = text extracted and chunked (possibly zero chunks for an empty file);
--   skipped = deliberately not read (binary behind a text extension, too large);
--   failed  = reading failed (`content_error` = io kind), retried on the next pass.
-- `extractor_version` + `content_fingerprint` ("<size>:<mtime>") at processing time tell a
-- later pass whether the file must be read again.
ALTER TABLE items ADD COLUMN content_state TEXT
    CHECK (content_state IN ('indexed', 'skipped', 'failed'));
ALTER TABLE items ADD COLUMN content_error TEXT;

-- A chunk is embedded once per generation (`chunk_vectors`), so a new generation can be
-- built while the old one stays searchable (docs/SEARCH_AND_INDEXING.md §16).
DROP INDEX chunks_unembedded;
ALTER TABLE chunks DROP COLUMN embedding_generation;

-- One vector space x chunker version (T203 adds validation and switching).
CREATE TABLE generations (
    id              INTEGER PRIMARY KEY,
    -- EmbeddingSpace::key(): model, weights revision, preprocessing, prompts, dim, norm.
    space_key       TEXT    NOT NULL,
    chunker_version INTEGER NOT NULL,
    dim             INTEGER NOT NULL CHECK (dim > 0),
    scalar          TEXT    NOT NULL CHECK (scalar IN ('f16')),
    state           TEXT    NOT NULL DEFAULT 'building'
                    CHECK (state IN ('building', 'active', 'retired')),
    created_at      INTEGER NOT NULL,
    UNIQUE (space_key, chunker_version)
) STRICT;

-- The embedding results: the expensive, durable part of the index (hours of CPU). The ANN
-- file is derived from this table and can always be rebuilt from it (ADR-016).
-- Exactly one of `vector` (dim little-endian IEEE f16) and `error_code` is set; a failed
-- chunk is not retried in the same generation.
CREATE TABLE chunk_vectors (
    chunk_id    INTEGER NOT NULL REFERENCES chunks (id) ON DELETE CASCADE,
    generation  INTEGER NOT NULL REFERENCES generations (id) ON DELETE CASCADE,
    vector      BLOB,
    error_code  TEXT,
    embedded_at INTEGER NOT NULL,
    CHECK ((vector IS NULL) <> (error_code IS NULL)),
    PRIMARY KEY (generation, chunk_id)
) STRICT, WITHOUT ROWID;

-- ON DELETE CASCADE from chunks looks rows up by chunk id.
CREATE INDEX chunk_vectors_chunk ON chunk_vectors (chunk_id);
