//! `lumen-bench chunk` (T201): extraction + chunking over real folders — outcomes per file,
//! throughput, chunk sizes, and (with `--tokenizer`, feature `tokenizer`) how the token
//! estimate compares with the embedding model's tokenizer.
//!
//! Privacy: counts and distributions only; no path, name or text is written.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use lumen_catalog::IndexLocations;
use lumen_extract::{
    ChunkConfig, DEFAULT_MAX_BYTES, DocKind, EstimateTokens, ExtractError, Skip, chunk,
    extract_file,
};
use lumen_indexer::{EntryKind, scan};
use serde::Serialize;

use crate::machine::MachineInfo;
use crate::stats::Summary;

#[derive(Debug, Clone, Default)]
pub(crate) struct ChunkOptions {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) tokenizer: Option<PathBuf>,
    pub(crate) target_tokens: Option<usize>,
    pub(crate) label: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub(crate) struct Outcomes {
    extracted: u64,
    unsupported: u64,
    too_large: u64,
    binary: u64,
    read_failed: u64,
    lossy_decoding: u64,
    legacy_encoding: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct TokenizerCheck {
    /// Real tokens per chunk (model tokenizer, no prompt).
    real_tokens: Summary,
    /// estimate / real, per chunk.
    estimate_ratio: Summary,
    /// Chunks whose real count exceeds `max_tokens`.
    over_max: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChunkReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    target_tokens: usize,
    max_tokens: usize,
    files: Outcomes,
    /// Extracted files by document kind (`prose`, `markdown`, `code`, `data`).
    by_kind: BTreeMap<&'static str, u64>,
    extracted_mib: f64,
    seconds: f64,
    mib_per_s: f64,
    chunks: u64,
    chunks_per_file: Summary,
    /// Estimated tokens per chunk.
    estimated_tokens: Summary,
    /// Share of code chunks with a symbol name.
    code_symbol_share: f64,
    tokenizer: Option<TokenizerCheck>,
}

#[cfg(feature = "tokenizer")]
fn real_counter(path: &std::path::Path) -> Result<impl Fn(&str) -> usize, String> {
    let t = tokenizers::Tokenizer::from_file(path).map_err(|e| e.to_string())?;
    Ok(move |s: &str| t.encode(s, false).map_or(0, |e| e.len()))
}

#[cfg(not(feature = "tokenizer"))]
fn real_counter(_path: &std::path::Path) -> Result<fn(&str) -> usize, String> {
    Err("--tokenizer needs `--features tokenizer`".into())
}

/// # Errors
/// No roots, or an unusable tokenizer.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn run(opts: &ChunkOptions) -> Result<ChunkReport, String> {
    if opts.roots.is_empty() {
        return Err("chunk needs at least one --root".into());
    }
    let cfg = ChunkConfig {
        target_tokens: opts.target_tokens.unwrap_or(128),
        max_tokens: opts.target_tokens.map_or(192, |t| t * 3 / 2),
        ..ChunkConfig::default()
    };
    let real = opts.tokenizer.as_deref().map(real_counter).transpose()?;
    let scan_opts = IndexLocations::standard(&opts.roots, 0).scan_options(false);
    let mut files: Vec<PathBuf> = Vec::new();
    scan(
        &scan_opts,
        |e| {
            if e.kind == EntryKind::File {
                files.push(e.path);
            }
        },
        None,
    );

    let started = Instant::now();
    let mut out = Outcomes::default();
    let mut by_kind: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut bytes = 0usize;
    let mut per_file = Vec::new();
    let mut est = Vec::new();
    let mut real_tokens = Vec::new();
    let mut ratio = Vec::new();
    let mut over_max = 0u64;
    let (mut code_chunks, mut code_with_symbol) = (0u64, 0u64);
    for path in &files {
        match extract_file(path, DEFAULT_MAX_BYTES) {
            Ok(doc) => {
                out.extracted += 1;
                out.lossy_decoding += u64::from(doc.lossy);
                out.legacy_encoding += u64::from(doc.encoding == "windows-1252");
                bytes += doc.text.len();
                *by_kind
                    .entry(match doc.kind {
                        DocKind::Prose => "prose",
                        DocKind::Markdown => "markdown",
                        DocKind::Code(_) => "code",
                        DocKind::Data => "data",
                    })
                    .or_insert(0) += 1;
                let chunks = chunk(&doc, &cfg, &EstimateTokens);
                per_file.push(chunks.len() as f64);
                for c in &chunks {
                    est.push(c.tokens as f64);
                    if matches!(doc.kind, DocKind::Code(_)) {
                        code_chunks += 1;
                        code_with_symbol += u64::from(c.symbol.is_some());
                    }
                    if let Some(count) = &real {
                        let n = count(c.text(&doc.text));
                        real_tokens.push(n as f64);
                        if n > 0 {
                            ratio.push(c.tokens as f64 / n as f64);
                        }
                        over_max += u64::from(n > cfg.max_tokens);
                    }
                }
            }
            Err(ExtractError::Skipped(Skip::Unsupported)) => out.unsupported += 1,
            Err(ExtractError::Skipped(Skip::TooLarge(_))) => out.too_large += 1,
            Err(ExtractError::Skipped(Skip::Binary)) => out.binary += 1,
            Err(ExtractError::Io(_)) => out.read_failed += 1,
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let mib = bytes as f64 / (1024.0 * 1024.0);
    let zero = Summary::of(&[0.0]).ok_or("summary")?;
    Ok(ChunkReport {
        schema_version: 1,
        kind: "chunk",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        target_tokens: cfg.target_tokens,
        max_tokens: cfg.max_tokens,
        files: out,
        by_kind,
        extracted_mib: mib,
        seconds,
        mib_per_s: mib / seconds.max(1e-9),
        chunks: est.len() as u64,
        chunks_per_file: Summary::of(&per_file).unwrap_or(zero),
        estimated_tokens: Summary::of(&est).unwrap_or(zero),
        code_symbol_share: code_with_symbol as f64 / code_chunks.max(1) as f64,
        tokenizer: real.map(|_| TokenizerCheck {
            real_tokens: Summary::of(&real_tokens).unwrap_or(zero),
            estimate_ratio: Summary::of(&ratio).unwrap_or(zero),
            over_max,
        }),
    })
}

pub(crate) fn summarize(r: &ChunkReport) -> String {
    let mut s = format!(
        "chunk: {} extracted ({:?}), skipped: {} unsupported, {} too large, {} binary, {} unreadable\n  \
         {:.1} MiB in {:.2} s ({:.1} MiB/s) -> {} chunks (target {} / max {} tokens)\n  \
         chunks per file p50 {:.0} p95 {:.0} | estimated tokens p50 {:.0} p95 {:.0} max {:.0} | code chunks with a symbol {:.0}%\n",
        r.files.extracted,
        r.by_kind,
        r.files.unsupported,
        r.files.too_large,
        r.files.binary,
        r.files.read_failed,
        r.extracted_mib,
        r.seconds,
        r.mib_per_s,
        r.chunks,
        r.target_tokens,
        r.max_tokens,
        r.chunks_per_file.p50_ms,
        r.chunks_per_file.p95_ms,
        r.estimated_tokens.p50_ms,
        r.estimated_tokens.p95_ms,
        r.estimated_tokens.max_ms,
        r.code_symbol_share * 100.0,
    );
    if let Some(t) = &r.tokenizer {
        s.push_str(&format!(
            "  model tokenizer: tokens p50 {:.0} p95 {:.0} max {:.0} | estimate/real p50 {:.2} p5..p95 {:.2}..{:.2} | over max {}\n",
            t.real_tokens.p50_ms,
            t.real_tokens.p95_ms,
            t.real_tokens.max_ms,
            t.estimate_ratio.p50_ms,
            t.estimate_ratio.min_ms,
            t.estimate_ratio.p95_ms,
            t.over_max,
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_a_small_tree_without_names() {
        let dir = std::env::temp_dir().join(format!("lumen-bench-chunk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("secret-notes.md"),
            "# Plan\n\nWrite the chunker.\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn secret_fn() {}\n").unwrap();
        std::fs::write(dir.join("photo.jpg"), [0xFFu8, 0xD8, 0]).unwrap();
        let r = run(&ChunkOptions {
            roots: vec![dir.clone()],
            ..ChunkOptions::default()
        })
        .unwrap();
        assert_eq!(r.files.extracted, 2);
        assert_eq!(r.files.unsupported, 1);
        assert_eq!(r.chunks, 2);
        assert!((r.code_symbol_share - 1.0).abs() < 1e-9);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("secret"));
        assert!(summarize(&r).contains("chunks per file"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
