//! `lumen-bench pipeline` (T202): the content-indexing path end to end over real folders —
//! catalog sync → content pass (extract + chunk + store) → embedding queue (embed + store
//! f16 vectors) — on a temporary database. Measures what the queue itself costs next to
//! the embedder (overhead per chunk, storage per vector) and what a duty cycle does to
//! throughput and CPU share.
//!
//! Privacy: counts and timings only; no path, name or text is written.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use lumen_catalog::{IndexLocations, sync_files};
use lumen_content::{
    Control, PassConfig, QueueConfig, QueueJob, Stop, run_content_pass, run_queue,
};
use lumen_core::CancellationToken;
use lumen_embedding::{Embedder, EmbeddingProfile};
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_storage::{GenerationSpec, Store};
use serde::Serialize;

use crate::cpu::{CpuUse, cpu_time};
use crate::embed::{EmbedOptions, make_backend};
use crate::machine::MachineInfo;

#[derive(Debug, Clone)]
pub(crate) struct PipelineOptions {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) work_dir: Option<PathBuf>,
    pub(crate) embed: EmbedOptions,
    pub(crate) duty: f64,
    pub(crate) batch: usize,
    /// Stop the queue after this long (a full CPU run over a large folder takes hours).
    pub(crate) max_seconds: f64,
    pub(crate) label: Option<String>,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            work_dir: None,
            embed: EmbedOptions::default(),
            duty: 1.0,
            batch: QueueConfig::default().batch,
            max_seconds: 120.0,
            label: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ContentStage {
    files: u64,
    indexed: u64,
    skipped: u64,
    failed: u64,
    chunks: u64,
    text_mib: f64,
    seconds: f64,
    files_per_s: f64,
    text_mib_per_s: f64,
    /// Re-running the pass right away (nothing changed) — the steady-state cost.
    rerun_seconds: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct QueueStage {
    embedded: u64,
    failed: u64,
    pending_after: u64,
    batches: u64,
    batch: usize,
    duty: f64,
    stop: &'static str,
    seconds: f64,
    busy_seconds: f64,
    yielded_seconds: f64,
    chunks_per_s: f64,
    /// Wall time per chunk outside the embedder and outside deliberate waits: queue reads,
    /// result writes, bookkeeping.
    overhead_ms_per_chunk: f64,
    cpu: Option<CpuUse>,
    /// Database growth per stored vector (row + index pages).
    db_bytes_per_vector: Option<f64>,
    /// A fresh run after reopening the database finds nothing left (the queue persisted).
    resumed_pending: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PipelineReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    backend: String,
    space: String,
    catalog_entries: u64,
    catalog_seconds: f64,
    content: ContentStage,
    queue: QueueStage,
    db_mib: f64,
}

fn db_size(dir: &std::path::Path) -> u64 {
    ["lumen.db", "lumen.db-wal"]
        .iter()
        .filter_map(|f| std::fs::metadata(dir.join(f)).ok())
        .map(|m| m.len())
        .sum()
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn run(opts: &PipelineOptions) -> Result<PipelineReport, String> {
    if opts.roots.is_empty() {
        return Err("--root DIR is required".into());
    }
    let work = opts.work_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("lumen-bench-pipeline-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("work dir: {e}"))?;
    let db = work.join("lumen.db");
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let now = || {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis()),
        )
        .unwrap_or(0)
    };

    let mut store = Store::open_writer(&db).map_err(|e| err(&e))?;
    let t = Instant::now();
    let locations = IndexLocations::standard(&opts.roots, now());
    let scan = sync_files(&mut store, &locations.scan_options(false), None).map_err(|e| err(&e))?;
    let catalog_seconds = t.elapsed().as_secs_f64();
    let catalog_entries = scan.scan.files + scan.scan.dirs;

    let cancel = CancellationToken::new();
    let pass = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &cancel,
        &now,
        &mut |_| {},
    )
    .map_err(|e| err(&e))?;
    let t = Instant::now();
    let rerun = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &cancel,
        &now,
        &mut |_| {},
    )
    .map_err(|e| err(&e))?;
    let rerun_seconds = t.elapsed().as_secs_f64();
    debug_assert_eq!(rerun.files, 0);
    let secs = pass.elapsed.as_secs_f64().max(1e-9);
    let text_mib = pass.text_bytes as f64 / (1024.0 * 1024.0);
    let content = ContentStage {
        files: pass.files,
        indexed: pass.indexed,
        skipped: pass.skipped,
        failed: pass.failed,
        chunks: pass.chunks,
        text_mib,
        seconds: secs,
        files_per_s: pass.files as f64 / secs,
        text_mib_per_s: text_mib / secs,
        rerun_seconds,
    };

    let (backend, _) = make_backend(&opts.embed.backend, &opts.embed)?;
    let embedder = Embedder::new(
        backend,
        EmbeddingProfile {
            dim: opts.embed.dim,
            ..EmbeddingProfile::DEFAULT
        },
    )
    .map_err(|e| err(&e))?;
    embedder.warm_text().map_err(|e| err(&e))?;
    let space = embedder.space().key();
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: &space,
                chunker_version: EXTRACTOR_VERSION,
                dim: opts.embed.dim,
            },
            now(),
        )
        .map_err(|e| err(&e))?;
    store.checkpoint().map_err(|e| err(&e))?;
    let size_before = db_size(&work);

    let control = Control::new();
    control.set_duty(opts.duty);
    let job = QueueJob {
        embedder: &embedder,
        generation,
        control: &control,
        cancel: &cancel,
        cfg: QueueConfig {
            batch: opts.batch.max(1),
            max_run: Duration::from_secs_f64(opts.max_seconds.max(0.0)),
        },
    };
    let cpu_start = cpu_time(None);
    let q = run_queue(&mut store, &job, &now, &mut |_| {}).map_err(|e| err(&e))?;
    let cpu = CpuUse::between(cpu_start, cpu_time(None), q.elapsed.as_secs_f64());
    store.checkpoint().map_err(|e| err(&e))?;
    let size_after = db_size(&work);
    let counts = store.queue_counts(generation).map_err(|e| err(&e))?;

    // Restart: a new connection must see the same pending set (0 when drained).
    drop(store);
    let reopened = Store::open_writer(&db).map_err(|e| err(&e))?;
    let resumed_pending = reopened.queue_counts(generation).ok().map(|c| c.pending());
    let db_mib = db_size(&work) as f64 / (1024.0 * 1024.0);
    drop(reopened);
    if opts.work_dir.is_none() {
        let _ = std::fs::remove_dir_all(&work);
    }

    let n = q.embedded + q.failed;
    let seconds = q.elapsed.as_secs_f64();
    let queue = QueueStage {
        embedded: q.embedded,
        failed: q.failed,
        pending_after: counts.pending(),
        batches: q.batches,
        batch: job.cfg.batch,
        duty: control.duty(),
        stop: match q.stop {
            Stop::Drained => "drained",
            Stop::Paused => "paused",
            Stop::Cancelled => "cancelled",
            Stop::TimeSlice => "time_limit",
        },
        seconds,
        busy_seconds: q.busy.as_secs_f64(),
        yielded_seconds: q.yielded.as_secs_f64(),
        chunks_per_s: if seconds > 0.0 {
            n as f64 / seconds
        } else {
            0.0
        },
        overhead_ms_per_chunk: if n > 0 {
            1000.0 * (q.elapsed.saturating_sub(q.busy + q.yielded)).as_secs_f64() / n as f64
        } else {
            0.0
        },
        cpu,
        db_bytes_per_vector: (q.embedded > 0)
            .then(|| size_after.saturating_sub(size_before) as f64 / q.embedded as f64),
        resumed_pending,
    };
    Ok(PipelineReport {
        schema_version: 1,
        kind: "pipeline",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        backend: embedder.backend().capabilities().backend.clone(),
        space,
        catalog_entries,
        catalog_seconds,
        content,
        queue,
        db_mib,
    })
}

pub(crate) fn summarize(r: &PipelineReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "pipeline ({}): {}", r.backend, r.space);
    let _ = writeln!(
        s,
        "  catalog: {} entries in {:.2} s",
        r.catalog_entries, r.catalog_seconds
    );
    let c = &r.content;
    let _ = writeln!(
        s,
        "  content: {} files ({} indexed, {} skipped, {} failed) -> {} chunks, {:.1} MiB text in {:.2} s ({:.0} files/s, {:.1} MiB/s); rerun {:.3} s",
        c.files,
        c.indexed,
        c.skipped,
        c.failed,
        c.chunks,
        c.text_mib,
        c.seconds,
        c.files_per_s,
        c.text_mib_per_s,
        c.rerun_seconds
    );
    let q = &r.queue;
    let _ = writeln!(
        s,
        "  queue: {} embedded, {} failed, {} pending ({}) in {:.2} s = {:.1} chunks/s · busy {:.2} s · yielded {:.2} s · overhead {:.3} ms/chunk{}",
        q.embedded,
        q.failed,
        q.pending_after,
        q.stop,
        q.seconds,
        q.chunks_per_s,
        q.busy_seconds,
        q.yielded_seconds,
        q.overhead_ms_per_chunk,
        q.cpu.map_or(String::new(), |c| format!(
            " · {:.2} cores ({:.0}% of machine)",
            c.cores, c.machine_percent
        ))
    );
    if let Some(b) = q.db_bytes_per_vector {
        let _ = writeln!(s, "  storage: {b:.0} B per vector · db {:.1} MiB", r.db_mib);
    }
    s
}
