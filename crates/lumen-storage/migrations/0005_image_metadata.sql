-- T303: additive image context. Existing text/PDF/code chunks, vectors and generations stay.
ALTER TABLE items ADD COLUMN image_width INTEGER;
ALTER TABLE items ADD COLUMN image_height INTEGER;
ALTER TABLE items ADD COLUMN image_orientation INTEGER;
ALTER TABLE items ADD COLUMN image_format TEXT;
ALTER TABLE items ADD COLUMN image_digest BLOB;
ALTER TABLE items ADD COLUMN image_version INTEGER;

-- Catalog invalidation (including known same-metadata writes) clears stale image context.
CREATE TRIGGER items_image_invalidate AFTER UPDATE OF content_state ON items
WHEN new.content_state IS NULL OR new.content_state = 'skipped' BEGIN
    UPDATE items SET image_width = NULL, image_height = NULL, image_orientation = NULL,
        image_format = NULL, image_digest = NULL, image_version = NULL WHERE id = new.id;
END;
