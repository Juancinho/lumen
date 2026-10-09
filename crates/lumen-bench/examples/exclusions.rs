//! T112 release-only exclusion cleanup on 100k synthetic entries, never live data.
//! cargo run --release -p lumen-bench --example exclusions -- report.json
use lumen_catalog::{CatalogProvider, IndexLocations, exclusions::prune_user_exclusions};
use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId};
use lumen_storage::{GenerationSpec, Source, Store};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
    if cfg!(debug_assertions) {
        return Err("measure with --release".into());
    }
    let report = std::env::args_os().nth(1).ok_or("provide report.json")?;
    let dir =
        TempDir(std::env::temp_dir().join(format!("lumen-t112-bench-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0)?;
    let db = dir.0.join("synthetic.db");
    let mut store = Store::open_writer(&db)?;
    let generation = store.ensure_generation(
        GenerationSpec {
            space_key: "t112-synthetic",
            chunker_version: 1,
            dim: 256,
        },
        0,
    )?;
    store.promote_first(generation, 0)?;
    // Tooling fixture: one unit vector, one chunk and one usage event per item.
    let mut blob = vec![0_u8; 512];
    blob[1] = 0x3c;
    let conn = store.connection();
    conn.execute_batch("BEGIN")?;
    {
        let mut item = conn.prepare("INSERT INTO items(kind,source,canonical_path,display_name,name_key,name_parts,path_parts,extension) VALUES ('file','files',?1,?2,?2,?2,'offline',?3)")?;
        let mut chunk = conn.prepare(
            "INSERT INTO chunks(item_id,ordinal,chunk_kind,text) VALUES (?1,0,'text','needle')",
        )?;
        let mut vector = conn.prepare("INSERT INTO chunk_vectors(chunk_id,generation,vector,embedded_at,seq) VALUES (?1,?2,?3,0,?1)")?;
        let mut usage = conn.prepare(
            "INSERT INTO usage_stats(item_id,uses,last_used_at,rank_key) VALUES (?1,1,0,0)",
        )?;
        for i in 0..100_000 {
            let ext = ["js", "json", "log", "md"][i % 4];
            let name = format!("entry{i:06}.{ext}");
            item.execute((&format!("D:/offline/{name}"), &name, ext))?;
            let id = conn.last_insert_rowid();
            chunk.execute([id])?;
            vector.execute((conn.last_insert_rowid(), generation, &blob))?;
            usage.execute([id])?;
        }
    }
    conn.execute_batch("COMMIT")?;
    store.checkpoint()?;
    let provider = CatalogProvider::new(Store::open_reader(&db)?);
    let query = ProviderQuery {
        id: QueryId::new(1).ok_or("invalid query id")?,
        text: "entry099999",
        typing: true,
        limit: 10,
    };
    let cancel = CancellationToken::new();
    for _ in 0..20 {
        assert_eq!(
            provider
                .search(&query, &cancel)?
                .first()
                .map(|r| r.title.as_str()),
            Some("entry099999.md")
        );
    }
    let done = Arc::new(AtomicBool::new(false));
    let reader_done = Arc::clone(&done);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut samples = Vec::new();
        let query = ProviderQuery {
            id: QueryId::new(1).expect("valid id"),
            text: "entry099999",
            typing: true,
            limit: 10,
        };
        ready_tx.send(()).expect("writer waiting");
        while !reader_done.load(Ordering::Acquire) {
            let start = Instant::now();
            assert_eq!(
                provider
                    .search(&query, &cancel)
                    .expect("query")
                    .first()
                    .map(|r| r.title.as_str()),
                Some("entry099999.md")
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples
    });
    ready_rx.recv()?;
    let mut model = IndexLocations::standard(&[PathBuf::from("D:/offline")], 0);
    for ext in ["js", "json", "log"] {
        model.set_extension_excluded(ext, true);
    }
    let start = Instant::now();
    let removed = prune_user_exclusions(&mut store, &model, &CancellationToken::new())?;
    let cleanup_ms = start.elapsed().as_secs_f64() * 1000.0;
    done.store(true, Ordering::Release);
    let queries = reader.join().map_err(|_| "query thread failed")?;
    assert_eq!(removed, 75_000);
    assert_eq!(store.count_items(Source::Files)?, 25_000);
    assert_eq!(store.vector_count(generation)?, 25_000);
    assert_eq!(
        store.active_generation()?.ok_or("generation missing")?.id,
        generation
    );
    let start = Instant::now();
    assert_eq!(
        prune_user_exclusions(&mut store, &model, &CancellationToken::new())?,
        0
    );
    let retained_scan_ms = start.elapsed().as_secs_f64() * 1000.0;
    std::fs::write(
        report,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1, "task": "T112", "release": true, "os": std::env::consts::OS,
            "machine": std::env::var("COMPUTERNAME").unwrap_or_default(),
            "synthetic_items": 100000, "chunks_vectors_usage_per_item": 1,
            "dimension": 256, "page_limit": 512, "removed": removed, "retained": 25000,
            "cleanup_ms": cleanup_ms, "retained_scan_ms": retained_scan_ms,
            "concurrent_name_queries": queries.len(), "concurrent_name_ms": stats::Summary::of(&queries),
            "assertions": "all queries return retained file; unaffected vectors and active generation retained",
            "limitations": "one synthetic loaded-machine run; not real-library ETA or whole-app latency"
        }))?,
    )?;
    drop(store);
    Ok(())
}
