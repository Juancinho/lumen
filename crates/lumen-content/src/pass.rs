//! The content pass (Pass 1, docs/SEARCH_AND_INDEXING.md §21): text files whose content is
//! new, changed or stale are extracted and chunked, and their chunks replace the old ones.
//! Chunks are immediately searchable lexically (`chunks_fts`) and become pending for the
//! embedding queue. Extraction runs at MB/s, orders of magnitude above embedding (ADR-028),
//! so the pass simply runs to the end of the candidate list.

use std::time::{Duration, Instant};

use lumen_core::CancellationToken;
use lumen_extract::{
    ChunkConfig, DEFAULT_MAX_BYTES, EXTRACTOR_VERSION, ExtractError, Skip, TEXT_EXTENSIONS,
    TokenCount, chunk, extract_file,
};
use lumen_storage::{ContentOutcome, ContentWrite, NewChunk, StorageError, Store};

#[derive(Debug, Clone, Copy)]
pub struct PassConfig {
    /// Files above this size are skipped (`too_large`).
    pub max_bytes: u64,
    /// Files per write transaction.
    pub batch_files: usize,
    pub chunk: ChunkConfig,
}

impl Default for PassConfig {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            batch_files: 32,
            chunk: ChunkConfig::default(),
        }
    }
}

/// Counts and timings only (never paths or text).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassReport {
    /// Files looked at.
    pub files: u64,
    pub indexed: u64,
    pub skipped: u64,
    pub failed: u64,
    pub chunks: u64,
    /// Bytes of extracted text.
    pub text_bytes: u64,
    pub elapsed: Duration,
    pub cancelled: bool,
}

/// Runs the content pass to completion (or cancellation, checked between files). Only
/// items whose stored path satisfies `scope` are read (locations indexed by name only are
/// left alone, ADR-027); `progress` is called after every write batch.
///
/// # Errors
/// Storage failures; per-file problems are recorded on the item instead.
pub fn run_content_pass(
    store: &mut Store,
    cfg: &PassConfig,
    counter: &dyn TokenCount,
    scope: &dyn Fn(&str) -> bool,
    cancel: &CancellationToken,
    now_ms: &dyn Fn() -> i64,
    progress: &mut dyn FnMut(&PassReport),
) -> Result<PassReport, StorageError> {
    let started = Instant::now();
    let mut report = PassReport::default();
    let mut cursor = 0;
    loop {
        let candidates = store.content_candidates(
            cursor,
            TEXT_EXTENSIONS,
            EXTRACTOR_VERSION,
            cfg.batch_files.max(1),
        )?;
        let Some(last) = candidates.last() else {
            break;
        };
        cursor = last.item_id;

        // Extract everything first (owned), then borrow it into one write batch.
        let mut extracted = Vec::with_capacity(candidates.len());
        for c in candidates.iter().filter(|c| scope(&c.path)) {
            if cancel.is_cancelled() {
                break;
            }
            let path = lumen_catalog::path::decode(&c.path, c.raw_path.as_deref());
            let result = extract_file(&path, cfg.max_bytes).map(|doc| {
                let chunks = chunk(&doc, &cfg.chunk, counter);
                (doc, chunks)
            });
            extracted.push((c.item_id, c.fingerprint(), result));
        }
        let writes: Vec<ContentWrite<'_>> = extracted
            .iter()
            .map(|(item_id, fingerprint, result)| ContentWrite {
                item_id: *item_id,
                fingerprint,
                outcome: outcome(result),
            })
            .collect();
        if !writes.is_empty() {
            store.write_content(&writes, EXTRACTOR_VERSION, now_ms())?;
        }

        for (_, _, result) in &extracted {
            report.files += 1;
            match result {
                Ok((doc, chunks)) => {
                    report.indexed += 1;
                    report.chunks += chunks.len() as u64;
                    report.text_bytes += doc.text.len() as u64;
                }
                Err(ExtractError::Skipped(_)) => report.skipped += 1,
                Err(ExtractError::Io(_)) => report.failed += 1,
            }
        }
        report.elapsed = started.elapsed();
        progress(&report);
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
    }
    report.elapsed = started.elapsed();
    Ok(report)
}

type Extraction = Result<(lumen_extract::Extracted, Vec<lumen_extract::Chunk>), ExtractError>;

fn outcome(result: &Extraction) -> ContentOutcome<'_> {
    match result {
        Ok((doc, chunks)) => ContentOutcome::Indexed(
            chunks
                .iter()
                .map(|c| NewChunk {
                    item_id: 0,
                    ordinal: i64::from(c.ordinal),
                    chunk_kind: c.kind.as_str(),
                    text: c.text(&doc.text),
                    symbol_name: c.symbol.as_deref(),
                    page_number: None,
                    start_offset: i64::try_from(c.start).ok(),
                    end_offset: i64::try_from(c.end).ok(),
                })
                .collect(),
        ),
        Err(ExtractError::Skipped(Skip::Binary)) => ContentOutcome::Skipped("binary"),
        Err(ExtractError::Skipped(Skip::TooLarge(_))) => ContentOutcome::Skipped("too_large"),
        // Candidates are filtered by extension, so this only happens if the lists diverge.
        Err(ExtractError::Skipped(Skip::Unsupported)) => ContentOutcome::Skipped("unsupported"),
        Err(ExtractError::Io(kind)) => ContentOutcome::Failed(io_code(*kind)),
    }
}

/// Stable short codes for `items.content_error` (no paths, no OS message text).
fn io_code(kind: std::io::ErrorKind) -> &'static str {
    use std::io::ErrorKind as K;
    match kind {
        K::NotFound => "io:not_found",
        K::PermissionDenied => "io:permission_denied",
        K::Interrupted | K::WouldBlock | K::TimedOut => "io:busy",
        _ => "io:other",
    }
}
