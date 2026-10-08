//! `lumen-bench query-lane` (T204): warm query-embedding latency through the
//! `QueryEmbedder` service — alone, next to a busy indexing session, and next to indexing
//! that the query lane preempts (`Control::hold`). Two runtime sessions, as in the app:
//! one for queries, one for indexing.
//!
//! Privacy: built-in synthetic queries and documents only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use lumen_content::Control;
use lumen_core::CancellationToken;
use lumen_embedding::{Embedder, EmbeddingProfile, EmbeddingTask, TextInput};
use lumen_semantic::{QueryConfig, QueryEmbedder};
use serde::Serialize;

use crate::corpus;
use crate::embed::{EmbedOptions, make_backend};
use crate::machine::MachineInfo;
use crate::stats::Summary;

#[derive(Debug, Clone)]
pub(crate) struct QueryLaneOptions {
    pub(crate) embed: EmbedOptions,
    pub(crate) query_threads: Option<usize>,
    pub(crate) index_threads: Option<usize>,
    /// Chunks per indexing call (what a query can collide with).
    pub(crate) index_batch: usize,
    pub(crate) queries: usize,
    /// Pause between queries (typing cadence).
    pub(crate) gap: Duration,
    pub(crate) label: Option<String>,
}

impl Default for QueryLaneOptions {
    fn default() -> Self {
        Self {
            embed: EmbedOptions::default(),
            query_threads: None,
            index_threads: None,
            index_batch: 8,
            queries: 60,
            gap: Duration::from_millis(80),
            label: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Scenario {
    name: &'static str,
    /// `QueryEmbedder::embed` wall time per query (cache off).
    latency: Summary,
    /// Chunks the indexing session embedded per second during the scenario.
    indexing_chunks_per_s: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct QueryLaneReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    backend: String,
    query_threads: Option<usize>,
    index_threads: Option<usize>,
    index_batch: usize,
    gap_ms: f64,
    scenarios: Vec<Scenario>,
}

fn embedder(opts: &EmbedOptions, threads: Option<usize>) -> Result<Embedder, String> {
    let mut o = opts.clone();
    o.ort.threads = threads;
    let (backend, _) = make_backend(&o.backend, &o)?;
    Embedder::new(
        backend,
        EmbeddingProfile {
            dim: o.dim,
            ..EmbeddingProfile::DEFAULT
        },
    )
    .map_err(|e| e.to_string())
}

/// Background indexing load: batches of `batch` synthetic ~128-token chunks until `stop`.
fn indexing_load(
    e: Embedder,
    batch: usize,
    control: Control,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let docs: Vec<String> = (0..batch.max(1) as u64)
            .map(|i| corpus::synthetic_document(i + 100, 100))
            .collect();
        let inputs: Vec<TextInput<'_>> = docs.iter().map(|d| TextInput::new(d)).collect();
        let never = CancellationToken::new();
        while !stop.load(Ordering::Relaxed) {
            if !control.wait_until_runnable(&never, Duration::from_millis(50)) {
                continue;
            }
            if e.embed(EmbeddingTask::SearchDocument, &inputs, None)
                .is_ok()
            {
                done.fetch_add(inputs.len() as u64, Ordering::Relaxed);
            }
        }
    })
}

fn run_queries(q: &QueryEmbedder, opts: &QueryLaneOptions) -> Result<Summary, String> {
    let never = CancellationToken::new();
    let mut samples = Vec::with_capacity(opts.queries);
    for (i, text) in corpus::QUERIES
        .iter()
        .cycle()
        .take(opts.queries)
        .enumerate()
    {
        // Distinct text per query: the cache is off, but keep it honest anyway.
        let text = format!("{text} {i}");
        let t = Instant::now();
        q.embed(&text, &never).map_err(|e| e.to_string())?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(opts.gap);
    }
    Summary::of(&samples).ok_or_else(|| "no queries".into())
}

pub(crate) fn run(opts: &QueryLaneOptions) -> Result<QueryLaneReport, String> {
    let cfg = QueryConfig {
        cache: 0,
        ..QueryConfig::default()
    };
    let mut scenarios = Vec::new();
    let backend_name = embedder(&opts.embed, opts.query_threads)?
        .backend()
        .capabilities()
        .backend
        .clone();

    for (name, load, preempt) in [
        ("queries_only", false, false),
        ("with_indexing", true, false),
        ("with_indexing_preempted", true, true),
    ] {
        let control = Control::new();
        let (eo, qt) = (opts.embed.clone(), opts.query_threads);
        let q = QueryEmbedder::start(
            Box::new(move || embedder(&eo, qt)),
            preempt.then(|| control.clone()),
            cfg,
        )
        .map_err(|e| e.to_string())?;
        // Warm both sessions before measuring.
        q.embed("warm up", &CancellationToken::new())
            .map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let loader = if load {
            let e = embedder(&opts.embed, opts.index_threads)?;
            e.warm_text().map_err(|e| e.to_string())?;
            let h = indexing_load(
                e,
                opts.index_batch,
                control.clone(),
                Arc::clone(&stop),
                Arc::clone(&done),
            );
            // Let the indexing session reach steady state.
            std::thread::sleep(Duration::from_millis(500));
            Some(h)
        } else {
            None
        };
        done.store(0, Ordering::Relaxed);
        let t = Instant::now();
        let latency = run_queries(&q, opts)?;
        let secs = t.elapsed().as_secs_f64();
        stop.store(true, Ordering::Relaxed);
        if let Some(h) = loader {
            let _ = h.join();
        }
        #[allow(clippy::cast_precision_loss)]
        let indexing_chunks_per_s = load.then(|| done.load(Ordering::Relaxed) as f64 / secs);
        scenarios.push(Scenario {
            name,
            latency,
            indexing_chunks_per_s,
        });
    }
    Ok(QueryLaneReport {
        schema_version: 1,
        kind: "query-lane",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        backend: backend_name,
        query_threads: opts.query_threads,
        index_threads: opts.index_threads,
        index_batch: opts.index_batch,
        gap_ms: opts.gap.as_secs_f64() * 1000.0,
        scenarios,
    })
}

pub(crate) fn summarize(r: &QueryLaneReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "query lane ({}): query threads {:?}, index threads {:?} × batch {}, gap {:.0} ms",
        r.backend, r.query_threads, r.index_threads, r.index_batch, r.gap_ms
    );
    for sc in &r.scenarios {
        let _ = writeln!(
            s,
            "  {:<24} p50 {:>7.1} · p95 {:>7.1} · max {:>7.1} ms (n={}){}",
            sc.name,
            sc.latency.p50_ms,
            sc.latency.p95_ms,
            sc.latency.max_ms,
            sc.latency.n,
            sc.indexing_chunks_per_s
                .map_or(String::new(), |c| format!(" · indexing {c:.1} chunks/s"))
        );
    }
    s
}
