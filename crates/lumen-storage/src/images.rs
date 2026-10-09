//! Image metadata/units on the existing single content writer (T303, ADR-041).
use crate::{ContentCandidate, Result, StorageError, Store};
use rusqlite::{OptionalExtension, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageCounts {
    pub indexed: u64,
    pub pending: u64,
    pub failed: u64,
    pub skipped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub format: String,
    pub digest: Vec<u8>,
}

impl Store {
    /// An image changed during inference: remove its stale unit and re-run metadata.
    /// # Errors
    /// SQLite failure.
    pub fn invalidate_image(&mut self, item: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM chunks WHERE item_id=?1 AND chunk_kind='image'",
            [item],
        )?;
        tx.execute("UPDATE items SET content_state=NULL,content_fingerprint=NULL,content_error=NULL WHERE id=?1", [item])?;
        tx.commit()?;
        Ok(())
    }
    /// Replace one image unit and metadata atomically, only if its catalog identity is current.
    /// # Errors
    /// SQLite failure. Returns false for a changed/deleted catalog item.
    pub fn write_image(
        &mut self,
        candidate: &ContentCandidate,
        metadata: &ImageMetadata,
        version: u32,
        now: i64,
    ) -> Result<bool> {
        if metadata.width == 0
            || metadata.height == 0
            || !(1..=8).contains(&metadata.orientation)
            || metadata.digest.len() != 32
        {
            return Err(StorageError::Corrupt("invalid image metadata".into()));
        }
        let tx = self.conn.transaction()?;
        let current: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id=?1 AND canonical_path=?2
            AND size_bytes IS ?3 AND modified_at IS ?4)",
            params![
                candidate.item_id,
                candidate.path,
                candidate.size_bytes,
                candidate.modified_at
            ],
            |r| r.get(0),
        )?;
        if !current {
            return Ok(false);
        }
        tx.execute("DELETE FROM chunks WHERE item_id=?1", [candidate.item_id])?;
        tx.execute("INSERT INTO chunks(item_id,ordinal,chunk_kind,text,search_context) VALUES(?1,0,'image','','')",[candidate.item_id])?;
        tx.execute(
            "UPDATE items SET image_width=?2,image_height=?3,image_orientation=?4,image_format=?5,
            image_digest=?6,image_version=?7,content_state='indexed',content_error=NULL,
            content_fingerprint=?8,extractor_version=1,indexed_at=?9 WHERE id=?1",
            params![
                candidate.item_id,
                metadata.width,
                metadata.height,
                metadata.orientation,
                metadata.format,
                metadata.digest,
                version,
                candidate.fingerprint(),
                now
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// Stored local digest used only for bounded ambiguous-move verification.
    /// # Errors
    /// SQLite failure.
    pub fn image_digest(&self, item: i64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row("SELECT image_digest FROM items WHERE id=?1", [item], |r| {
                r.get(0)
            })
            .optional()?
            .flatten())
    }

    /// Visual coverage of admitted image units; skipped files have no embedding unit.
    /// # Errors
    /// SQLite failure.
    pub fn image_counts(&self, generation: Option<i64>) -> Result<ImageCounts> {
        let (indexed, pending, failed) = self.conn.query_row(
            "SELECT count(CASE WHEN v.vector IS NOT NULL THEN 1 END),
             count(CASE WHEN v.chunk_id IS NULL THEN 1 END),
             count(CASE WHEN v.error_code IS NOT NULL THEN 1 END)
             FROM chunks c JOIN items i ON i.id=c.item_id
             LEFT JOIN chunk_vectors v ON v.chunk_id=c.id AND v.generation=?1
             WHERE c.chunk_kind='image' AND i.content_state='indexed'",
            [generation],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )?;
        let skipped = self.conn.query_row(
            "SELECT count(*) FROM items WHERE content_state='skipped'
            AND content_error LIKE 'image:%'",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        let count = |n| u64::try_from(n).unwrap_or(0);
        let file_failures: i64 = self.conn.query_row(
            "SELECT count(*) FROM items WHERE content_state='failed'
             AND content_error LIKE 'image:%'",
            [],
            |r| r.get(0),
        )?;
        Ok(ImageCounts {
            indexed: count(indexed),
            pending: count(pending),
            failed: count(failed + file_failures),
            skipped: count(skipped),
        })
    }
}
