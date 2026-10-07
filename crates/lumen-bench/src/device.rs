//! `lumen-bench probe` and `lumen-bench device-policy` (T013, ADR-019).
//!
//! `probe` measures one device through the production `Embedder` and writes a JSON probe;
//! run it once per device, CPU first (`--save-vectors`), then accelerators with
//! `--cpu-vectors` so their vectors are compared with the CPU's. One process per device:
//! a device hang (T006, iGPU) cannot take the other measurements down.
//!
//! `device-policy` loads the probes and prints the decision of
//! `lumen_embedding::policy::plan` for a matrix of power/profile/activity states.

use std::path::PathBuf;
use std::sync::Arc;

use lumen_embedding::policy::{
    self, CPU_DEVICE, DevicePlan, DeviceProbe, IndexingPlan, PolicyConfig, PowerSource,
    ProbeMetrics, ProbeOutcome, Quarantine, ResourceProfile, SystemState,
};
use lumen_embedding::probe::{self, ProbeConfig, ProbeCorpus, ProbeVectors};
use lumen_embedding::{Embedder, EmbeddingBatch, EmbeddingProfile, ExecutionTarget};
use serde::{Deserialize, Serialize};

use crate::corpus;
use crate::embed::{self, EmbedOptions};
use crate::machine::MachineInfo;

/// Documents in the throughput sample (~260 tokens each, like T006).
const PROBE_DOCS: usize = 32;
const PROBE_DOC_WORDS: usize = 200;

#[derive(Debug, Clone, Default)]
pub(crate) struct ProbeOptions {
    pub(crate) embed: EmbedOptions,
    /// Policy device id; default = `--device` (ort) or `cpu`.
    pub(crate) device_id: Option<String>,
    pub(crate) integrated: bool,
    pub(crate) runtime_key: Option<String>,
    pub(crate) cpu_vectors: Option<PathBuf>,
    pub(crate) save_vectors: Option<PathBuf>,
    pub(crate) device_memory_mib: Option<f64>,
    pub(crate) device_memory_total_mib: Option<f64>,
    pub(crate) measured_queries: usize,
}

/// Wire form of [`DeviceProbe`] (the core crate has no serde, ADR-013).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProbeFile {
    pub(crate) schema_version: u32,
    pub(crate) kind: String,
    pub(crate) label: Option<String>,
    pub(crate) machine: Option<serde_json::Value>,
    pub(crate) device: String,
    pub(crate) target: String,
    pub(crate) integrated: bool,
    pub(crate) space_key: String,
    pub(crate) runtime_key: String,
    pub(crate) failed: Option<String>,
    pub(crate) metrics: Option<MetricsDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct MetricsDto {
    query_p50_ms: f64,
    query_p95_ms: f64,
    index_chunks_per_s: f64,
    min_cosine_vs_cpu: Option<f64>,
    stable: bool,
    offloaded_fraction: Option<f64>,
    device_memory_mib: Option<f64>,
    device_memory_total_mib: Option<f64>,
}

impl From<ProbeMetrics> for MetricsDto {
    fn from(m: ProbeMetrics) -> Self {
        Self {
            query_p50_ms: m.query_p50_ms,
            query_p95_ms: m.query_p95_ms,
            index_chunks_per_s: m.index_chunks_per_s,
            min_cosine_vs_cpu: m.min_cosine_vs_cpu,
            stable: m.stable,
            offloaded_fraction: m.offloaded_fraction,
            device_memory_mib: m.device_memory_mib,
            device_memory_total_mib: m.device_memory_total_mib,
        }
    }
}

impl From<MetricsDto> for ProbeMetrics {
    fn from(m: MetricsDto) -> Self {
        Self {
            query_p50_ms: m.query_p50_ms,
            query_p95_ms: m.query_p95_ms,
            index_chunks_per_s: m.index_chunks_per_s,
            min_cosine_vs_cpu: m.min_cosine_vs_cpu,
            stable: m.stable,
            offloaded_fraction: m.offloaded_fraction,
            device_memory_mib: m.device_memory_mib,
            device_memory_total_mib: m.device_memory_total_mib,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct VectorsFile {
    space_key: String,
    dim: usize,
    queries: Vec<f32>,
    documents: Vec<f32>,
}

fn target_name(t: ExecutionTarget) -> &'static str {
    t.as_str()
}

fn parse_target(s: &str) -> ExecutionTarget {
    match s {
        "cpu" => ExecutionTarget::Cpu,
        "gpu" => ExecutionTarget::Gpu,
        "npu" => ExecutionTarget::Npu,
        _ => ExecutionTarget::Other,
    }
}

impl ProbeFile {
    pub(crate) fn to_probe(&self) -> DeviceProbe {
        DeviceProbe {
            device: self.device.clone(),
            target: parse_target(&self.target),
            integrated: self.integrated,
            space_key: self.space_key.clone(),
            runtime_key: self.runtime_key.clone(),
            outcome: match (&self.failed, self.metrics) {
                (None, Some(m)) => ProbeOutcome::Measured(m.into()),
                (Some(why), _) => ProbeOutcome::Failed(why.clone()),
                (None, None) => ProbeOutcome::Failed("probe file has no metrics".into()),
            },
        }
    }
}

/// Runs one probe. Device/runtime failures become a probe with `failed` set (exit 0), so a
/// matrix script keeps going; only usage errors are `Err`.
///
/// # Errors
/// Unreadable `--cpu-vectors`, unwritable `--save-vectors`.
pub(crate) fn run_probe(opts: &ProbeOptions) -> Result<ProbeFile, String> {
    let device = opts.device_id.clone().unwrap_or_else(|| {
        opts.embed
            .ort
            .device
            .clone()
            .unwrap_or_else(|| CPU_DEVICE.to_owned())
    });
    let reference = opts
        .cpu_vectors
        .as_ref()
        .map(|p| -> Result<(String, ProbeVectors), String> {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let text = text.trim_start_matches('\u{feff}');
            let f: VectorsFile =
                serde_json::from_str(text).map_err(|e| format!("{}: {e}", p.display()))?;
            let batch = |v: Vec<f32>| {
                EmbeddingBatch::try_from_flat(f.dim, v)
                    .ok_or_else(|| format!("{}: malformed vectors", p.display()))
            };
            Ok((
                f.space_key.clone(),
                ProbeVectors {
                    queries: batch(f.queries.clone())?,
                    documents: batch(f.documents.clone())?,
                },
            ))
        })
        .transpose()?;

    let docs: Vec<String> = (0..PROBE_DOCS)
        .map(|i| corpus::synthetic_document(i as u64, PROBE_DOC_WORDS))
        .collect();
    let doc_refs: Vec<&str> = docs.iter().map(String::as_str).collect();
    let probe_corpus = ProbeCorpus {
        queries: corpus::QUERIES,
        documents: &doc_refs,
    };

    let mut out = ProbeFile {
        schema_version: 1,
        kind: "probe".into(),
        label: opts.embed.label.clone(),
        machine: serde_json::to_value(MachineInfo::collect()).ok(),
        device: device.clone(),
        target: if device == CPU_DEVICE { "cpu" } else { "gpu" }.into(),
        integrated: opts.integrated,
        space_key: String::new(),
        runtime_key: String::new(),
        failed: None,
        metrics: None,
    };

    let built = embed::make_backend(&opts.embed.backend, &opts.embed).and_then(|(b, diag)| {
        let profile = EmbeddingProfile {
            dim: opts.embed.dim,
            ..EmbeddingProfile::DEFAULT
        };
        Embedder::new(Arc::clone(&b), profile)
            .map(|e| (e, diag))
            .map_err(|e| e.to_string())
    });
    let (embedder, diagnostics) = match built {
        Ok(x) => x,
        Err(why) => {
            out.failed = Some(why);
            return Ok(out);
        }
    };
    let caps = embedder.backend().capabilities();
    out.target = target_name(caps.target).into();
    out.space_key = embedder.space().key();
    out.runtime_key = opts.runtime_key.clone().unwrap_or_else(|| {
        format!(
            "{}-{}/{device}",
            caps.backend,
            caps.runtime_version.as_deref().unwrap_or("unknown")
        )
    });

    let reference_vectors = match &reference {
        Some((space, v)) if *space == out.space_key => Some(v),
        Some((space, _)) => {
            out.failed = Some(format!(
                "--cpu-vectors were made in space {space}, this probe runs {}",
                out.space_key
            ));
            return Ok(out);
        }
        None => None,
    };
    let cfg = ProbeConfig {
        measured_queries: opts.measured_queries.max(1),
        ..ProbeConfig::default()
    };
    match probe::measure(&embedder, &probe_corpus, reference_vectors, &cfg) {
        Ok((mut m, vectors)) => {
            if device == CPU_DEVICE {
                m.min_cosine_vs_cpu = Some(1.0);
            }
            m.offloaded_fraction = diagnostics
                .pointer("/placement/offloaded_fraction")
                .and_then(serde_json::Value::as_f64);
            m.device_memory_mib = opts.device_memory_mib;
            m.device_memory_total_mib = opts.device_memory_total_mib;
            out.metrics = Some(m.into());
            if let Some(path) = &opts.save_vectors {
                let file = VectorsFile {
                    space_key: out.space_key.clone(),
                    dim: vectors.queries.dim(),
                    queries: vectors.queries.as_flat().to_vec(),
                    documents: vectors.documents.as_flat().to_vec(),
                };
                let json = serde_json::to_string(&file).map_err(|e| e.to_string())?;
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        Err(e) => out.failed = Some(e.to_string()),
    }
    Ok(out)
}

pub(crate) fn summarize_probe(p: &ProbeFile) -> String {
    match (&p.failed, &p.metrics) {
        (Some(why), _) => format!("probe {}: FAILED: {why}\n", p.device),
        (None, Some(m)) => format!(
            "probe {} ({}): query p50/p95 {:.1}/{:.1} ms, indexing {:.2} chunks/s, \
             stable={}, cos vs cpu={}, offloaded={}, device mem={}\n  space {}\n",
            p.device,
            p.target,
            m.query_p50_ms,
            m.query_p95_ms,
            m.index_chunks_per_s,
            m.stable,
            m.min_cosine_vs_cpu
                .map_or_else(|| "n/a".into(), |c| format!("{c:.5}")),
            m.offloaded_fraction
                .map_or_else(|| "unknown".into(), |f| format!("{:.0}%", f * 100.0)),
            m.device_memory_mib
                .map_or_else(|| "unknown".into(), |v| format!("{v:.0} MiB")),
            p.space_key
        ),
        (None, None) => format!("probe {}: no result\n", p.device),
    }
}

// ---- device-policy --------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(crate) struct ScenarioPlan {
    scenario: &'static str,
    query_device: String,
    indexing: String,
    rejected: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PolicyReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    space_key: String,
    devices: Vec<String>,
    logical_cpus: usize,
    plans: Vec<ScenarioPlan>,
}

fn scenarios(logical_cpus: usize) -> Vec<(&'static str, SystemState)> {
    let base = SystemState {
        power: PowerSource::Ac,
        profile: ResourceProfile::Balanced,
        logical_cpus,
        available_memory_mib: Some(8192),
        user_active: false,
    };
    vec![
        ("balanced, AC, idle", base),
        (
            "balanced, AC, user active",
            SystemState {
                user_active: true,
                ..base
            },
        ),
        (
            "balanced, battery 70%",
            SystemState {
                power: PowerSource::Battery { percent: Some(70) },
                ..base
            },
        ),
        (
            "balanced, battery 15%",
            SystemState {
                power: PowerSource::Battery { percent: Some(15) },
                ..base
            },
        ),
        (
            "eco, AC",
            SystemState {
                profile: ResourceProfile::Eco,
                ..base
            },
        ),
        (
            "turbo, battery 70%",
            SystemState {
                profile: ResourceProfile::Turbo,
                power: PowerSource::Battery { percent: Some(70) },
                ..base
            },
        ),
        (
            "balanced, AC, low memory",
            SystemState {
                available_memory_mib: Some(400),
                ..base
            },
        ),
    ]
}

fn describe(plan: &DevicePlan) -> (String, Vec<String>) {
    let indexing = match &plan.indexing {
        IndexingPlan::Run { device, threads } if device == CPU_DEVICE => {
            format!("{device} x{threads} threads")
        }
        IndexingPlan::Run { device, .. } => device.clone(),
        IndexingPlan::Paused(why) => format!("paused ({why:?})"),
    };
    let rejected = plan
        .rejected
        .iter()
        .map(|(d, r)| format!("{d}: {r}"))
        .collect();
    (indexing, rejected)
}

/// # Errors
/// Unreadable probe files, no probes.
pub(crate) fn run_policy(
    files: &[PathBuf],
    space: Option<String>,
    label: Option<String>,
) -> Result<PolicyReport, String> {
    if files.is_empty() {
        return Err("device-policy needs at least one --probe FILE".into());
    }
    let mut probes = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // PowerShell 5.1 writes UTF-8 with a BOM.
        let text = text.trim_start_matches('\u{feff}');
        let f: ProbeFile =
            serde_json::from_str(text).map_err(|e| format!("{}: {e}", path.display()))?;
        probes.push(f.to_probe());
    }
    let space_key = space
        .or_else(|| {
            probes
                .iter()
                .find(|p| p.device == CPU_DEVICE && !p.space_key.is_empty())
                .map(|p| p.space_key.clone())
        })
        .ok_or("no CPU probe with a space key; pass --space")?;
    let logical_cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let plans = scenarios(logical_cpus)
        .into_iter()
        .map(|(scenario, state)| {
            let plan = policy::plan(
                &space_key,
                &probes,
                &Quarantine::new(),
                &state,
                &PolicyConfig::default(),
            );
            let (indexing, rejected) = describe(&plan);
            ScenarioPlan {
                scenario,
                query_device: plan.query_device,
                indexing,
                rejected,
            }
        })
        .collect();
    Ok(PolicyReport {
        schema_version: 1,
        kind: "device-policy",
        label,
        space_key,
        devices: probes.iter().map(|p| p.device.clone()).collect(),
        logical_cpus,
        plans,
    })
}

pub(crate) fn summarize_policy(r: &PolicyReport) -> String {
    use std::fmt::Write as _;
    let mut s = format!(
        "device-policy: devices {:?}, {} logical CPUs\n  space {}\n",
        r.devices, r.logical_cpus, r.space_key
    );
    for p in &r.plans {
        let _ = writeln!(
            s,
            "  {:<28} query={:<8} indexing={}",
            p.scenario, p.query_device, p.indexing
        );
    }
    if let Some(first) = r.plans.first() {
        for why in &first.rejected {
            let _ = writeln!(s, "    not used: {why}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("lumen-bench-device-{}-{name}", std::process::id()))
    }

    #[test]
    fn mock_probes_round_trip_into_a_cpu_plan() {
        let vectors = tmp("cpu-vectors.json");
        let cpu = run_probe(&ProbeOptions {
            embed: EmbedOptions::default(),
            save_vectors: Some(vectors.clone()),
            measured_queries: 5,
            ..ProbeOptions::default()
        })
        .unwrap();
        assert!(cpu.failed.is_none(), "{:?}", cpu.failed);
        assert_eq!(cpu.metrics.unwrap().min_cosine_vs_cpu, Some(1.0));

        // A "GPU" running the same mock: identical vectors, but no placement/memory info.
        let gpu = run_probe(&ProbeOptions {
            embed: EmbedOptions::default(),
            device_id: Some("dml:0".into()),
            cpu_vectors: Some(vectors.clone()),
            measured_queries: 5,
            ..ProbeOptions::default()
        })
        .unwrap();
        assert!(gpu.metrics.unwrap().min_cosine_vs_cpu.unwrap() > 0.99999);

        let (a, b) = (tmp("cpu.json"), tmp("gpu.json"));
        // With a BOM, as PowerShell 5.1 writes it.
        std::fs::write(
            &a,
            format!("\u{feff}{}", serde_json::to_string(&cpu).unwrap()),
        )
        .unwrap();
        std::fs::write(&b, serde_json::to_string(&gpu).unwrap()).unwrap();
        let report = run_policy(&[a.clone(), b.clone()], None, None).unwrap();
        assert_eq!(report.plans.len(), 7);
        assert!(report.plans.iter().all(|p| p.query_device == CPU_DEVICE));
        // Unknown placement keeps the accelerator off and says why.
        assert!(report.plans[0].rejected[0].contains("placement unknown"));
        assert!(summarize_policy(&report).contains("low memory"));
        for p in [vectors, a, b] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn failed_backend_is_a_failed_probe_not_an_error() {
        let p = run_probe(&ProbeOptions {
            embed: EmbedOptions {
                backend: "nope".into(),
                ..EmbedOptions::default()
            },
            ..ProbeOptions::default()
        })
        .unwrap();
        assert!(p.failed.is_some());
        assert!(summarize_probe(&p).contains("FAILED"));
        assert!(matches!(p.to_probe().outcome, ProbeOutcome::Failed(_)));
    }
}
