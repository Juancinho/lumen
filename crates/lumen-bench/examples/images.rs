//! T303 actual release visual retrieval, temporary corpus only; no live store.
//! cargo run --release -p lumen-bench --features ort --example images -- model-dir vision-dir ort.dll fixtures-dir report.json
#![allow(clippy::unwrap_used)]

use lumen_catalog::{CatalogProvider, sync_files};
use lumen_content::{Control, QueueConfig, QueueJob, run_image_pass, run_queue};
use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId};
use lumen_embedding::{Embedder, EmbeddingProfile, ImageInput, Modality};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::{GenerationSpec, Store};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Instant,
};

#[path = "../src/machine.rs"]
#[allow(dead_code)]
mod machine;
#[path = "../src/stats.rs"]
mod stats;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("release build required".into());
    }
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 5 {
        return Err("expected model-dir vision-dir ort.dll fixtures-dir report.json".into());
    }
    lumen_embedding_ort::init_runtime(&args[2])?;
    let mut cfg = lumen_embedding_ort::OrtConfig::new(
        &args[0],
        lumen_embedding_ort::ModelVariant::Q4,
        lumen_embedding_ort::Device::Cpu,
    );
    cfg.threads = Some(2);
    cfg.vision_dir = Some(args[1].clone());
    let mut query_cfg = cfg.clone();
    query_cfg.vision_dir = None;
    let backend = Arc::new(lumen_embedding_ort::OrtBackend::new(cfg)?);
    let e = Arc::new(Embedder::new(backend, EmbeddingProfile::DEFAULT)?);
    let mut image_timings = Vec::new();
    let mut vectors = Vec::new();
    let mut preparation = Vec::new();
    let memory = || memory_stats::memory_stats().map(|s| s.physical_mem);
    let before = memory();
    for name in ["0001.jpg", "0002.jpg"] {
        let started = Instant::now();
        let decoded = lumen_image::decode(&args[3].join(name), None, &|| false)?;
        let decode_ms = started.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let p = lumen_image::prepare(
            decoded.metadata.width,
            decoded.metadata.height,
            &decoded.rgb,
        )?;
        preparation.push(json!({"fixture":name,"width":decoded.metadata.width,"height":decoded.metadata.height,"tokens":p.tokens,"decode_ms":decode_ms,"preprocess_ms":start.elapsed().as_secs_f64()*1000.0}));
        drop(p);
        let start = Instant::now();
        let v = e
            .embed_images(
                &[ImageInput {
                    width: decoded.metadata.width,
                    height: decoded.metadata.height,
                    rgb: &decoded.rgb,
                }],
                None,
            )?
            .into_flat();
        image_timings.push(start.elapsed().as_secs_f64() * 1000.0);
        vectors.push(v);
    }
    let resident = memory();
    let mut scores = Vec::new();
    for query in [
        "a cat wrapped in a towel",
        "a sandy beach beside the ocean",
        "un gato envuelto en una toalla",
        "una playa de arena junto al mar",
    ] {
        let q = e.embed_query(query, None)?;
        scores.push(json!({"query":query,"cosines":vectors.iter().map(|v|lumen_embedding::dot(&q,v)).collect::<Vec<_>>()}));
    }
    let dir = std::env::temp_dir().join(format!("lumen-t303-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("lumen.db");
    let mut store = Store::open_writer(&db)?;
    sync_files(
        &mut store,
        &ScanOptions {
            roots: vec![args[3].clone()],
            exclusions: Exclusions::default(),
            identity: false,
        },
        None,
    )?;
    run_image_pass(&mut store, &|_| true, &CancellationToken::new(), &|| 1)?;
    let g = store.ensure_generation(
        GenerationSpec {
            space_key: &e.space().key(),
            chunker_version: 1,
            dim: 256,
        },
        1,
    )?;
    store.promote_first(g, 1)?;
    let queue_start = Instant::now();
    let report = run_queue(
        &mut store,
        &QueueJob {
            embedder: &e,
            generation: g,
            control: &Control::new(),
            cancel: &CancellationToken::new(),
            cfg: QueueConfig::default(),
        },
        &|| 2,
        &mut |_| {},
    )?;
    let pipeline_ms = queue_start.elapsed().as_secs_f64() * 1000.0;
    let index = lumen_semantic::SemanticIndex::open(
        &store,
        &dir,
        store.active_generation()?.unwrap(),
        lumen_semantic::IndexSettings::default(),
    )?;
    let ann = Arc::new(RwLock::new(Some(index)));
    store.checkpoint()?;
    let query_lane = Arc::new(lumen_semantic::QueryEmbedder::start(
        Box::new(move || {
            let backend = lumen_embedding_ort::OrtBackend::new(query_cfg.clone())
                .map_err(|e| e.to_string())?;
            Embedder::new(Arc::new(backend), EmbeddingProfile::DEFAULT).map_err(|e| e.to_string())
        }),
        None,
        Default::default(),
    )?);
    let semantic = lumen_semantic::SemanticProvider::new(
        query_lane.clone(),
        ann,
        Store::open_reader(&db)?,
        Default::default(),
    );
    let names = CatalogProvider::new(Store::open_reader(&db)?);
    let mut retrieval = Vec::new();
    for query in [
        "cat type:image",
        "playa type:image",
        "ocean ext:jpg",
        "0001.jpg",
    ] {
        let input = ProviderQuery {
            id: QueryId::new(1).unwrap(),
            text: query,
            typing: false,
            limit: 10,
        };
        let result = semantic.search(&input, &CancellationToken::new())?;
        let name = names.search(&input, &CancellationToken::new())?;
        retrieval.push(json!({"query":query,"semantic_titles":result.iter().map(|r|&r.title).collect::<Vec<_>>(),"name_titles":name.iter().map(|r|&r.title).collect::<Vec<_>>()}));
    }
    let mut name_samples = Vec::new();
    for _ in 0..100 {
        let t = Instant::now();
        names.search(
            &ProviderQuery {
                id: QueryId::new(1).unwrap(),
                text: "0001",
                typing: true,
                limit: 10,
            },
            &CancellationToken::new(),
        )?;
        name_samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    // Real CPU query session during an already-running visual native call. Queue holds
    // apply at image boundaries; they cannot interrupt a codec/ORT invocation in flight.
    let decoded = lumen_image::decode(&args[3].join("0001.jpg"), None, &|| false)?;
    let indexing = e.clone();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let visual_worker = std::thread::spawn(move || {
        start_tx.send(()).unwrap();
        indexing.embed_images(
            &[ImageInput {
                width: decoded.metadata.width,
                height: decoded.metadata.height,
                rgb: &decoded.rgb,
            }],
            None,
        )
    });
    start_rx.recv()?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut interactive_queries = Vec::new();
    let mut interactive_names = Vec::new();
    let mut overlapping_queries = 0;
    for _ in 0..30 {
        if !visual_worker.is_finished() {
            overlapping_queries += 1;
        }
        query_lane.clear_cache();
        let start = Instant::now();
        query_lane.embed("a sandy beach beside the ocean", &CancellationToken::new())?;
        interactive_queries.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        names.search(
            &ProviderQuery {
                id: QueryId::new(1).unwrap(),
                text: "0001",
                typing: true,
                limit: 10,
            },
            &CancellationToken::new(),
        )?;
        interactive_names.push(start.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    visual_worker
        .join()
        .map_err(|_| "visual benchmark worker panicked")??;
    let after = memory();
    e.backend().unload(Modality::Image)?;
    e.backend().unload(Modality::Text)?;
    let unloaded = memory();
    std::fs::write(
        &args[4],
        serde_json::to_vec_pretty(
            &json!({"task":"T303","machine":machine::MachineInfo::collect(),"cpu_threads":2,"space":e.space().key(),"preparation":preparation,"image_inference_cold_then_warm_ms":image_timings,"memory_bytes":{"before":before,"encoders_resident":resident,"after_query_and_pipeline":after,"unloaded":unloaded},"scores":scores,"retrieval":retrieval,"pipeline":{"embedded":report.embedded,"failed":report.failed,"elapsed_ms":pipeline_ms},"name_latency":stats::Summary::of(&name_samples),"interactive_during_image":{"samples_started_before_image_finished":overlapping_queries,"query_ms":stats::Summary::of(&interactive_queries),"name_ms":stats::Summary::of(&interactive_names)}}),
        )?,
    )?;
    drop(semantic);
    drop(names);
    drop(store);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
