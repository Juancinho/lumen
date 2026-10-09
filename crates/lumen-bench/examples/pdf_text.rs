//! T301 release-only extraction/content/query timings on generated PDFs, no live store.
//! cargo run --release -p lumen-bench --example pdf_text -- report.json
#![allow(clippy::unwrap_used)]

use lumen_catalog::{ContentProvider, sync_files};
use lumen_content::{PassConfig, run_content_pass};
use lumen_core::{CancellationToken, Payload, Provider, ProviderQuery, QueryId};
use lumen_extract::{ChunkConfig, EstimateTokens, PdfLimits, extract_pdf};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::Store;
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

#[path = "../../../fixtures/pdf/mod.rs"]
mod fixture;
#[path = "../src/machine.rs"]
mod machine;
#[path = "../src/stats.rs"]
mod stats;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("release build required".into());
    }
    let report = std::env::args_os()
        .nth(1)
        .ok_or("provide report.json output path")?;
    let dir = Temp(std::env::temp_dir().join(format!("lumen-t301-bench-{}", std::process::id())));
    let files = dir.0.join("files");
    std::fs::create_dir_all(&files)?;
    let paragraph = "Coastal conservation protects coral reefs and fish habitats. Renewable solar energy powers remote research stations. ".repeat(20);
    let mut extraction = serde_json::Map::new();
    for page_count in [3, 32, 128] {
        let sample = dir.0.join(format!("sample-{page_count}.pdf"));
        let pages: Vec<_> = (0..page_count).map(|_| paragraph.as_bytes()).collect();
        fixture::document(&pages).save(&sample)?;
        let mut samples = Vec::new();
        let mut chunks = 0;
        for i in 0..25 {
            let started = Instant::now();
            let indexed = extract_pdf(
                &sample,
                &PdfLimits::default(),
                &ChunkConfig::default(),
                &EstimateTokens,
                &|| false,
            )?;
            chunks = indexed.chunks.len();
            if i >= 5 {
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        extraction.insert(page_count.to_string(), json!({"file_bytes": std::fs::metadata(sample)?.len(), "chunks": chunks, "latency": stats::Summary::of(&samples)}));
    }
    // Repeated corpus runs the actual content pass and durable FTS, including blank pages.
    let mut pdf = fixture::document(&[b"solar electricity", b"", paragraph.as_bytes()]);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes)?;
    for i in 0..100 {
        std::fs::write(files.join(format!("guide{i:03}.pdf")), &bytes)?;
    }
    let db = dir.0.join("synthetic.db");
    let mut store = Store::open_writer(&db)?;
    sync_files(
        &mut store,
        &ScanOptions {
            roots: vec![files],
            exclusions: Exclusions::default(),
            identity: false,
        },
        None,
    )?;
    let never = CancellationToken::new();
    let memory_before = machine::memory();
    let started = Instant::now();
    let indexed = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &never,
        &|| 1,
        &mut |_| {},
    )?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let memory_after = machine::memory();
    store.checkpoint()?;
    let provider = ContentProvider::new(Store::open_reader(&db)?);
    let query = ProviderQuery {
        id: QueryId::new(1).ok_or("nonzero query id required")?,
        text: "coral reefs ext:pdf",
        typing: false,
        limit: 30,
    };
    let mut samples = Vec::new();
    let mut hits = 0;
    for i in 0..105 {
        let started = Instant::now();
        let results = provider.search(&query, &never)?;
        hits = results.len();
        if hits == 0
            || results
                .iter()
                .any(|r| !matches!(&r.payload, Payload::Pdf(p) if p.page_number.get() == 3))
        {
            return Err("page context lost in FTS results".into());
        }
        if i >= 5 {
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let resumed = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &never,
        &|| 2,
        &mut |_| {},
    )?;
    let data = json!({
        "task": "T301", "machine": machine::MachineInfo::collect(), "synthetic_only": true,
        "backend": "lopdf 0.45.0 single thread; EstimateTokens; no embedding/model",
        "extraction_by_page_count": extraction,
        "content_pass": {"files": indexed.files, "indexed": indexed.indexed, "chunks": indexed.chunks, "text_bytes": indexed.text_bytes, "elapsed_ms": elapsed_ms,
            "memory_before": memory_before, "memory_after": memory_after, "unchanged_resume_files": resumed.files},
        "settled_fts": {"hits": hits, "matched_page": 3, "latency": stats::Summary::of(&samples)},
    });
    std::fs::write(report, serde_json::to_vec_pretty(&data)?)?;
    Ok(())
}
