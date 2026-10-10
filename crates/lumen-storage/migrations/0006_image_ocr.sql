-- T304: OCR enriches FTS on the existing image unit; visual vectors/seq stay untouched.
CREATE TABLE image_ocr (
    item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
    source_digest BLOB NOT NULL CHECK(length(source_digest)=32),
    version INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('indexed','empty','skipped','failed')),
    language TEXT,
    error_code TEXT
) STRICT;
CREATE TRIGGER items_ocr_invalidate AFTER UPDATE OF content_state ON items
WHEN new.content_state IS NULL OR new.content_state='skipped' BEGIN
    DELETE FROM image_ocr WHERE item_id=new.id;
END;
