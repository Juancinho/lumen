//! Release-only T208 latency on a synthetic 100k-item/chunk catalog. No live data.
//! cargo run --release -p lumen-bench --example query_syntax -- report.json
#![allow(clippy::unwrap_used)]

use lumen_catalog::{CatalogProvider, ContentProvider};
use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId, SearchQuery};
use lumen_storage::{SearchBudget, Store};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

#[path = "../src/stats.rs"]
mod stats;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = std::env::args_os()
        .nth(1)
        .ok_or("provide a report.json output path")?;
    let dir =
        TempDir(std::env::temp_dir().join(format!("lumen-t208-bench-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0)?;
    let db = dir.0.join("synthetic.db");
    let store = Store::open_writer(&db)?;
    // Bench tooling may write SQL; production providers use typed storage methods.
    let conn = store.connection();
    conn.execute_batch("BEGIN")?;
    {
        let mut insert = conn.prepare("INSERT INTO items(kind, source, canonical_path, display_name, name_key, name_parts, path_parts, extension, modified_at) VALUES ('file', 'files', ?1, ?2, ?2, ?3, 'docs', ?4, 1790812800000)")?;
        let mut chunk = conn.prepare(
            "INSERT INTO chunks(item_id, ordinal, chunk_kind, text) VALUES (?1, 0, 'text', ?2)",
        )?;
        for i in 0..100_000 {
            let ext = if i % 2 == 0 { "md" } else { "pdf" };
            let name = format!("invoice{i:06}.{ext}");
            let text = format!("invoice item{i:06} holiday beach");
            insert.execute((&format!("D:/Docs/{name}"), &name, &text, ext))?;
            chunk.execute((conn.last_insert_rowid(), &text))?;
        }
    }
    conn.execute_batch("COMMIT")?;
    store.checkpoint()?;
    let names = CatalogProvider::new(Store::open_reader(&db)?);
    let content = ContentProvider::new(Store::open_reader(&db)?);
    let never = CancellationToken::new();
    let scenarios: [(&str, &dyn Provider, &str, bool); 6] = [
        ("name_unfiltered", &names, "invoice099900", true),
        (
            "name_filtered",
            &names,
            "invoice099900 ext:md in:D:/Docs after:2026-09-30",
            true,
        ),
        (
            "metadata_only",
            &names,
            "ext:pdf in:Docs before:2026-10-02",
            true,
        ),
        ("content_unfiltered", &content, "item099900", false),
        (
            "content_filtered",
            &content,
            "item099900 ext:md in:D:/Docs after:2026-09-30",
            false,
        ),
        (
            "content_phrase",
            &content,
            "\"invoice item099900\" ext:md",
            false,
        ),
    ];
    let mut timings = serde_json::Map::new();
    for (label, provider, text, typing) in scenarios {
        let q = ProviderQuery {
            id: QueryId::new(1).unwrap(),
            text,
            typing,
            limit: 30,
        };
        let mut samples = Vec::new();
        let mut empty = 0;
        for i in 0..110 {
            let start = Instant::now();
            let rows = provider.search(&q, &never)?;
            if i >= 10 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                empty += usize::from(rows.is_empty());
            }
        }
        timings.insert(
            label.into(),
            json!({"timing": stats::Summary::of(&samples), "empty_queries": empty}),
        );
    }
    let mut parse = Vec::new();
    for _ in 0..1000 {
        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(SearchQuery::parse(std::hint::black_box(
                "invoice ext:md in:\"D:\\Mis documentos\" after:2026-01-01",
            )));
        }
        parse.push(start.elapsed().as_secs_f64() * 10.0); // per-call milliseconds
    }
    let syntax = SearchQuery::parse("ext:md in:D:/Docs");
    let mut filter = Vec::new();
    let ids: Vec<_> = (1..=1024).collect();
    for _ in 0..100 {
        let start = Instant::now();
        let allowed =
            store.matching_chunk_ids(&ids, &syntax.filters, &SearchBudget::unbounded())?;
        assert_eq!(allowed.len(), 512);
        filter.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let result = json!({"task": "T208", "profile": "release", "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "items": 100000, "chunks": 100000, "warmup": 10, "iterations": 100, "scenarios": timings,
        "parse_ms": stats::Summary::of(&parse), "ann_1024_metadata_filter_ms": stats::Summary::of(&filter)});
    std::fs::write(report, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
