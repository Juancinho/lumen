//! Independent OCR coverage; enrich one image's FTS without replacing its visual vector.
use crate::{Result, StorageError, Store};
use rusqlite::{OptionalExtension, params};
pub const VERSION: u32 = 1;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
#[derive(Debug, Clone)]
pub struct Candidate {
    pub item: i64,
    pub chunk: i64,
    pub path: String,
    pub raw_path: Option<Vec<u8>>,
    pub digest: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub indexed: u64,
    pub empty: u64,
    pub pending: u64,
    pub skipped: u64,
    pub failed: u64,
}
#[derive(Debug, Clone)]
pub struct Preview {
    pub path: String,
    pub state: String,
    pub language: Option<String>,
    pub reason: Option<String>,
    pub text: String,
}
impl Store {
    /// # Errors
    /// SQLite failure. Scope admission remains the caller's responsibility.
    pub fn ocr_candidates(&self, after: i64, limit: usize) -> Result<Vec<Candidate>> {
        let mut q=self.conn.prepare("SELECT i.id,c.id,i.canonical_path,i.raw_path,i.image_digest,i.image_width,i.image_height
            FROM items i JOIN chunks c ON c.item_id=i.id AND c.chunk_kind='image'
            LEFT JOIN image_ocr o ON o.item_id=i.id
            WHERE i.id>?1 AND i.status NOT IN ('error','excluded') AND i.content_state='indexed'
            AND i.image_digest IS NOT NULL AND (o.item_id IS NULL OR o.source_digest!=i.image_digest
                OR o.version!=?2 OR o.state='failed' OR (o.state='indexed' AND c.text=''))
            ORDER BY i.id LIMIT ?3")?;
        Ok(q.query_map(
            params![after, VERSION, i64::try_from(limit.min(64)).unwrap_or(64)],
            |r| {
                Ok(Candidate {
                    item: r.get(0)?,
                    chunk: r.get(1)?,
                    path: r.get(2)?,
                    raw_path: r.get(3)?,
                    digest: r.get(4)?,
                    width: r.get(5)?,
                    height: r.get(6)?,
                })
            },
        )?
        .collect::<std::result::Result<_, _>>()?)
    }
    /// Atomic FTS enrichment + coverage with current identity validation.
    /// # Errors
    /// Invalid bounded output or SQLite failure. False means changed/deleted source.
    pub fn write_ocr(
        &mut self,
        c: &Candidate,
        text: &str,
        language: Option<&str>,
        state: &str,
        reason: Option<&str>,
    ) -> Result<bool> {
        if c.digest.len() != 32
            || text.len() > MAX_TEXT_BYTES
            || text.contains('\0')
            || !["indexed", "empty", "skipped", "failed"].contains(&state)
            || (state == "indexed") == text.trim().is_empty()
            || (state != "indexed" && !text.is_empty())
            || reason.is_some_and(|s| !valid_reason(s))
            || language.is_some_and(|s| s.len() > 80)
        {
            return Err(StorageError::Corrupt("invalid OCR output".into()));
        }
        let tx = self.conn.transaction()?;
        let changed=tx.execute("UPDATE chunks SET text=?3,start_offset=0,end_offset=?4 WHERE id=?1 AND item_id=?2 AND chunk_kind='image'
            AND EXISTS(SELECT 1 FROM items WHERE id=?2 AND canonical_path=?5 AND image_digest=?6 AND content_state='indexed')",
            params![c.chunk,c.item,text,i64::try_from(text.len()).unwrap_or(0),c.path,c.digest])?;
        if changed == 0 {
            return Ok(false);
        }
        tx.execute("INSERT INTO image_ocr(item_id,source_digest,version,state,language,error_code) VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(item_id) DO UPDATE SET source_digest=excluded.source_digest,version=excluded.version,
            state=excluded.state,language=excluded.language,error_code=excluded.error_code",params![c.item,c.digest,VERSION,state,language,reason])?;
        tx.commit()?;
        Ok(true)
    }
    /// Remove only OCR text/coverage in one bounded writer page. Disk and vectors stay.
    /// # Errors
    /// SQLite failure.
    pub fn clear_ocr_page(&mut self) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let ids: Vec<i64> = {
            let mut q = tx.prepare("SELECT item_id FROM image_ocr ORDER BY item_id LIMIT 512")?;
            q.query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        for id in &ids {
            tx.execute("UPDATE chunks SET text='',start_offset=NULL,end_offset=NULL WHERE item_id=?1 AND chunk_kind='image'",[id])?;
            tx.execute("DELETE FROM image_ocr WHERE item_id=?1", [id])?;
        }
        tx.commit()?;
        Ok(ids.len())
    }
    /// # Errors
    /// SQLite failure. Read on a worker, not under a shared status lock.
    pub fn ocr_counts(&self, scope: &dyn Fn(&str) -> bool) -> Result<Counts> {
        let mut counts = Counts::default();
        let mut q=self.conn.prepare("SELECT i.canonical_path,CASE WHEN o.source_digest=i.image_digest AND o.version=?1 THEN o.state ELSE NULL END
            FROM items i JOIN chunks c ON c.item_id=i.id AND c.chunk_kind='image' LEFT JOIN image_ocr o ON o.item_id=i.id WHERE i.content_state='indexed'")?;
        for row in q.query_map([VERSION], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })? {
            let (path, state) = row?;
            if !scope(&path) {
                continue;
            }
            match state.as_deref() {
                Some("indexed") => counts.indexed += 1,
                Some("empty") => counts.empty += 1,
                Some("skipped") => counts.skipped += 1,
                Some("failed") => counts.failed += 1,
                _ => counts.pending += 1,
            }
        }
        Ok(counts)
    }
    /// # Errors
    /// SQLite failure. No file reads or raw pixels.
    pub fn image_ocr_preview(&self, item: i64) -> Result<Option<Preview>> {
        Ok(self.conn.query_row("SELECT o.state,o.language,o.error_code,c.text,i.canonical_path FROM image_ocr o
            JOIN items i ON i.id=o.item_id AND i.image_digest=o.source_digest
            JOIN chunks c ON c.item_id=i.id AND c.chunk_kind='image' WHERE i.id=?1 AND o.version=?2",
            params![item,VERSION],|r|Ok(Preview{state:r.get(0)?,language:r.get(1)?,reason:r.get(2)?,text:r.get(3)?,path:r.get(4)?})).optional()?)
    }
}

#[must_use]
pub fn valid_reason(reason: &str) -> bool {
    matches!(
        reason,
        "ocr:pixel_limit"
            | "ocr:text_limit"
            | "ocr:timeout"
            | "ocr:recognition"
            | "image:unsupported"
            | "image:source_limit"
            | "image:pixel_limit"
            | "image:decode"
            | "image:io"
            | "image:placeholder"
            | "image:changed"
    )
}
