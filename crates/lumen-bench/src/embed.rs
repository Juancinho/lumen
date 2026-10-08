//! `lumen-bench embed`: query latency, document throughput, load cost and memory
//! for one embedding backend. T006 runs this per candidate runtime/device.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_embedding::{
    Embedder, EmbeddingBackend, EmbeddingProfile, EmbeddingTask, MockBackend, MockLatency,
    TextInput,
};
use serde::Serialize;

use crate::corpus;
use crate::fidelity::{self, FidelityReport};
use crate::machine::{self, MachineInfo, MemorySnapshot};
use crate::stats::Summary;

/// docs/PERFORMANCE.md §2: warm text query embedding.
pub(crate) const BUDGET_P50_MS: f64 = 60.0;
pub(crate) const BUDGET_P95_MS: f64 = 120.0;

#[derive(Debug, Clone)]
pub(crate) struct EmbedOptions {
    pub(crate) backend: String,
    pub(crate) dim: usize,
    pub(crate) warmup: usize,
    pub(crate) iterations: usize,
    pub(crate) batch_sizes: Vec<usize>,
    pub(crate) docs: usize,
    pub(crate) doc_words: usize,
    pub(crate) queries: Option<Vec<String>>,
    pub(crate) mock_latency: MockLatency,
    pub(crate) label: Option<String>,
    /// Words of the long-input probe (~1.3 tokens/word; 100 words ≈ the 128-token
    /// signature of Google's published benchmarks). 0 disables it.
    pub(crate) long_words: usize,
    /// Reference vectors for the fidelity check (`--reference`).
    pub(crate) reference: Option<PathBuf>,
    pub(crate) corpus: PathBuf,
    pub(crate) ort: OrtOptions,
    pub(crate) llama: crate::llama::LlamaOptions,
    /// Process whose CPU time the throughput phase reports (`--cpu-pid`; default this
    /// process). Set it to the `llama-server` PID when that process does the work.
    pub(crate) cpu_pid: Option<u32>,
}

/// `--backend ort` settings (ignored by other backends).
#[derive(Debug, Clone, Default)]
pub(crate) struct OrtOptions {
    pub(crate) model_dir: Option<PathBuf>,
    pub(crate) variant: Option<String>,
    pub(crate) device: Option<String>,
    pub(crate) threads: Option<usize>,
    pub(crate) dylib: Option<PathBuf>,
    /// Report which EP got which nodes (one extra model load before measuring).
    pub(crate) placement: bool,
    /// Fail instead of running unsupported GPU nodes on CPU.
    pub(crate) no_cpu_fallback: bool,
}

impl Default for EmbedOptions {
    fn default() -> Self {
        Self {
            backend: "mock".into(),
            dim: 256,
            warmup: 20,
            iterations: 200,
            batch_sizes: vec![1, 8, 32],
            docs: 256,
            doc_words: 200,
            queries: None,
            mock_latency: MockLatency::default(),
            label: None,
            long_words: 100,
            reference: None,
            corpus: PathBuf::from("fixtures/embedding/corpus.json"),
            ort: OrtOptions::default(),
            llama: crate::llama::LlamaOptions::default(),
            cpu_pid: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct BackendInfo {
    name: String,
    runtime_version: Option<String>,
    target: &'static str,
    device: Option<String>,
    max_batch: usize,
    concurrent_calls: bool,
    model_id: String,
    model_revision: String,
    native_dim: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct QueryLatency {
    pub(crate) first_query_ms: f64,
    pub(crate) warm: Summary,
    /// Single long input (`long_words` words), comparable to 128-token reference numbers.
    pub(crate) long_input: Option<Summary>,
    pub(crate) long_words: usize,
    pub(crate) budget_p50_ms: f64,
    pub(crate) budget_p95_ms: f64,
    pub(crate) within_budget: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Throughput {
    pub(crate) batch_size: usize,
    pub(crate) items: usize,
    pub(crate) items_per_s: f64,
    pub(crate) per_batch: Summary,
    /// CPU spent by the measured process during this batch size (T014).
    pub(crate) cpu: Option<crate::cpu::CpuUse>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MemoryReport {
    before_load: Option<MemorySnapshot>,
    after_warm: Option<MemorySnapshot>,
    after_throughput: Option<MemorySnapshot>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmbedReport {
    pub(crate) schema_version: u32,
    pub(crate) kind: &'static str,
    pub(crate) label: Option<String>,
    pub(crate) machine: MachineInfo,
    pub(crate) backend: BackendInfo,
    pub(crate) space: String,
    pub(crate) cold_load_ms: f64,
    pub(crate) query: QueryLatency,
    pub(crate) throughput: Vec<Throughput>,
    pub(crate) memory: MemoryReport,
    pub(crate) doc_words: usize,
    pub(crate) fidelity: Option<FidelityReport>,
    /// Backend-specific settings that define the run (variant, device, threads).
    pub(crate) config: serde_json::Value,
    pub(crate) diagnostics: serde_json::Value,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Creates a backend by name. Runtime backends are behind cargo features.
/// Also returns backend-specific diagnostics for the report (e.g. node placement).
pub(crate) fn make_backend(
    name: &str,
    opts: &EmbedOptions,
) -> Result<(Arc<dyn EmbeddingBackend>, serde_json::Value), String> {
    match name {
        "mock" => Ok((
            Arc::new(MockBackend::with_latency(opts.mock_latency)),
            serde_json::Value::Null,
        )),
        #[cfg(feature = "ort")]
        "ort" => make_ort(&opts.ort),
        "llama-server" => Ok((
            Arc::new(crate::llama::LlamaServerBackend::new(&opts.llama)),
            serde_json::Value::Null,
        )),
        other => Err(format!(
            "unknown backend `{other}` (available: mock, llama-server{})",
            if cfg!(feature = "ort") {
                ", ort"
            } else {
                "; build with --features ort for ort"
            }
        )),
    }
}

#[cfg(feature = "ort")]
fn make_ort(o: &OrtOptions) -> Result<(Arc<dyn EmbeddingBackend>, serde_json::Value), String> {
    use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig, init_runtime};
    let dylib = o
        .dylib
        .as_ref()
        .ok_or("--ort-dylib is required for --backend ort")?;
    init_runtime(dylib).map_err(|e| e.to_string())?;
    let model_dir = o
        .model_dir
        .as_ref()
        .ok_or("--model-dir is required for --backend ort")?;
    let variant_name = o.variant.as_deref().unwrap_or("q4");
    let variant = ModelVariant::parse(variant_name)
        .ok_or_else(|| format!("unknown --variant `{variant_name}` (fp32, fp16, q8, q4, q4f16)"))?;
    let device_name = o.device.as_deref().unwrap_or("cpu");
    let device = Device::parse(device_name).ok_or_else(|| {
        format!("unknown --device `{device_name}` (cpu, dml:<adapter>, dml:high, dml:low)")
    })?;
    let mut config = OrtConfig::new(model_dir, variant, device);
    config.threads = o.threads;
    config.cpu_fallback = !o.no_cpu_fallback;
    let backend = OrtBackend::new(config).map_err(|e| e.to_string())?;
    let diagnostics = if o.placement {
        let p = backend.placement().map_err(|e| e.to_string())?;
        serde_json::json!({ "placement": {
            "nodes_per_provider": p.nodes_per_provider,
            "cpu_op_types": p.cpu_op_types,
            "offloaded_fraction": p.offloaded_fraction(),
        }})
    } else {
        serde_json::Value::Null
    };
    Ok((Arc::new(backend), diagnostics))
}

fn backend_config(opts: &EmbedOptions) -> serde_json::Value {
    if opts.backend == "llama-server" {
        return serde_json::json!({
            "addr": opts.llama.addr.as_deref().unwrap_or("127.0.0.1:8080"),
            "variant": opts.llama.variant.as_deref().unwrap_or("gguf-q8_0"),
            "target": opts.llama.target.as_deref().unwrap_or("cpu"),
        });
    }
    if opts.backend == "ort" {
        serde_json::json!({
            "variant": opts.ort.variant.as_deref().unwrap_or("q4"),
            "device": opts.ort.device.as_deref().unwrap_or("cpu"),
            "threads": opts.ort.threads,
            "ort_dylib": opts.ort.dylib.as_ref().map(|p| p.display().to_string()),
        })
    } else {
        serde_json::json!({ "mock_latency_ms": {
            "load": opts.mock_latency.load.as_secs_f64() * 1000.0,
            "per_call": opts.mock_latency.per_call.as_secs_f64() * 1000.0,
            "per_item": opts.mock_latency.per_item.as_secs_f64() * 1000.0,
        }})
    }
}

/// Runs the benchmark.
///
/// # Errors
/// Backend creation, invalid profile or embedding failures, as a message.
pub(crate) fn run(opts: &EmbedOptions) -> Result<EmbedReport, String> {
    if opts.iterations == 0 {
        return Err("--iterations must be > 0".into());
    }
    let owned_queries: Vec<String>;
    let queries: Vec<&str> = match &opts.queries {
        Some(list) => {
            owned_queries = list.clone();
            owned_queries.iter().map(String::as_str).collect()
        }
        None => corpus::QUERIES.to_vec(),
    };
    if queries.is_empty() {
        return Err("query list is empty".into());
    }

    let before_load = machine::memory();

    // Cold path: construct + validate + load the text encoder.
    let started = Instant::now();
    let (backend, diagnostics) = make_backend(&opts.backend, opts)?;
    let profile = EmbeddingProfile {
        dim: opts.dim,
        ..EmbeddingProfile::DEFAULT
    };
    let embedder = Embedder::new(backend, profile).map_err(|e| e.to_string())?;
    embedder.warm_text().map_err(|e| e.to_string())?;
    let cold_load_ms = ms(started.elapsed());
    let after_warm = machine::memory();

    // First query after load (may include lazy runtime init).
    let started = Instant::now();
    embedder
        .embed_query(queries[0], None)
        .map_err(|e| e.to_string())?;
    let first_query_ms = ms(started.elapsed());

    // Warm, single-query latency: the interactive path.
    for q in queries.iter().cycle().take(opts.warmup) {
        embedder.embed_query(q, None).map_err(|e| e.to_string())?;
    }
    let mut samples = Vec::with_capacity(opts.iterations);
    for q in queries.iter().cycle().take(opts.iterations) {
        let started = Instant::now();
        let v = embedder.embed_query(q, None).map_err(|e| e.to_string())?;
        samples.push(ms(started.elapsed()));
        std::hint::black_box(v);
    }
    let warm = Summary::of(&samples).ok_or("no samples")?;
    let within_budget = warm.p50_ms <= BUDGET_P50_MS && warm.p95_ms <= BUDGET_P95_MS;

    // Long single input: comparable with published 128-token figures.
    let long_input = if opts.long_words > 0 {
        let long = corpus::synthetic_document(999, opts.long_words);
        let runs = (opts.iterations / 4).max(3);
        let mut long_samples = Vec::with_capacity(runs);
        for _ in 0..runs {
            let started = Instant::now();
            let v = embedder
                .embed_query(&long, None)
                .map_err(|e| e.to_string())?;
            long_samples.push(ms(started.elapsed()));
            std::hint::black_box(v);
        }
        Summary::of(&long_samples)
    } else {
        None
    };

    // Document throughput per batch size: the indexing path.
    let docs: Vec<String> = (0..opts.docs)
        .map(|i| corpus::synthetic_document(i as u64 + 1, opts.doc_words))
        .collect();
    let inputs: Vec<TextInput<'_>> = docs.iter().map(|d| TextInput::new(d)).collect();
    let mut throughput = Vec::new();
    for &batch_size in &opts.batch_sizes {
        if batch_size == 0 || inputs.is_empty() {
            continue;
        }
        let mut per_batch = Vec::new();
        let cpu_start = crate::cpu::cpu_time(opts.cpu_pid);
        let started = Instant::now();
        for chunk in inputs.chunks(batch_size) {
            let t = Instant::now();
            let out = embedder
                .embed(EmbeddingTask::SearchDocument, chunk, None)
                .map_err(|e| e.to_string())?;
            per_batch.push(ms(t.elapsed()));
            std::hint::black_box(out);
        }
        let total = started.elapsed().as_secs_f64();
        let cpu = crate::cpu::CpuUse::between(cpu_start, crate::cpu::cpu_time(opts.cpu_pid), total);
        #[allow(clippy::cast_precision_loss)]
        let items_per_s = if total > 0.0 {
            inputs.len() as f64 / total
        } else {
            f64::INFINITY
        };
        throughput.push(Throughput {
            batch_size,
            items: inputs.len(),
            items_per_s,
            per_batch: Summary::of(&per_batch).ok_or("no batches")?,
            cpu,
        });
    }

    let fidelity = match &opts.reference {
        Some(reference) => Some(fidelity::evaluate(&embedder, &opts.corpus, reference)?),
        None => None,
    };

    let caps = embedder.backend().capabilities();
    Ok(EmbedReport {
        schema_version: 2,
        kind: "embed",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        backend: BackendInfo {
            name: caps.backend.clone(),
            runtime_version: caps.runtime_version.clone(),
            target: caps.target.as_str(),
            device: caps.device.clone(),
            max_batch: caps.max_batch,
            concurrent_calls: caps.concurrent_calls,
            model_id: caps.model.id.clone(),
            model_revision: caps.model.revision.clone(),
            native_dim: caps.model.native_dim,
        },
        space: embedder.space().key(),
        cold_load_ms,
        query: QueryLatency {
            first_query_ms,
            warm,
            long_input,
            long_words: opts.long_words,
            budget_p50_ms: BUDGET_P50_MS,
            budget_p95_ms: BUDGET_P95_MS,
            within_budget,
        },
        throughput,
        memory: MemoryReport {
            before_load,
            after_warm,
            after_throughput: machine::memory(),
        },
        doc_words: opts.doc_words,
        fidelity,
        config: backend_config(opts),
        diagnostics,
    })
}

/// Human-readable summary (stderr).
pub(crate) fn summarize(r: &EmbedReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let q = &r.query.warm;
    let _ = writeln!(
        s,
        "embed · backend={} target={} space={}",
        r.backend.name, r.backend.target, r.space
    );
    let _ = writeln!(
        s,
        "  machine: {} {} · {} cpus · {} · build={}",
        r.machine.os,
        r.machine.arch,
        r.machine.logical_cpus,
        r.machine.cpu.as_deref().unwrap_or("unknown cpu"),
        r.machine.build_profile
    );
    let _ = writeln!(
        s,
        "  cold load {:.1} ms · first query {:.2} ms",
        r.cold_load_ms, r.query.first_query_ms
    );
    let _ = writeln!(
        s,
        "  warm query (n={}): p50 {:.2} · p95 {:.2} · p99 {:.2} · max {:.2} ms  [{} budget {}/{} ms]",
        q.n,
        q.p50_ms,
        q.p95_ms,
        q.p99_ms,
        q.max_ms,
        if r.query.within_budget {
            "within"
        } else {
            "OVER"
        },
        r.query.budget_p50_ms,
        r.query.budget_p95_ms
    );
    if let Some(l) = &r.query.long_input {
        let _ = writeln!(
            s,
            "  long input (~{} words, n={}): p50 {:.2} · p95 {:.2} ms",
            r.query.long_words, l.n, l.p50_ms, l.p95_ms
        );
    }
    for t in &r.throughput {
        let _ = writeln!(
            s,
            "  docs batch {:>3}: {:>9.1} items/s · p50 {:.2} ms/batch ({} items × ~{} words){}",
            t.batch_size,
            t.items_per_s,
            t.per_batch.p50_ms,
            t.items,
            r.doc_words,
            t.cpu.map_or(String::new(), |c| format!(
                " · {:.2} cores ({:.0}% of machine)",
                c.cores, c.machine_percent
            ))
        );
    }
    if let (Some(a), Some(b)) = (r.memory.before_load, r.memory.after_warm) {
        let _ = writeln!(
            s,
            "  memory resident: {:.1} MiB before load → {:.1} MiB warm",
            a.resident_mib, b.resident_mib
        );
    }
    if let Some(f) = &r.fidelity {
        let _ = writeln!(
            s,
            "  fidelity vs reference: min cos {:.5} · mean {:.5} · recall@1 {:.3} (reference {:.3})",
            f.min_cosine, f.mean_cosine, f.recall_at_1, f.reference_recall_at_1
        );
    }
    if r.machine.build_profile != "release" {
        let _ = writeln!(
            s,
            "  WARNING: debug build — not acceptance evidence (use --release)"
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> EmbedOptions {
        EmbedOptions {
            warmup: 2,
            iterations: 10,
            batch_sizes: vec![1, 4],
            docs: 8,
            doc_words: 20,
            long_words: 30,
            ..EmbedOptions::default()
        }
    }

    #[test]
    fn mock_run_produces_complete_report() {
        let report = run(&quick()).unwrap();
        assert_eq!(report.backend.name, "mock");
        assert_eq!(
            report.space,
            "lumen-mock-hash@1/pre1/embeddinggemma-retrieval@1/d256/l2"
        );
        assert_eq!(report.query.warm.n, 10);
        assert!(report.query.within_budget);
        assert_eq!(report.throughput.len(), 2);
        assert_eq!(report.throughput[1].per_batch.n, 2);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema_version"], 2);
        assert!(json["query"]["warm"]["p95_ms"].is_number());
        assert!(json["machine"]["build_profile"].is_string());
        assert!(json["query"]["long_input"]["p50_ms"].is_number());
        assert!(json["fidelity"].is_null());
    }

    #[test]
    fn mock_fidelity_runs_against_committed_reference() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut opts = quick();
        opts.corpus = root.join("fixtures/embedding/corpus.json");
        opts.reference = Some(root.join("fixtures/embedding/reference-eg2-onnx-fp32-d256.json"));
        let f = run(&opts).unwrap().fidelity.unwrap();
        assert_eq!(f.texts, 60);
        // The mock is not EmbeddingGemma: low cosine to the reference is expected.
        assert!(f.min_cosine < 0.5);
        assert!((f.reference_recall_at_1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn simulated_latency_shows_up_and_budget_can_fail() {
        let mut opts = quick();
        opts.iterations = 5;
        opts.batch_sizes = vec![];
        opts.mock_latency = MockLatency {
            per_call: Duration::from_millis(70),
            ..MockLatency::default()
        };
        let report = run(&opts).unwrap();
        assert!(report.query.warm.p50_ms >= 70.0);
        assert!(!report.query.within_budget);
        assert!(summarize(&report).contains("OVER"));
    }

    #[test]
    fn rejects_unknown_backend_and_bad_profile() {
        let mut opts = quick();
        opts.backend = "onnx-magic".into();
        assert!(run(&opts).unwrap_err().contains("unknown backend"));
        let mut opts = quick();
        opts.dim = 300;
        assert!(
            run(&opts)
                .unwrap_err()
                .contains("invalid embedding profile")
        );
    }
}
