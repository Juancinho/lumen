-- 0003 ANN generations (T203, ADR-031).
--
-- Every stored result gets a per-generation write sequence number, so a derived ANN file can
-- say exactly which results it contains ("everything with seq <= built_through_seq"):
-- results written later (new chunks, re-embedded chunks whose id SQLite reused) are found
-- in the in-memory delta, and a file hit whose row is gone or newer is dropped. Rows that
-- existed before this migration get seq 0 (older than any file).
ALTER TABLE generations ADD COLUMN next_seq INTEGER NOT NULL DEFAULT 1;
ALTER TABLE generations ADD COLUMN activated_at INTEGER;
ALTER TABLE chunk_vectors ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;
CREATE INDEX chunk_vectors_seq ON chunk_vectors (generation, seq);

-- The current ANN file of a generation (derived data: deleting the row or the file only
-- costs a rebuild from chunk_vectors). `file_name` is relative to the app's vector folder.
CREATE TABLE ann_files (
    generation        INTEGER PRIMARY KEY REFERENCES generations (id) ON DELETE CASCADE,
    file_name         TEXT    NOT NULL,
    built_through_seq INTEGER NOT NULL,
    vectors           INTEGER NOT NULL CHECK (vectors >= 0),
    -- lumen-vector configuration the file was built with (scalar, metric, M, ef_construction).
    index_config      TEXT    NOT NULL,
    built_at          INTEGER NOT NULL
) STRICT;
