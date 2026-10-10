//! Isolated synthetic device probe. This mode exits before starting Tauri or opening a DB.

use std::path::PathBuf;
use std::sync::Arc;

use lumen_embedding::policy::{DeviceProbe, ProbeMetrics, ProbeOutcome};
use lumen_embedding::probe::{ProbeConfig, ProbeCorpus, measure};
use lumen_embedding::{Embedder, EmbeddingProfile, EmbeddingTask, ExecutionTarget, TextInput};
use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig, init_runtime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Request {
    #[serde(default)]
    pub vision: Option<PathBuf>,
    pub key: String,
    pub model: PathBuf,
    pub runtime: PathBuf,
    pub variant: String,
    pub adapter: u32,
    pub name: String,
    pub total_mib: u64,
    pub threads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Metrics {
    pub query_p50_ms: f64,
    pub query_p95_ms: f64,
    pub index_chunks_per_s: f64,
    pub min_cosine_vs_cpu: Option<f64>,
    pub stable: bool,
    pub offloaded_fraction: Option<f64>,
    pub device_memory_mib: Option<f64>,
    pub device_memory_total_mib: Option<f64>,
}

impl From<ProbeMetrics> for Metrics {
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

impl Metrics {
    fn domain(&self) -> ProbeMetrics {
        ProbeMetrics {
            query_p50_ms: self.query_p50_ms,
            query_p95_ms: self.query_p95_ms,
            index_chunks_per_s: self.index_chunks_per_s,
            min_cosine_vs_cpu: self.min_cosine_vs_cpu,
            stable: self.stable,
            offloaded_fraction: self.offloaded_fraction,
            device_memory_mib: self.device_memory_mib,
            device_memory_total_mib: self.device_memory_total_mib,
        }
    }
    fn valid(&self) -> bool {
        [
            self.query_p50_ms,
            self.query_p95_ms,
            self.index_chunks_per_s,
        ]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
            && self
                .min_cosine_vs_cpu
                .is_none_or(|v| v.is_finite() && (-1.0..=1.0).contains(&v))
            && self
                .offloaded_fraction
                .is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && self
                .device_memory_mib
                .is_none_or(|v| v.is_finite() && v >= 0.0)
            && self
                .device_memory_total_mib
                .is_none_or(|v| v.is_finite() && v > 0.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Report {
    #[serde(default)]
    pub images: Option<ImageMetrics>,
    pub version: u32,
    pub key: String,
    pub space: String,
    pub adapter: u32,
    pub name: String,
    pub cpu: Metrics,
    pub gpu: Metrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ImageMetrics {
    pub legacy_cycle_ms: f64,
    pub accelerated_cycle_ms: f64,
    pub min_cosine: f64,
}

impl ImageMetrics {
    pub(crate) fn accepted(&self) -> bool {
        self.legacy_cycle_ms.is_finite()
            && self.legacy_cycle_ms > 0.0
            && self.accelerated_cycle_ms.is_finite()
            && self.accelerated_cycle_ms > 0.0
            && self.min_cosine.is_finite()
            && self.min_cosine >= 0.999
            && self.min_cosine <= 1.0
            && self.legacy_cycle_ms / self.accelerated_cycle_ms >= 1.15
    }
}

/// CPU vision encoder + validated GPU multimodal backbone. No files, captions or user DB.
fn measure_images(request: &Request) -> Result<ImageMetrics, String> {
    use lumen_embedding::{ImageInput, dot};
    use std::time::Instant;
    let vision = request.vision.as_ref().ok_or("vision absent")?;
    if request.variant != "q4" {
        return Err("q4 required".into());
    }
    let device = Device::DirectMl {
        adapter: request.adapter,
    };
    let mut reference = Vec::new();
    let mut cycles = [0.0; 2];
    let mut min_cosine = 1.0_f64;
    let text = "Synthetic text fidelity after visual inference";
    let mut text_reference = Vec::new();
    for mode in 0..3 {
        let text_device = if mode == 0 { Device::Cpu } else { device };
        let mut cfg = OrtConfig::new(&request.model, ModelVariant::Q4, text_device);
        cfg.vision_dir = Some(vision.clone());
        cfg.image_device = if mode == 2 { device } else { Device::Cpu };
        cfg.threads = Some(2);
        let e = Embedder::new(
            Arc::new(OrtBackend::new(cfg).map_err(|e| e.to_string())?),
            EmbeddingProfile::DEFAULT,
        )
        .map_err(|e| e.to_string())?;
        let before = e.embed_query(text, None).map_err(|e| e.to_string())?;
        if mode == 0 {
            text_reference = before;
        }
        for (index, (width, height)) in [(64, 48), (48, 64)].into_iter().enumerate() {
            let rgb: Vec<u8> = (0..width * height * 3)
                .map(|i| u8::try_from((i * 17 + index as u32 * 31) % 256).unwrap_or(0))
                .collect();
            // Match the text probe's steady-state comparison. DirectML specializes
            // the visual shape on first use; charge that startup once, outside the
            // repeated image -> text cycle, on all three routes equally.
            e.embed_images(
                &[ImageInput {
                    width,
                    height,
                    rgb: &rgb,
                }],
                None,
            )
            .map_err(|e| e.to_string())?;
            e.embed_query(text, None).map_err(|e| e.to_string())?;
            let start = Instant::now();
            let v = e
                .embed_images(
                    &[ImageInput {
                        width,
                        height,
                        rgb: &rgb,
                    }],
                    None,
                )
                .map_err(|e| e.to_string())?
                .into_flat();
            let after = e.embed_query(text, None).map_err(|e| e.to_string())?;
            if mode == 0 {
                reference.push(v);
            } else {
                cycles[mode - 1] += start.elapsed().as_secs_f64() * 1000.0;
                if mode == 2 {
                    min_cosine = min_cosine
                        .min(f64::from(dot(&reference[index], &v)))
                        .min(f64::from(dot(&text_reference, &after)));
                }
            }
        }
    }
    Ok(ImageMetrics {
        legacy_cycle_ms: cycles[0],
        accelerated_cycle_ms: cycles[1],
        min_cosine,
    })
}

impl Report {
    pub(crate) fn valid_for(&self, key: &str, adapter: u32) -> bool {
        self.version == 1
            && self.key == key
            && self.adapter == adapter
            && !self.space.is_empty()
            && self.cpu.valid()
            && self.gpu.valid()
    }
    pub(crate) fn probes(&self) -> [DeviceProbe; 2] {
        let make = |device: String, target, metrics: &Metrics| DeviceProbe {
            device,
            target,
            integrated: false,
            space_key: self.space.clone(),
            runtime_key: self.key.clone(),
            outcome: ProbeOutcome::Measured(metrics.domain()),
        };
        [
            make("cpu".into(), ExecutionTarget::Cpu, &self.cpu),
            make(
                format!("dml:{}", self.adapter),
                ExecutionTarget::Gpu,
                &self.gpu,
            ),
        ]
    }
}

pub(crate) fn build(request: &Request, device: Device, threads: usize) -> Result<Embedder, String> {
    init_runtime(&request.runtime).map_err(|e| e.to_string())?;
    let variant = ModelVariant::parse(&request.variant).ok_or("unsupported embedding variant")?;
    let mut cfg = OrtConfig::new(&request.model, variant, device);
    cfg.threads = Some(threads);
    cfg.max_batch = 8;
    let backend = OrtBackend::new(cfg).map_err(|e| e.to_string())?;
    Embedder::new(Arc::new(backend), EmbeddingProfile::DEFAULT).map_err(|e| e.to_string())
}

/// Small, varied, approximately 128-token inputs; no indexed user content.
pub(crate) fn run(request: &Request) -> Result<Report, String> {
    if !lumen_windows::gpu::dedicated_gpus()?
        .iter()
        .any(|gpu| gpu.adapter == request.adapter)
    {
        return Err("Probe requires a dedicated hardware GPU".into());
    }
    let docs: Vec<String> = [
        "source code and retries",
        "holiday travel and photos",
        "monthly invoices and payments",
        "project meeting notes",
        "network configuration",
        "cooking recipes",
        "local file search",
        "Windows keyboard actions",
    ]
    .iter()
    .map(|topic| {
        format!(
            "A synthetic document about {topic}. {}",
            "This local example describes a useful task with several details, steps and results. "
                .repeat(7)
        )
    })
    .collect();
    let refs: Vec<&str> = docs.iter().map(String::as_str).collect();
    let corpus = ProbeCorpus {
        queries: &[
            "retry network requests",
            "monthly payments",
            "travel plans",
            "keyboard shortcuts",
        ],
        documents: &refs,
    };
    let cfg = ProbeConfig {
        warmup_queries: 2,
        measured_queries: 8,
    };
    let cpu = build(request, Device::Cpu, request.threads)?;
    let space = cpu.space().key();
    let doc_inputs: Vec<_> = refs.iter().map(|s| TextInput::new(s)).collect();
    // DirectML specializes dynamic document shapes on their first use. Compare the
    // warm bulk lane on both devices, rather than rejecting a faster steady-state GPU
    // because one measured batch included compilation/shape initialization.
    cpu.embed(EmbeddingTask::SearchDocument, &doc_inputs, None)
        .map_err(|e| e.to_string())?;
    let (mut cpu_metrics, vectors) =
        measure(&cpu, &corpus, None, &cfg).map_err(|e| e.to_string())?;
    cpu_metrics.min_cosine_vs_cpu = Some(1.0);
    drop(cpu);
    // Placement is diagnostic only and drops its throwaway session before inference.
    let variant = ModelVariant::parse(&request.variant).ok_or("unsupported variant")?;
    let diagnostic = OrtBackend::new(OrtConfig::new(
        &request.model,
        variant,
        Device::DirectMl {
            adapter: request.adapter,
        },
    ))
    .map_err(|e| e.to_string())?;
    let placement = diagnostic.placement().map_err(|e| e.to_string())?;
    drop(diagnostic);
    let gpu = build(
        request,
        Device::DirectMl {
            adapter: request.adapter,
        },
        1,
    )?;
    if gpu.space().key() != space {
        return Err("GPU model space does not match CPU".into());
    }
    gpu.embed(EmbeddingTask::SearchDocument, &doc_inputs, None)
        .map_err(|e| e.to_string())?;
    let (mut gpu_metrics, _) =
        measure(&gpu, &corpus, Some(&vectors), &cfg).map_err(|e| e.to_string())?;
    gpu_metrics.offloaded_fraction = placement.offloaded_fraction();
    gpu_metrics.device_memory_mib = lumen_windows::gpu::memory_mib(request.adapter);
    #[allow(clippy::cast_precision_loss)]
    {
        gpu_metrics.device_memory_total_mib = Some(request.total_mib as f64);
    }
    drop(gpu);
    let images = if request.vision.is_some() {
        measure_images(request).ok()
    } else {
        None
    };
    Ok(Report {
        images,
        version: 1,
        key: request.key.clone(),
        space,
        adapter: request.adapter,
        name: request.name.clone(),
        cpu: cpu_metrics.into(),
        gpu: gpu_metrics.into(),
    })
}

pub(crate) fn child_mode() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != "--gpu-probe") {
        return None;
    }
    let result = (|| {
        if args.len() != 4 {
            return Err("gpu probe requires request and output paths".to_owned());
        }
        if std::fs::metadata(&args[2])
            .map_err(|e| e.to_string())?
            .len()
            > 65_536
        {
            return Err("gpu probe request too large".to_owned());
        }
        let input = std::fs::read(&args[2]).map_err(|e| e.to_string())?;
        let request: Request = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
        let report = run(&request)?;
        std::fs::write(
            &args[3],
            serde_json::to_vec(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("lumen: GPU probe failed: {error}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_reports_reject_old_runtime_adapter_and_invalid_metrics() {
        let metrics = Metrics {
            query_p50_ms: 30.0,
            query_p95_ms: 40.0,
            index_chunks_per_s: 8.0,
            min_cosine_vs_cpu: Some(1.0),
            stable: true,
            offloaded_fraction: Some(0.95),
            device_memory_mib: Some(2300.0),
            device_memory_total_mib: Some(4096.0),
        };
        let mut report = Report {
            images: None,
            version: 1,
            key: "driver-runtime-model".into(),
            space: "q4-space".into(),
            adapter: 1,
            name: "discrete GPU".into(),
            cpu: metrics.clone(),
            gpu: metrics,
        };
        assert!(report.valid_for("driver-runtime-model", 1));
        assert!(!report.valid_for("changed-driver", 1));
        assert!(!report.valid_for("driver-runtime-model", 0));
        report.gpu.index_chunks_per_s = f64::INFINITY;
        assert!(!report.valid_for("driver-runtime-model", 1));
        report.gpu.index_chunks_per_s = 8.0;
        report.gpu.offloaded_fraction = Some(2.0);
        assert!(!report.valid_for("driver-runtime-model", 1));
        report.gpu.offloaded_fraction = Some(0.95);
        report.version = 0;
        assert!(!report.valid_for("driver-runtime-model", 1));
    }

    #[test]
    fn image_gate_requires_fidelity_and_measured_mixed_work_speedup() {
        let mut m = ImageMetrics {
            legacy_cycle_ms: 12000.0,
            accelerated_cycle_ms: 9000.0,
            min_cosine: 0.999999,
        };
        assert!(m.accepted());
        m.min_cosine = 0.998;
        assert!(!m.accepted());
        m.min_cosine = 1.0;
        m.accelerated_cycle_ms = 11900.0;
        assert!(!m.accepted());
        m.accelerated_cycle_ms = f64::NAN;
        assert!(!m.accepted());
    }
}
