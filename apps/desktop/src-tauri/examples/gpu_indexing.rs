//! Release-only synthetic GPU/CPU queue and query check. Never opens the app-data DB.
#![allow(clippy::print_stdout)]

#[path = "../src/gpu_probe.rs"]
#[allow(dead_code)]
mod gpu_probe;

use lumen_content::{Control, QueueConfig, QueueJob, run_queue};
use lumen_core::CancellationToken;
use lumen_embedding::policy::{
    IndexingPlan, PowerSource, Quarantine, ResourceProfile, SystemState, accelerated_indexing_plan,
};
use lumen_embedding::{EmbeddingTask, TextInput};
use lumen_embedding_ort::Device;
use lumen_semantic::{QueryConfig, QueryEmbedder};
use lumen_storage::{GenerationSpec, NewChunk, NewItem, Store};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("use --release for this measurement".into());
    }
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("gpu_indexing REQUEST.json REPORT.json".into());
    }
    let mut request: gpu_probe::Request = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let adapter = lumen_windows::gpu::dedicated_gpus()?
        .into_iter()
        .next()
        .ok_or("no dedicated GPU for this measurement")?;
    request.adapter = adapter.adapter;
    request.name = adapter.name;
    request.total_mib = adapter.memory_mib;
    let compatibility = gpu_probe::run(&request)?;
    assert!(compatibility.valid_for(&request.key, request.adapter));
    let plan = accelerated_indexing_plan(
        &compatibility.space,
        &compatibility.probes(),
        &Quarantine::new(),
        &SystemState {
            power: PowerSource::Ac,
            profile: ResourceProfile::Balanced,
            logical_cpus: 12,
            available_memory_mib: Some(4096),
            user_active: true,
        },
    );
    assert!(
        matches!(plan, IndexingPlan::Run { device, .. } if device == format!("dml:{}", request.adapter)),
        "native compatibility probe must admit the GPU for this hardware check"
    );
    let root = std::env::temp_dir().join(format!("lumen-t212-queue-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let db = root.join("synthetic.db");
    let mut store = Store::open_writer(&db)?;
    let item = store.insert_item(&NewItem::file("synthetic://t212.txt", "t212.txt"))?;
    let texts: Vec<_> = (0..26).map(|i| format!("Synthetic document {i}. {}",
        "Local search finds useful documents about projects, invoices, travel plans and source code. ".repeat(7))).collect();
    let chunks: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| NewChunk {
            item_id: item,
            ordinal: i64::try_from(i).expect("26 synthetic chunks fit i64"),
            chunk_kind: "text",
            text,
            symbol_name: None,
            page_number: None,
            start_offset: None,
            end_offset: None,
        })
        .collect();
    store.insert_chunks(&chunks)?;
    let cpu = gpu_probe::build(&request, Device::Cpu, 4)?;
    let space = cpu.space().key();
    let generation = store.ensure_generation(
        GenerationSpec {
            space_key: &space,
            chunker_version: lumen_extract::EXTRACTOR_VERSION,
            dim: 256,
        },
        1,
    )?;
    store.promote_first(generation, 1)?;
    let cancel = CancellationToken::new();
    let control = Control::new();
    let cpu_part = run_queue(
        &mut store,
        &QueueJob {
            embedder: &cpu,
            generation,
            control: &control,
            cancel: &cancel,
            cfg: QueueConfig {
                batch: 1,
                max_run: Duration::from_secs(30),
            },
        },
        &|| 1,
        &mut |counts| {
            if counts.embedded >= 2 {
                control.pause();
            }
        },
    )?;
    let original = store.vectors(generation, 0, 100)?;
    assert_eq!(original.len(), 2);
    let gpu = gpu_probe::build(
        &request,
        Device::DirectMl {
            adapter: request.adapter,
        },
        1,
    )?;
    assert_eq!(space, gpu.space().key());
    assert_eq!(
        generation,
        store.ensure_generation(
            GenerationSpec {
                space_key: &gpu.space().key(),
                chunker_version: lumen_extract::EXTRACTOR_VERSION,
                dim: 256
            },
            1
        )?
    );
    gpu.warm_text()?;
    let warm: Vec<_> = texts[..8]
        .iter()
        .map(|s| TextInput::with_title(s, "t212.txt"))
        .collect();
    gpu.embed(EmbeddingTask::SearchDocument, &warm, None)?;
    control.resume();
    let gpu_part = run_queue(
        &mut store,
        &QueueJob {
            embedder: &gpu,
            generation,
            control: &control,
            cancel: &cancel,
            cfg: QueueConfig {
                batch: 8,
                max_run: Duration::from_secs(60),
            },
        },
        &|| 2,
        &mut |counts| {
            if counts.embedded >= 16 {
                control.pause();
            }
        },
    )?;
    let after = store.vectors(generation, 0, 100)?;
    assert!(
        original.iter().all(|pair| after.contains(pair)),
        "CPU vectors changed"
    );
    assert_eq!(gpu_part.embedded, 16);
    control.resume();
    // The actual query lane uses a separate CPU session and holds the GPU queue.
    let query_request = request.clone();
    let query = QueryEmbedder::start(
        Box::new(move || gpu_probe::build(&query_request, Device::Cpu, 4)),
        Some(control.clone()),
        QueryConfig {
            cache: 0,
            ..QueryConfig::default()
        },
    )?;
    query.embed("warm up", &cancel)?;
    let background_control = control.clone();
    let background_cancel = cancel.clone();
    let path = db.clone();
    let worker = std::thread::spawn(move || {
        let mut store = Store::open_writer(&path).map_err(|e| e.to_string())?;
        run_queue(
            &mut store,
            &QueueJob {
                embedder: &gpu,
                generation,
                control: &background_control,
                cancel: &background_cancel,
                cfg: QueueConfig {
                    batch: 8,
                    max_run: Duration::from_secs(60),
                },
            },
            &|| 3,
            &mut |_| {},
        )
        .map_err(|e| e.to_string())
    });
    let mut latencies = Vec::new();
    for i in 0..40 {
        let start = Instant::now();
        let vector = query.embed(&format!("find useful project documents {i}"), &cancel)?;
        assert_eq!(vector.len(), 256);
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(Duration::from_millis(80));
    }
    let tail = worker.join().map_err(|_| "GPU queue worker panicked")??;
    assert_eq!(store.queue_counts(generation)?.pending(), 0);
    let before_fallback = store.vectors(generation, 0, 100)?;
    let retry = run_queue(
        &mut store,
        &QueueJob {
            embedder: &cpu,
            generation,
            control: &control,
            cancel: &cancel,
            cfg: QueueConfig::default(),
        },
        &|| 4,
        &mut |_| {},
    )?;
    assert_eq!(
        retry.embedded, 0,
        "device switching re-embedded completed vectors"
    );
    assert_eq!(before_fallback, store.vectors(generation, 0, 100)?);
    latencies.sort_by(f64::total_cmp);
    #[allow(clippy::cast_precision_loss)]
    let gpu_throughput = gpu_part.embedded as f64 / gpu_part.busy.as_secs_f64();
    let report = serde_json::json!({ "release": true, "synthetic_chunks": 26,
        "adapter": request.adapter, "gpu_name": request.name, "compatibility_probe": compatibility,
        "space": space, "generation": generation, "cpu_prefix_embedded": cpu_part.embedded,
        "gpu_bulk_embedded": gpu_part.embedded, "gpu_tail_embedded": tail.embedded,
        "gpu_bulk_chunks_per_s": gpu_throughput, "cpu_query_p50_ms": latencies[20],
        "cpu_query_p95_ms": latencies[37], "cpu_vectors_preserved": true,
        "device_switch_reembedded": retry.embedded, "pending_after": 0,
        "resident_indexing_concurrently": true });
    std::fs::write(&args[2], serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    drop(query);
    drop(cpu);
    drop(store);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root)?;
    Ok(())
}
