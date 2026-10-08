//! `lumen-bench storage`: SQLite/FTS5 bulk insert throughput, lexical query latency and size
//! (T007, docs/PERFORMANCE.md §8 budget: lexical update < 16 ms p50 per keystroke).

use std::path::PathBuf;
use std::time::Instant;

use std::time::Duration;

use lumen_storage::{FtsQuery, NewChunk, NewItem, SearchBudget, StorageError, Store};
use serde::Serialize;

use crate::corpus;
use crate::machine::MachineInfo;
use crate::stats::Summary;

#[derive(Debug, Clone)]
pub(crate) struct StorageOptions {
    pub(crate) chunks: usize,
    pub(crate) chunks_per_item: usize,
    pub(crate) words: usize,
    pub(crate) batch: usize,
    pub(crate) work_dir: PathBuf,
    pub(crate) label: Option<String>,
}

impl Default for StorageOptions {
    fn default() -> Self {
        Self {
            chunks: 100_000,
            chunks_per_item: 10,
            words: 120,
            batch: 1000,
            work_dir: std::env::temp_dir().join("lumen-bench-storage"),
            label: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct StorageReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    sqlite: String,
    chunks: usize,
    items: usize,
    words_per_chunk: usize,
    insert_s: f64,
    chunks_per_s: f64,
    db_mib_after_checkpoint: f64,
    vocabulary_words: usize,
    zipf_exponent: f64,
    fts_typing: Summary,
    /// Mean hits per keystroke query (capped at 50).
    fts_typing_mean_hits: f64,
    /// Final (not typing) queries over vocabulary terms.
    fts_final: Summary,
    fts_mean_hits: f64,
    /// Final realistic queries (`corpus::QUERIES`), whose terms occur in the Zipf corpus.
    fts_final_realistic: Summary,
    fts_final_realistic_mean_hits: f64,
    /// Realistic final queries with no hit at all (multi-term AND over rare terms).
    fts_final_realistic_zero_hit: usize,
    /// Per-keystroke queries stopped by the interactive budget (`budget_ms`).
    fts_typing_budgeted: Summary,
    fts_typing_interrupted: usize,
    budget_ms: f64,
    path_lookup: Summary,
}

/// Word-frequency skew of the synthetic corpus (classic Zipf).
const ZIPF_EXPONENT: f64 = 1.0;

/// Interactive budget for one keystroke's lexical query (PERFORMANCE.md §2: 16/40 ms).
const BUDGET: Duration = Duration::from_millis(20);

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// # Errors
/// Storage failures as messages.
pub(crate) fn run(opts: &StorageOptions) -> Result<StorageReport, String> {
    let _ = std::fs::remove_dir_all(&opts.work_dir);
    std::fs::create_dir_all(&opts.work_dir).map_err(|e| e.to_string())?;
    let path = opts.work_dir.join("bench.db");
    let mut store = Store::open_writer(&path).map_err(|e| e.to_string())?;
    let items = opts.chunks.div_ceil(opts.chunks_per_item.max(1));

    let started = Instant::now();
    let mut item_ids = Vec::with_capacity(items);
    let mut paths = Vec::with_capacity(items);
    for i in 0..items {
        let p = format!(r"C:\Users\bench\Documents\project-{}\file-{i}.md", i % 97);
        let name = format!("file-{i}.md");
        item_ids.push(
            store
                .insert_item(&NewItem::file(&p, &name))
                .map_err(|e| e.to_string())?,
        );
        paths.push(p);
    }
    let zipf = corpus::ZipfCorpus::new(ZIPF_EXPONENT);
    let texts: Vec<String> = (0..opts.chunks)
        .map(|i| zipf.document(i as u64 + 7, opts.words))
        .collect();
    for (b, batch) in texts.chunks(opts.batch.max(1)).enumerate() {
        let chunks: Vec<NewChunk<'_>> = batch
            .iter()
            .enumerate()
            .map(|(j, text)| {
                let global = b * opts.batch + j;
                NewChunk {
                    item_id: item_ids[global / opts.chunks_per_item],
                    ordinal: (global % opts.chunks_per_item) as i64,
                    chunk_kind: "text",
                    text,
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                }
            })
            .collect();
        store.insert_chunks(&chunks).map_err(|e| e.to_string())?;
    }
    let insert_s = started.elapsed().as_secs_f64();
    store.checkpoint().map_err(|e| e.to_string())?;
    #[allow(clippy::cast_precision_loss)]
    let db_mib = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);

    let reader = Store::open_reader(&path).map_err(|e| e.to_string())?;
    let mut typing = Vec::new();
    let mut typing_hits = 0_usize;
    let mut finals = Vec::new();
    let mut hits = 0_usize;
    let mut realistic = Vec::new();
    let mut realistic_hits = 0_usize;
    let mut realistic_zero = 0_usize;
    let mut budgeted = Vec::new();
    let mut interrupted = 0_usize;
    for q in corpus::QUERIES {
        // Every keystroke prefix of the query, as the root search would issue them.
        for end in (1..=q.len()).filter(|&e| q.is_char_boundary(e)) {
            if let Some(fq) = FtsQuery::from_user(&q[..end], true) {
                let t = Instant::now();
                typing_hits += reader
                    .search_chunks(&fq, 50, &SearchBudget::unbounded())
                    .map_err(|e| e.to_string())?
                    .len();
                typing.push(ms(t));
                let t = Instant::now();
                match reader.search_chunks(&fq, 50, &SearchBudget::within(BUDGET)) {
                    Ok(_) => {}
                    Err(StorageError::Interrupted) => interrupted += 1,
                    Err(e) => return Err(e.to_string()),
                }
                budgeted.push(ms(t));
            }
        }
        if let Some(fq) = FtsQuery::from_user(q, false) {
            let t = Instant::now();
            let n = reader
                .search_chunks(&fq, 50, &SearchBudget::unbounded())
                .map_err(|e| e.to_string())?
                .len();
            realistic.push(ms(t));
            realistic_hits += n;
            realistic_zero += usize::from(n == 0);
        }
    }
    // Final queries that do match the synthetic corpus: ranking + snippets on real hits.
    for q in corpus::vocabulary_queries(60) {
        if let Some(fq) = FtsQuery::from_user(&q, false) {
            let t = Instant::now();
            hits += reader
                .search_chunks(&fq, 50, &SearchBudget::unbounded())
                .map_err(|e| e.to_string())?
                .len();
            finals.push(ms(t));
        }
    }
    let mut lookups = Vec::new();
    for p in paths.iter().step_by((items / 500).max(1)) {
        let t = Instant::now();
        reader.item_id_by_path(p).map_err(|e| e.to_string())?;
        lookups.push(ms(t));
    }
    let sqlite: String = reader
        .connection()
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    drop(reader);
    drop(store);
    let _ = std::fs::remove_dir_all(&opts.work_dir);

    #[allow(clippy::cast_precision_loss)]
    Ok(StorageReport {
        schema_version: 3,
        kind: "storage",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        sqlite,
        chunks: opts.chunks,
        items,
        words_per_chunk: opts.words,
        vocabulary_words: zipf.vocabulary_size(),
        zipf_exponent: ZIPF_EXPONENT,
        insert_s,
        chunks_per_s: opts.chunks as f64 / insert_s.max(1e-9),
        db_mib_after_checkpoint: db_mib,
        fts_typing: Summary::of(&typing).ok_or("no queries")?,
        fts_typing_mean_hits: typing_hits as f64 / typing.len().max(1) as f64,
        fts_final: Summary::of(&finals).ok_or("no queries")?,
        fts_mean_hits: hits as f64 / finals.len().max(1) as f64,
        fts_final_realistic: Summary::of(&realistic).ok_or("no queries")?,
        fts_final_realistic_mean_hits: realistic_hits as f64 / realistic.len().max(1) as f64,
        fts_final_realistic_zero_hit: realistic_zero,
        fts_typing_budgeted: Summary::of(&budgeted).ok_or("no queries")?,
        fts_typing_interrupted: interrupted,
        budget_ms: BUDGET.as_secs_f64() * 1000.0,
        path_lookup: Summary::of(&lookups).ok_or("no lookups")?,
    })
}

pub(crate) fn summarize(r: &StorageReport) -> String {
    let mut warnings = String::new();
    for (what, mean) in [
        ("per-keystroke", r.fts_typing_mean_hits),
        ("vocabulary final", r.fts_mean_hits),
        ("realistic final", r.fts_final_realistic_mean_hits),
    ] {
        if mean <= 0.0 {
            warnings.push_str(&format!(
                "\n  WARNING: {what} queries returned no hits: their latency is not representative"
            ));
        }
    }
    format!(
        "storage · sqlite {} · {} chunks ({} items, {} words, Zipf {} over {} words) · insert {:.0} chunks/s · db {:.0} MiB\n  \
         fts per keystroke (n={}): p50 {:.3} · p95 {:.3} · max {:.3} ms ({:.1} hits avg)\n  \
         fts per keystroke with {:.0} ms budget: p95 {:.3} · max {:.3} ms ({} interrupted)\n  \
         fts final, vocabulary: p50 {:.3} · p95 {:.3} ms ({:.1} hits avg, capped 50)\n  \
         fts final, realistic: p50 {:.3} · p95 {:.3} ms ({:.1} hits avg, {} of {} with none)\n  \
         path lookup: p50 {:.4} · p95 {:.4} ms{}{}\n",
        r.sqlite,
        r.chunks,
        r.items,
        r.words_per_chunk,
        r.zipf_exponent,
        r.vocabulary_words,
        r.chunks_per_s,
        r.db_mib_after_checkpoint,
        r.fts_typing.n,
        r.fts_typing.p50_ms,
        r.fts_typing.p95_ms,
        r.fts_typing.max_ms,
        r.fts_typing_mean_hits,
        r.budget_ms,
        r.fts_typing_budgeted.p95_ms,
        r.fts_typing_budgeted.max_ms,
        r.fts_typing_interrupted,
        r.fts_final.p50_ms,
        r.fts_final.p95_ms,
        r.fts_mean_hits,
        r.fts_final_realistic.p50_ms,
        r.fts_final_realistic.p95_ms,
        r.fts_final_realistic_mean_hits,
        r.fts_final_realistic_zero_hit,
        r.fts_final_realistic.n,
        r.path_lookup.p50_ms,
        r.path_lookup.p95_ms,
        warnings,
        if r.machine.build_profile == "release" {
            ""
        } else {
            "\n  WARNING: debug build — not acceptance evidence"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_run_completes() {
        let opts = StorageOptions {
            chunks: 300,
            batch: 64,
            work_dir: std::env::temp_dir()
                .join(format!("lumen-bench-storage-test-{}", std::process::id())),
            ..StorageOptions::default()
        };
        let r = run(&opts).unwrap();
        assert_eq!(r.items, 30);
        assert!(r.fts_typing.n > 100);
        assert!(
            r.fts_mean_hits > 1.0,
            "final queries must hit: {}",
            r.fts_mean_hits
        );
        assert!(r.fts_typing_mean_hits > 1.0, "{}", r.fts_typing_mean_hits);
        assert!(r.fts_final_realistic_mean_hits > 0.0);
        assert!(!summarize(&r).contains("WARNING: per-keystroke"));
        assert!(summarize(&r).contains("chunks/s"));
    }
}
