-- T209: derived lexical context and background-discovered repository metadata.
-- Chunk identities, texts, vectors and generations are preserved.
ALTER TABLE items ADD COLUMN code_language TEXT;
ALTER TABLE items ADD COLUMN repository_path TEXT;
ALTER TABLE items ADD COLUMN code_context_path TEXT;
ALTER TABLE chunks ADD COLUMN search_context TEXT NOT NULL DEFAULT '';

DROP TRIGGER chunks_fts_insert;
DROP TRIGGER chunks_fts_delete;
DROP TRIGGER chunks_fts_update;
DROP TABLE chunks_fts;

UPDATE chunks SET search_context = (
    SELECT name_parts || ' ' || path_parts || ' ' || display_name
    FROM items WHERE items.id = chunks.item_id
) WHERE chunk_kind = 'code';

CREATE VIRTUAL TABLE chunks_fts USING fts5 (
    text, symbol_name, search_context,
    content = 'chunks', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2', prefix = '3 4'
);
CREATE TRIGGER chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts (rowid, text, symbol_name, search_context)
    VALUES (new.id, new.text, new.symbol_name, new.search_context);
END;
CREATE TRIGGER chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text, symbol_name, search_context)
    VALUES ('delete', old.id, old.text, old.symbol_name, old.search_context);
END;
CREATE TRIGGER chunks_fts_update AFTER UPDATE OF text, symbol_name, search_context ON chunks
WHEN old.text IS NOT new.text OR old.symbol_name IS NOT new.symbol_name
     OR old.search_context IS NOT new.search_context BEGIN
    INSERT INTO chunks_fts (chunks_fts, rowid, text, symbol_name, search_context)
    VALUES ('delete', old.id, old.text, old.symbol_name, old.search_context);
    INSERT INTO chunks_fts (rowid, text, symbol_name, search_context)
    VALUES (new.id, new.text, new.symbol_name, new.search_context);
END;
CREATE TRIGGER items_code_context AFTER UPDATE OF name_parts, path_parts, display_name,
    code_language ON items
WHEN old.name_parts IS NOT new.name_parts OR old.path_parts IS NOT new.path_parts
     OR old.display_name IS NOT new.display_name OR old.code_language IS NOT new.code_language BEGIN
    -- Read the current item: another trigger may already have invalidated its language
    -- after a move. The outer UPDATE's NEW record would still carry that old language.
    UPDATE chunks SET search_context = (SELECT name_parts || ' ' || path_parts || ' ' ||
        display_name || ' ' || coalesce(code_language, '') FROM items WHERE id = new.id)
    WHERE item_id = new.id AND chunk_kind = 'code';
END;
-- A move invalidates the repository immediately; the next background pass rediscovers it.
CREATE TRIGGER items_code_move AFTER UPDATE OF canonical_path ON items
WHEN old.canonical_path IS NOT new.canonical_path BEGIN
    UPDATE items SET repository_path = NULL, code_context_path = NULL, code_language = NULL
    WHERE id = new.id;
END;
INSERT INTO chunks_fts (chunks_fts) VALUES ('rebuild');
