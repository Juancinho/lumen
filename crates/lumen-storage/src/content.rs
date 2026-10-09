//! Content indexing state and the persistent embedding queue (T202, ADR-029).
//!
//! - **Content pass:** [`Store::content_candidates`] lists text files whose content was never
//!   processed, changed since (`content_fingerprint`), failed last time, or was processed by
//!   an older extractor; [`Store::write_content`] replaces their chunks in one transaction.
//! - **Embedding queue:** the queue *is* the database — a chunk is pending for a generation
//!   while it has no `chunk_vectors` row there — so it survives restarts and needs no
//!   in-memory backlog. [`Store::pending_chunks`] pages through it by chunk id (keyset), and
//!   [`Store::write_vectors`] stores results as little-endian f16 (ADR-016).

use rusqlite::{OptionalExtension, params};

use crate::{NewChunk, Result, StorageError, Store};

/// A file whose content needs (re)processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentCandidate {
    pub item_id: i64,
    pub path: String,
    /// Exact OS path when `path` is not lossless (see `items.raw_path`).
    pub raw_path: Option<Vec<u8>>,
    pub name: String,
    pub extension: String,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<i64>,
}

impl ContentCandidate {
    /// The change-detection key stored with the content (docs/SEARCH_AND_INDEXING.md §8).
    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(self.size_bytes, self.modified_at)
    }
}

/// `"<size>:<mtime>"`, empty parts for unknown values. Must match the SQL in
/// [`Store::content_candidates`].
#[must_use]
pub fn fingerprint(size_bytes: Option<i64>, modified_at: Option<i64>) -> String {
    let part = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_default();
    format!("{}:{}", part(size_bytes), part(modified_at))
}

/// What the content pass found for one item.
#[derive(Debug, Clone)]
pub enum ContentOutcome<'a> {
    /// Extracted and chunked (an empty file has no chunks). `NewChunk::item_id` is ignored:
    /// the chunks belong to [`ContentWrite::item_id`].
    Indexed(Vec<NewChunk<'a>>),
    /// Deliberately not read (`binary`, `too_large`): no chunks, not retried until the file
    /// changes.
    Skipped(&'a str),
    /// Reading failed (`io:<kind>`): old chunks are kept, retried on the next pass.
    Failed(&'a str),
}

#[derive(Debug, Clone)]
pub struct ContentWrite<'a> {
    pub item_id: i64,
    /// [`ContentCandidate::fingerprint`] read before extraction.
    pub fingerprint: &'a str,
    pub outcome: ContentOutcome<'a>,
}

/// What defines a generation (docs/SEARCH_AND_INDEXING.md §16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationSpec<'a> {
    /// `EmbeddingSpace::key()`.
    pub space_key: &'a str,
    pub chunker_version: u32,
    pub dim: usize,
}

/// A chunk waiting for its vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub chunk_id: i64,
    pub item_id: i64,
    pub text: String,
    /// The item's display name: the document prompt's title (ADR-028).
    pub title: String,
}

/// One embedding result.
#[derive(Debug, Clone, Copy)]
pub struct VectorWrite<'a> {
    pub chunk_id: i64,
    /// The normalized vector, or a short error code (never content).
    pub result: std::result::Result<&'a [f32], &'a str>,
}

/// Queue state of one generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueueCounts {
    pub chunks: u64,
    pub embedded: u64,
    pub failed: u64,
}

impl QueueCounts {
    #[must_use]
    pub fn pending(&self) -> u64 {
        self.chunks.saturating_sub(self.embedded + self.failed)
    }
}

/// A chunk's owner and the start of its text (semantic results, T205).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    pub chunk_id: i64,
    pub item_id: i64,
    pub excerpt: String,
    pub kind: String,
    pub symbol: Option<String>,
    pub language: Option<String>,
    pub repository: Option<String>,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
}

/// Content state over all file items (progress UI, diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentCounts {
    pub indexed: u64,
    pub skipped: u64,
    pub failed: u64,
}

impl Store {
    /// Compare an ambiguous rename/write against the indexed representation, without
    /// loading vectors or replacing chunks. Only bounded extraction output is supplied.
    /// # Errors
    /// SQLite failure.
    pub fn content_matches(
        &self,
        item_id: i64,
        chunks: &[NewChunk<'_>],
        version: u32,
    ) -> Result<bool> {
        let indexed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1
            AND content_state = 'indexed' AND extractor_version = ?2)",
            params![item_id, version],
            |r| r.get(0),
        )?;
        if !indexed {
            return Ok(false);
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT ordinal, chunk_kind, text, symbol_name,
            page_number, start_offset, end_offset FROM chunks WHERE item_id = ?1 ORDER BY ordinal",
        )?;
        let mut rows = stmt.query([item_id])?;
        for chunk in chunks {
            let Some(row) = rows.next()? else {
                return Ok(false);
            };
            if row.get::<_, i64>(0)? != chunk.ordinal
                || row.get::<_, String>(1)? != chunk.chunk_kind
                || row.get::<_, String>(2)? != chunk.text
                || row.get::<_, Option<String>>(3)?.as_deref() != chunk.symbol_name
                || row.get::<_, Option<i64>>(4)? != chunk.page_number
                || row.get::<_, Option<i64>>(5)? != chunk.start_offset
                || row.get::<_, Option<i64>>(6)? != chunk.end_offset
            {
                return Ok(false);
            }
        }
        Ok(rows.next()?.is_none())
    }
    /// Code files needing metadata backfill or refresh after a move/content write.
    /// This does not replace chunks or invalidate vectors.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn code_candidates(&self, after_id: i64, limit: usize) -> Result<Vec<ContentCandidate>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, canonical_path, raw_path, display_name, extension, size_bytes, modified_at
             FROM items WHERE id > ?1 AND source = 'files' AND kind = 'file'
               AND status <> 'error' AND (attributes & 4) = 0
               AND code_context_path IS NOT canonical_path
               AND EXISTS (SELECT 1 FROM chunks WHERE item_id = items.id AND chunk_kind = 'code')
             ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![after_id, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| {
                Ok(ContentCandidate {
                    item_id: r.get(0)?,
                    path: r.get(1)?,
                    raw_path: r.get(2)?,
                    name: r.get(3)?,
                    extension: r.get(4)?,
                    size_bytes: r.get(5)?,
                    modified_at: r.get(6)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Stores locally discovered code context; a concurrent move makes the write a no-op.
    /// The context trigger updates FTS, preserving every chunk/vector identity.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn set_code_context(
        &self,
        item_id: i64,
        path: &str,
        language: &str,
        repository: Option<&str>,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "UPDATE items SET code_language = ?3, repository_path = ?4, code_context_path = ?2
             WHERE id = ?1 AND canonical_path = ?2",
            )?
            .execute(params![item_id, path, language, repository])?;
        Ok(())
    }

    /// Up to `limit` text files with `id > after_id` (ascending) that need their content
    /// (re)processed: never processed, failed, changed since (`size:mtime`), or processed
    /// by an extractor older than `extractor_version`. Only `extensions` (lowercase, without
    /// the dot) are considered; items whose metadata failed and cloud placeholders (reading
    /// them would download the file, ADR-018) are left out.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn content_candidates(
        &self,
        after_id: i64,
        extensions: &[&str],
        extractor_version: u32,
        limit: usize,
    ) -> Result<Vec<ContentCandidate>> {
        let extensions = json_list(extensions)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, canonical_path, raw_path, display_name, extension, size_bytes,
                    modified_at
             FROM items
             WHERE id > ?1 AND source = 'files' AND kind = 'file' AND status <> 'error'
               AND (attributes & 4) = 0
               AND lower(extension) IN (SELECT value FROM json_each(?2))
               AND (content_state IS NULL OR content_state = 'failed'
                    OR extractor_version IS NOT ?3
                    OR content_fingerprint IS NOT
                       (coalesce(size_bytes, '') || ':' || coalesce(modified_at, '')))
             ORDER BY id LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                after_id,
                extensions,
                extractor_version,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| {
                Ok(ContentCandidate {
                    item_id: r.get(0)?,
                    path: r.get(1)?,
                    raw_path: r.get(2)?,
                    name: r.get(3)?,
                    extension: r.get(4)?,
                    size_bytes: r.get(5)?,
                    modified_at: r.get(6)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Applies content-pass results in one transaction. `Indexed` replaces the item's
    /// chunks (their vectors go with them, by cascade); `Skipped` removes them; `Failed`
    /// keeps whatever was indexed before. Items deleted meanwhile are ignored.
    ///
    /// # Errors
    /// SQLite failure (the whole batch rolls back).
    pub fn write_content(
        &mut self,
        writes: &[ContentWrite<'_>],
        extractor_version: u32,
        now_ms: i64,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut delete = tx.prepare_cached("DELETE FROM chunks WHERE item_id = ?1")?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO chunks (item_id, ordinal, chunk_kind, text, symbol_name,
                                     page_number, start_offset, end_offset, search_context)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                    CASE WHEN ?3 = 'code' THEN (SELECT name_parts || ' ' || path_parts || ' ' ||
                        display_name || ' ' || coalesce(code_language, '') FROM items WHERE id = ?1)
                    ELSE '' END)",
            )?;
            let mut state = tx.prepare_cached(
                "UPDATE items SET content_state = ?2, content_error = ?3,
                    extractor_version = ?4, content_fingerprint = ?5, indexed_at = ?6
                    , code_context_path = CASE WHEN ?2 = 'indexed' THEN NULL ELSE code_context_path END
                 WHERE id = ?1",
            )?;
            let mut exists = tx.prepare_cached("SELECT 1 FROM items WHERE id = ?1")?;
            for w in writes {
                if exists
                    .query_row([w.item_id], |_| Ok(()))
                    .optional()?
                    .is_none()
                {
                    continue;
                }
                let (code, error) = match &w.outcome {
                    ContentOutcome::Indexed(chunks) => {
                        delete.execute([w.item_id])?;
                        for c in chunks {
                            insert.execute(params![
                                w.item_id,
                                c.ordinal,
                                c.chunk_kind,
                                c.text,
                                c.symbol_name,
                                c.page_number,
                                c.start_offset,
                                c.end_offset
                            ])?;
                        }
                        ("indexed", None)
                    }
                    ContentOutcome::Skipped(reason) => {
                        delete.execute([w.item_id])?;
                        ("skipped", Some(*reason))
                    }
                    ContentOutcome::Failed(reason) => ("failed", Some(*reason)),
                };
                state.execute(params![
                    w.item_id,
                    code,
                    error,
                    extractor_version,
                    w.fingerprint,
                    now_ms
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Content state counts over file items.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn content_counts(&self) -> Result<ContentCounts> {
        let mut counts = ContentCounts::default();
        let mut stmt = self.conn.prepare_cached(
            "SELECT content_state, count(*) FROM items
             WHERE content_state IS NOT NULL GROUP BY content_state",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (state, n) = row?;
            let n = u64::try_from(n).unwrap_or(0);
            match state.as_str() {
                "indexed" => counts.indexed = n,
                "skipped" => counts.skipped = n,
                _ => counts.failed = n,
            }
        }
        Ok(counts)
    }

    /// The generation for `spec`, created (`building`) if new.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn ensure_generation(&self, spec: GenerationSpec<'_>, now_ms: i64) -> Result<i64> {
        let dim = i64::try_from(spec.dim).map_err(|_| StorageError::Corrupt("dim".into()))?;
        self.conn
            .prepare_cached(
                "INSERT INTO generations (space_key, chunker_version, dim, scalar, created_at)
                 VALUES (?1, ?2, ?3, 'f16', ?4)
                 ON CONFLICT (space_key, chunker_version) DO NOTHING",
            )?
            .execute(params![spec.space_key, spec.chunker_version, dim, now_ms])?;
        let (id, stored_dim): (i64, i64) = self
            .conn
            .prepare_cached(
                "SELECT id, dim FROM generations WHERE space_key = ?1 AND chunker_version = ?2",
            )?
            .query_row(params![spec.space_key, spec.chunker_version], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if stored_dim != dim {
            return Err(StorageError::Corrupt(format!(
                "generation {id} has dim {stored_dim}, expected {dim}"
            )));
        }
        Ok(id)
    }

    /// Up to `limit` chunks with `chunk_id > after_chunk_id` (ascending) that have no
    /// result in `generation` yet.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn pending_chunks(
        &self,
        generation: i64,
        after_chunk_id: i64,
        limit: usize,
    ) -> Result<Vec<PendingChunk>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT c.id, c.item_id, c.text, i.display_name
             FROM chunks c JOIN items i ON i.id = c.item_id
             WHERE c.id > ?2
               AND NOT EXISTS (SELECT 1 FROM chunk_vectors v
                               WHERE v.generation = ?1 AND v.chunk_id = c.id)
             ORDER BY c.id LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                generation,
                after_chunk_id,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| {
                Ok(PendingChunk {
                    chunk_id: r.get(0)?,
                    item_id: r.get(1)?,
                    text: r.get(2)?,
                    title: r.get(3)?,
                })
            },
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Stores embedding results for `generation` in one transaction; returns how many
    /// were written. Chunks deleted since they were read are skipped. Each row gets the
    /// generation's next write sequence number (ADR-031: what an ANN file contains).
    ///
    /// # Errors
    /// A vector whose length is not the generation's dimension; SQLite failure.
    pub fn write_vectors(
        &mut self,
        generation: i64,
        writes: &[VectorWrite<'_>],
        now_ms: i64,
    ) -> Result<usize> {
        let dim: i64 = self
            .conn
            .prepare_cached("SELECT dim FROM generations WHERE id = ?1")?
            .query_row([generation], |r| r.get(0))
            .optional()?
            .ok_or_else(|| StorageError::Corrupt(format!("no generation {generation}")))?;
        let dim = usize::try_from(dim).unwrap_or(0);
        let tx = self.conn.transaction()?;
        let mut written = 0;
        let mut seq: i64 = tx
            .prepare_cached("SELECT next_seq FROM generations WHERE id = ?1")?
            .query_row([generation], |r| r.get(0))?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO chunk_vectors
                     (chunk_id, generation, vector, error_code, embedded_at, seq)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6 WHERE EXISTS (SELECT 1 FROM chunks WHERE id = ?1)",
            )?;
            for w in writes {
                let (blob, error) = match w.result {
                    Ok(v) => {
                        if v.len() != dim {
                            return Err(StorageError::Corrupt(format!(
                                "vector of {} dims for a {dim}-dim generation",
                                v.len()
                            )));
                        }
                        (Some(encode_f16(v)), None)
                    }
                    Err(code) => (None, Some(code)),
                };
                let n = stmt.execute(params![w.chunk_id, generation, blob, error, now_ms, seq])?;
                written += n;
                if n > 0 {
                    seq += 1;
                }
            }
        }
        tx.prepare_cached("UPDATE generations SET next_seq = ?2 WHERE id = ?1")?
            .execute(params![generation, seq])?;
        tx.commit()?;
        Ok(written)
    }

    /// Owner item and the first `excerpt_chars` characters of each chunk that still exists,
    /// in the order given.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn chunk_refs(&self, chunk_ids: &[i64], excerpt_chars: usize) -> Result<Vec<ChunkRef>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT c.item_id, substr(c.text, 1, ?2), c.chunk_kind, c.symbol_name,
                    i.code_language, i.repository_path, c.start_offset, c.end_offset
             FROM chunks c JOIN items i ON i.id = c.item_id WHERE c.id = ?1",
        )?;
        let n = i64::try_from(excerpt_chars).unwrap_or(i64::MAX);
        let mut out = Vec::with_capacity(chunk_ids.len());
        for &chunk_id in chunk_ids {
            if let Some(reference) = stmt
                .query_row(params![chunk_id, n], |r| {
                    Ok(ChunkRef {
                        chunk_id,
                        item_id: r.get(0)?,
                        excerpt: r.get(1)?,
                        kind: r.get(2)?,
                        symbol: r.get(3)?,
                        language: r.get(4)?,
                        repository: r.get(5)?,
                        start_offset: r.get(6)?,
                        end_offset: r.get(7)?,
                    })
                })
                .optional()?
            {
                out.push(reference);
            }
        }
        Ok(out)
    }

    /// Chunk totals and results for `generation`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn queue_counts(&self, generation: i64) -> Result<QueueCounts> {
        let count = |n: i64| u64::try_from(n).unwrap_or(0);
        let chunks: i64 = self
            .conn
            .prepare_cached("SELECT count(*) FROM chunks")?
            .query_row([], |r| r.get(0))?;
        let (embedded, failed): (i64, i64) = self
            .conn
            .prepare_cached(
                "SELECT count(vector), count(error_code) FROM chunk_vectors WHERE generation = ?1",
            )?
            .query_row([generation], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(QueueCounts {
            chunks: count(chunks),
            embedded: count(embedded),
            failed: count(failed),
        })
    }

    /// Up to `limit` stored vectors of `generation` with `chunk_id > after_chunk_id`
    /// (ascending): what an ANN build reads (T203).
    ///
    /// # Errors
    /// SQLite failure or a malformed blob.
    pub fn vectors(
        &self,
        generation: i64,
        after_chunk_id: i64,
        limit: usize,
    ) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT chunk_id, vector FROM chunk_vectors
             WHERE generation = ?1 AND chunk_id > ?2 AND vector IS NOT NULL
             ORDER BY chunk_id LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                generation,
                after_chunk_id,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            out.push((
                id,
                decode_f16(&blob)
                    .ok_or_else(|| StorageError::Corrupt(format!("vector blob of chunk {id}")))?,
            ));
        }
        Ok(out)
    }
}

/// `["md","rs"]` for `json_each` (extensions are plain ASCII words; anything else is a bug).
fn json_list(words: &[&str]) -> Result<String> {
    let mut out = String::from("[");
    for (i, w) in words.iter().enumerate() {
        if !w
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(StorageError::Corrupt(format!("extension `{w}`")));
        }
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(w);
        out.push('"');
    }
    out.push(']');
    Ok(out)
}

/// Little-endian IEEE 754 binary16, round to nearest even.
#[must_use]
pub fn encode_f16(v: &[f32]) -> Vec<u8> {
    v.iter()
        .flat_map(|&x| f32_to_f16(x).to_le_bytes())
        .collect()
}

/// Inverse of [`encode_f16`]; `None` for an odd byte count.
#[must_use]
pub fn decode_f16(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(2) {
        return None;
    }
    Some(
        blob.chunks_exact(2)
            .map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]])))
            .collect(),
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant == 0 { 0 } else { 0x0200 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    let round = |value: u32, rem: u32, halfway: u32| {
        if rem > halfway || (rem == halfway && value & 1 == 1) {
            value + 1
        } else {
            value
        }
    };
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mant | 0x0080_0000;
        let shift = (14 - e) as u32;
        let value = m >> shift;
        return sign | round(value, m & ((1 << shift) - 1), 1 << (shift - 1)) as u16;
    }
    // A carry out of the mantissa correctly bumps the exponent (up to infinity).
    let value = ((e as u32) << 10) | (mant >> 13);
    sign | round(value, mant & 0x1fff, 0x1000) as u16
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exp = u32::from((h >> 10) & 0x1f);
    let mant = u32::from(h & 0x03ff);
    let bits = match exp {
        0 if mant == 0 => sign,
        0 => {
            // Subnormal: mant * 2^-24.
            #[allow(clippy::cast_precision_loss)]
            let v = mant as f32 / 16_777_216.0;
            return if sign == 0 { v } else { -v };
        }
        0x1f => sign | 0x7f80_0000 | (mant << 13),
        _ => sign | ((exp + 112) << 23) | (mant << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ItemKind;
    use crate::catalog::{CatalogEntry, Source};

    #[test]
    fn f16_matches_ieee_binary16() {
        for (x, h) in [
            (0.0_f32, 0x0000_u16),
            (-0.0, 0x8000),
            (1.0, 0x3c00),
            (-2.0, 0xc000),
            (0.1, 0x2e66),
            (0.333_333_34, 0x3555),
            (65504.0, 0x7bff),
            (65520.0, 0x7c00),
            (f32::INFINITY, 0x7c00),
            (2.0_f32.powi(-24), 0x0001),
            (1023.0 * 2.0_f32.powi(-24), 0x03ff),
            (2.0_f32.powi(-14), 0x0400),
        ] {
            assert_eq!(f32_to_f16(x), h, "{x}");
        }
        assert!(f16_to_f32(0x7e00).is_nan() && f32_to_f16(f32::NAN) & 0x7c00 == 0x7c00);
        // Round trip error on typical normalized-embedding values is below 2^-11 relative.
        for i in 0..2000 {
            #[allow(clippy::cast_precision_loss)]
            let x = (i as f32 - 1000.0) / 3217.0;
            let y = f16_to_f32(f32_to_f16(x));
            assert!((x - y).abs() <= x.abs() / 2048.0 + 1e-7, "{x} -> {y}");
        }
        let blob = encode_f16(&[1.0, -0.5]);
        assert_eq!(blob, vec![0x00, 0x3c, 0x00, 0xb8]);
        assert_eq!(decode_f16(&blob), Some(vec![1.0, -0.5]));
        assert_eq!(decode_f16(&[1]), None);
    }

    fn entry<'a>(path: &'a str, name: &'a str, ext: &'a str, size: i64) -> CatalogEntry<'a> {
        CatalogEntry {
            kind: ItemKind::File,
            source: Source::Files,
            path,
            raw_path: None,
            name,
            name_key: name,
            name_parts: name,
            path_parts: "",
            extension: Some(ext),
            volume_id: None,
            file_id: None,
            launch_target: None,
            attributes: 0,
            size_bytes: Some(size),
            modified_at: Some(1_000),
            created_at: None,
            error: None,
        }
    }

    fn chunk(text: &str, ordinal: i64) -> NewChunk<'_> {
        NewChunk {
            item_id: 0,
            ordinal,
            chunk_kind: "text",
            text,
            symbol_name: None,
            page_number: None,
            start_offset: Some(0),
            end_offset: Some(i64::try_from(text.len()).unwrap()),
        }
    }

    #[test]
    fn content_pass_state_and_embedding_queue_round_trip() {
        let db = crate::tests::TempDb::new("content");
        let mut store = Store::open_writer(&db.path()).unwrap();
        let scan = store.begin_scan(Source::Files).unwrap();
        store
            .upsert_entries(
                scan,
                &[
                    entry(r"C:\d\notes.md", "notes.md", "md", 10),
                    entry(r"C:\d\photo.jpg", "photo.jpg", "jpg", 10),
                    entry(r"C:\d\Main.RS", "Main.RS", "RS", 20),
                    entry(r"C:\d\big.log", "big.log", "log", 30),
                ],
            )
            .unwrap();
        let exts = ["md", "rs", "log"];
        let all = store.content_candidates(0, &exts, 1, 10).unwrap();
        let names: Vec<_> = all.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["notes.md", "Main.RS", "big.log"]);
        // Keyset paging.
        let page = store
            .content_candidates(all[0].item_id, &exts, 1, 1)
            .unwrap();
        assert_eq!(page[0].name, "Main.RS");

        let fps: Vec<String> = all.iter().map(ContentCandidate::fingerprint).collect();
        assert_eq!(fps[0], "10:1000");
        store
            .write_content(
                &[
                    ContentWrite {
                        item_id: all[0].item_id,
                        fingerprint: &fps[0],
                        outcome: ContentOutcome::Indexed(vec![chunk("uno", 0), chunk("dos", 1)]),
                    },
                    ContentWrite {
                        item_id: all[1].item_id,
                        fingerprint: &fps[1],
                        outcome: ContentOutcome::Failed("io:PermissionDenied"),
                    },
                    ContentWrite {
                        item_id: all[2].item_id,
                        fingerprint: &fps[2],
                        outcome: ContentOutcome::Skipped("too_large"),
                    },
                    ContentWrite {
                        item_id: 9_999,
                        fingerprint: "",
                        outcome: ContentOutcome::Skipped("binary"),
                    },
                ],
                1,
                5,
            )
            .unwrap();
        // Only the failed file comes back; a newer extractor re-reads everything.
        let again = store.content_candidates(0, &exts, 1, 10).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].name, "Main.RS");
        assert_eq!(store.content_candidates(0, &exts, 2, 10).unwrap().len(), 3);
        assert_eq!(
            store.content_counts().unwrap(),
            ContentCounts {
                indexed: 1,
                skipped: 1,
                failed: 1
            }
        );

        // Embedding queue.
        let spec = GenerationSpec {
            space_key: "test-space",
            chunker_version: 1,
            dim: 2,
        };
        let generation = store.ensure_generation(spec, 1).unwrap();
        assert_eq!(store.ensure_generation(spec, 2).unwrap(), generation);
        assert!(
            store
                .ensure_generation(GenerationSpec { dim: 3, ..spec }, 3)
                .is_err()
        );
        let pending = store.pending_chunks(generation, 0, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(
            (pending[0].text.as_str(), pending[0].title.as_str()),
            ("uno", "notes.md")
        );
        let written = store
            .write_vectors(
                generation,
                &[
                    VectorWrite {
                        chunk_id: pending[0].chunk_id,
                        result: Ok(&[0.6, 0.8]),
                    },
                    VectorWrite {
                        chunk_id: pending[1].chunk_id,
                        result: Err("empty"),
                    },
                    VectorWrite {
                        chunk_id: 777,
                        result: Ok(&[1.0, 0.0]),
                    },
                ],
                9,
            )
            .unwrap();
        assert_eq!(written, 2);
        assert!(
            store
                .write_vectors(
                    generation,
                    &[VectorWrite {
                        chunk_id: pending[0].chunk_id,
                        result: Ok(&[1.0]),
                    }],
                    9
                )
                .is_err()
        );
        assert!(store.pending_chunks(generation, 0, 10).unwrap().is_empty());
        let counts = store.queue_counts(generation).unwrap();
        assert_eq!(
            (
                counts.chunks,
                counts.embedded,
                counts.failed,
                counts.pending()
            ),
            (2, 1, 1, 0)
        );
        let vectors = store.vectors(generation, 0, 10).unwrap();
        assert_eq!(vectors.len(), 1);
        assert!((vectors[0].1[0] - 0.6).abs() < 1e-3 && (vectors[0].1[1] - 0.8).abs() < 1e-3);

        // Re-indexing a changed file replaces chunks and drops their vectors.
        let scan = store.begin_scan(Source::Files).unwrap();
        store
            .upsert_entries(scan, &[entry(r"C:\d\notes.md", "notes.md", "md", 11)])
            .unwrap();
        let changed = store.content_candidates(0, &exts, 1, 10).unwrap();
        assert_eq!(changed[0].name, "notes.md");
        let fp = changed[0].fingerprint();
        store
            .write_content(
                &[ContentWrite {
                    item_id: changed[0].item_id,
                    fingerprint: &fp,
                    outcome: ContentOutcome::Indexed(vec![chunk("tres", 0)]),
                }],
                1,
                10,
            )
            .unwrap();
        let counts = store.queue_counts(generation).unwrap();
        assert_eq!(
            (
                counts.chunks,
                counts.embedded,
                counts.failed,
                counts.pending()
            ),
            (1, 0, 0, 1)
        );
        // Deleting the item cascades to its chunks and vectors.
        let id = store.item_id_by_path(r"C:\d\notes.md").unwrap().unwrap();
        let p = store.pending_chunks(generation, 0, 1).unwrap();
        store
            .write_vectors(
                generation,
                &[VectorWrite {
                    chunk_id: p[0].chunk_id,
                    result: Ok(&[1.0, 0.0]),
                }],
                11,
            )
            .unwrap();
        assert!(store.delete_item(id).unwrap());
        assert_eq!(
            store.queue_counts(generation).unwrap(),
            QueueCounts::default()
        );
        assert!(store.content_candidates(0, &["m\"d"], 1, 1).is_err());
    }
}
