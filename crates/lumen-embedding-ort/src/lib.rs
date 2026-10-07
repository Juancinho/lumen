//! ONNX Runtime backend for EmbeddingGemma 2 (T006).
//!
//! - Runs the `onnx-community/embeddinggemma-2-ONNX` text graph (`onnx/<variant>.onnx`):
//!   inputs `input_ids`, `attention_mask` (i64, `[batch, seq]`) plus empty
//!   `image_features`/`video_features`/`audio_features` (`[0, 512]` f32) for text-only use;
//!   output `sentence_embedding` (`[batch, 768]`, mean-pooled, unit length).
//! - Tokenizes with the model's `tokenizer.json` (adds `<bos>`/`<eos>`; right padding with id 0).
//! - ONNX Runtime is loaded **dynamically** from `onnxruntime.dll`/`.so` shipped next to the
//!   app, so the CPU build or the DirectML build can be chosen at install time (ADR-015).
//! - Devices: CPU everywhere; DirectML (any DX12 GPU) with the `directml` feature on Windows.
//!   EP registration failures are errors, never a silent CPU fallback, so benchmarks and the
//!   device-selection policy (T013) see the truth.
//!
//! Prompt formatting, truncation to the profile dimension and renormalization happen in
//! `lumen_embedding::Embedder`; this backend returns raw 768d vectors.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use lumen_embedding::{
    Capabilities, EmbeddingBackend, EmbeddingError, ExecutionTarget, Modality, ModalitySet,
    ModelInfo,
};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use tokenizers::Tokenizer;

/// Native output dimension of EmbeddingGemma 2.
pub const NATIVE_DIM: usize = 768;
/// Width of the (unused, empty) multimodal feature inputs of the text graph.
const FEATURE_WIDTH: usize = 512;
/// EmbeddingGemma 2 context limit (tokens, all modalities).
pub const MAX_INPUT_TOKENS: usize = 8192;

/// Which ONNX file of the export to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelVariant {
    /// `model.onnx` — fp32 weights (1.08 GB).
    Fp32,
    /// `model_fp16.onnx` — fp16 weights; for GPUs.
    Fp16,
    /// `model_quantized.onnx` — 8-bit `MatMulNBits` (314 MB).
    Q8,
    /// `model_q4.onnx` — 4-bit `MatMulNBits` (174 MB).
    Q4,
    /// `model_q4f16.onnx` — 4-bit weights, fp16 activations; for GPUs.
    Q4F16,
}

impl ModelVariant {
    pub const ALL: [Self; 5] = [Self::Fp32, Self::Fp16, Self::Q8, Self::Q4, Self::Q4F16];

    /// File stem inside `onnx/`.
    #[must_use]
    pub const fn file_stem(self) -> &'static str {
        match self {
            Self::Fp32 => "model",
            Self::Fp16 => "model_fp16",
            Self::Q8 => "model_quantized",
            Self::Q4 => "model_q4",
            Self::Q4F16 => "model_q4f16",
        }
    }

    /// Short name used in CLIs, reports and the model revision.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fp32 => "fp32",
            Self::Fp16 => "fp16",
            Self::Q8 => "q8",
            Self::Q4 => "q4",
            Self::Q4F16 => "q4f16",
        }
    }

    /// Parses [`ModelVariant::name`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == s)
    }
}

/// Where to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    Cpu,
    /// DirectML adapter index (DXGI enumeration order). Requires the `directml` feature
    /// and a DirectML build of ONNX Runtime.
    DirectMl {
        adapter: u32,
    },
    /// DirectML GPU chosen by preference (portable across machines).
    DirectMlPreferred(GpuPreference),
}

/// Which GPU DirectML should pick when no adapter index is given (hybrid laptops have
/// an integrated and a discrete GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPreference {
    /// Discrete/most capable GPU (plugged in, Performance profile).
    HighPerformance,
    /// Integrated/most power-efficient GPU (battery, Balanced profile).
    MinimumPower,
}

impl Device {
    /// Parses `cpu`, `dml:<adapter>`, `dml:high`, `dml:low`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "dml:high" => Some(Self::DirectMlPreferred(GpuPreference::HighPerformance)),
            "dml:low" => Some(Self::DirectMlPreferred(GpuPreference::MinimumPower)),
            other => other
                .strip_prefix("dml:")
                .and_then(|n| n.parse().ok())
                .map(|adapter| Self::DirectMl { adapter }),
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("cpu"),
            Self::DirectMl { adapter } => write!(f, "dml:{adapter}"),
            Self::DirectMlPreferred(GpuPreference::HighPerformance) => f.write_str("dml:high"),
            Self::DirectMlPreferred(GpuPreference::MinimumPower) => f.write_str("dml:low"),
        }
    }
}

/// Backend configuration.
#[derive(Debug, Clone)]
pub struct OrtConfig {
    /// Local copy of the export: `tokenizer.json` and `onnx/<variant>.onnx[_data]`.
    pub model_dir: PathBuf,
    pub variant: ModelVariant,
    pub device: Device,
    /// Intra-op threads for CPU (`None` = ONNX Runtime default: physical cores).
    pub threads: Option<usize>,
    /// Inputs per backend call (the `Embedder` splits larger requests).
    pub max_batch: usize,
    /// Token cap per input; longer inputs are truncated (chunkers stay far below this).
    pub max_tokens: usize,
    /// Allow ONNX Runtime to run nodes the GPU EP cannot take on the CPU EP. ORT always
    /// keeps some shape/index nodes on CPU on purpose, so `false` makes DirectML sessions
    /// fail; use [`OrtBackend::placement`] to see what actually runs where.
    pub cpu_fallback: bool,
}

impl OrtConfig {
    #[must_use]
    pub fn new(model_dir: impl Into<PathBuf>, variant: ModelVariant, device: Device) -> Self {
        Self {
            model_dir: model_dir.into(),
            variant,
            device,
            threads: None,
            max_batch: 16,
            max_tokens: 2048,
            cpu_fallback: true,
        }
    }

    fn onnx_path(&self) -> PathBuf {
        self.model_dir
            .join("onnx")
            .join(format!("{}.onnx", self.variant.file_stem()))
    }
}

/// Which execution provider ONNX Runtime assigned graph nodes to (from its verbose
/// session log). Empty `nodes_per_provider` means the log format was not recognized.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placement {
    /// e.g. `{"DmlExecutionProvider": 512, "CPUExecutionProvider": 23}`.
    pub nodes_per_provider: BTreeMap<String, usize>,
    /// Op types of the nodes placed on the CPU EP, with counts, when listed in the log.
    pub cpu_op_types: BTreeMap<String, usize>,
}

impl Placement {
    /// Parses ORT's `Node(s) placed on [EP]. Number of nodes: N` / `All nodes placed on [EP].
    /// Number of nodes: N` messages and the per-node lines (`  OpType (node_name)`) that follow.
    #[must_use]
    pub fn parse(messages: &[String]) -> Self {
        let mut out = Self::default();
        let mut current: Option<String> = None;
        for message in messages {
            for line in message.lines() {
                let trimmed = line.trim();
                if let Some((provider, count)) = parse_placement_header(trimmed) {
                    *out.nodes_per_provider.entry(provider.clone()).or_default() += count;
                    current = Some(provider);
                } else if trimmed.is_empty() || trimmed == "Node placements" {
                    // separators
                } else if let Some(provider) = &current {
                    if provider == "CPUExecutionProvider"
                        && let Some(op) = trimmed.split_whitespace().next()
                        && trimmed.ends_with(')')
                    {
                        *out.cpu_op_types.entry(op.to_owned()).or_default() += 1;
                    } else if !trimmed.ends_with(')') {
                        current = None;
                    }
                }
            }
        }
        out
    }

    /// Fraction of nodes not on the CPU EP (1.0 = fully offloaded), if known.
    #[must_use]
    pub fn offloaded_fraction(&self) -> Option<f64> {
        let total: usize = self.nodes_per_provider.values().sum();
        if total == 0 {
            return None;
        }
        let cpu = self
            .nodes_per_provider
            .get("CPUExecutionProvider")
            .copied()
            .unwrap_or(0);
        #[allow(clippy::cast_precision_loss)]
        Some((total - cpu) as f64 / total as f64)
    }
}

fn parse_placement_header(line: &str) -> Option<(String, usize)> {
    let rest = line
        .strip_prefix("Node(s) placed on [")
        .or_else(|| line.strip_prefix("All nodes placed on ["))?;
    let (provider, tail) = rest.split_once(']')?;
    let count = tail
        .rsplit(':')
        .next()?
        .trim()
        .trim_end_matches('.')
        .parse()
        .ok()?;
    Some((provider.to_owned(), count))
}

static RUNTIME: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Loads ONNX Runtime from `dylib` (e.g. `onnxruntime.dll` next to the executable). Must be
/// called once per process before creating an [`OrtBackend`]; later calls with the same path
/// are no-ops, calls with a different path fail (one runtime per process).
///
/// # Errors
/// The library cannot be loaded, is too old for API level 24, or a different library was
/// already loaded.
pub fn init_runtime(dylib: &Path) -> Result<(), EmbeddingError> {
    let result = RUNTIME.get_or_init(|| {
        ort::init_from(dylib)
            .map(|builder| {
                builder.with_name("lumen").commit();
                dylib.to_path_buf()
            })
            .map_err(|e| format!("load {}: {e}", dylib.display()))
    });
    match result {
        Ok(loaded) if loaded == dylib => Ok(()),
        Ok(loaded) => Err(EmbeddingError::Backend(format!(
            "ONNX Runtime already loaded from {}",
            loaded.display()
        ))),
        Err(e) => Err(EmbeddingError::Backend(e.clone())),
    }
}

fn backend_err(context: &str, err: impl fmt::Display) -> EmbeddingError {
    EmbeddingError::Backend(format!("{context}: {err}"))
}

/// EmbeddingGemma 2 on ONNX Runtime.
pub struct OrtBackend {
    config: OrtConfig,
    caps: Capabilities,
    tokenizer: Tokenizer,
    /// `None` while unloaded. `Session::run` needs `&mut`, so calls serialize here
    /// (`concurrent_calls = false`); T204 gives queries priority over indexing.
    session: Mutex<Option<Session>>,
    /// Wall time of the last session creation (model load + EP compile), for reports.
    last_load_ms: Mutex<Option<f64>>,
}

impl fmt::Debug for OrtBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrtBackend")
            .field("variant", &self.config.variant)
            .field("device", &self.config.device)
            .finish_non_exhaustive()
    }
}

impl OrtBackend {
    /// Loads the tokenizer and validates files. The ONNX session is created lazily
    /// (`warm`/first call). [`init_runtime`] must have succeeded.
    ///
    /// # Errors
    /// Missing files, unreadable tokenizer, runtime not initialized, invalid config.
    pub fn new(config: OrtConfig) -> Result<Self, EmbeddingError> {
        if RUNTIME.get().is_none_or(Result::is_err) {
            return Err(EmbeddingError::Backend(
                "ONNX Runtime not initialized: call init_runtime first".into(),
            ));
        }
        if config.max_batch == 0 || config.max_tokens < 3 {
            return Err(EmbeddingError::Backend(
                "max_batch must be >= 1 and max_tokens >= 3".into(),
            ));
        }
        let onnx = config.onnx_path();
        if !onnx.is_file() {
            return Err(EmbeddingError::Backend(format!(
                "model file not found: {}",
                onnx.display()
            )));
        }
        let mut tokenizer = Tokenizer::from_file(config.model_dir.join("tokenizer.json"))
            .map_err(|e| backend_err("load tokenizer.json", e))?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: config.max_tokens,
                ..tokenizers::TruncationParams::default()
            }))
            .map_err(|e| backend_err("configure truncation", e))?;
        tokenizer.with_padding(None);

        let (target, device) = match config.device {
            Device::Cpu => (ExecutionTarget::Cpu, None),
            Device::DirectMl { adapter } => (
                ExecutionTarget::Gpu,
                Some(format!("DirectML adapter {adapter}")),
            ),
            Device::DirectMlPreferred(pref) => {
                (ExecutionTarget::Gpu, Some(format!("DirectML {pref:?} GPU")))
            }
        };
        let caps = Capabilities {
            backend: format!("onnxruntime-{}", config.device),
            runtime_version: Some(ort::info().to_owned()),
            model: ModelInfo {
                id: "embeddinggemma-2".into(),
                // Weights differ per variant: different spaces (ADR-014).
                revision: format!("onnx-community-{}", config.variant.name()),
                native_dim: NATIVE_DIM,
                matryoshka_dims: vec![128, 256, 512, 768],
                max_input_tokens: MAX_INPUT_TOKENS,
                modalities: ModalitySet::TEXT,
            },
            target,
            device,
            max_batch: config.max_batch,
            concurrent_calls: false,
            preprocessing_version: 1,
        };
        Ok(Self {
            config,
            caps,
            tokenizer,
            session: Mutex::new(None),
            last_load_ms: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn config(&self) -> &OrtConfig {
        &self.config
    }

    /// Milliseconds the last model load took (session creation incl. EP compilation).
    #[must_use]
    pub fn last_load_ms(&self) -> Option<f64> {
        self.last_load_ms.lock().ok().and_then(|g| *g)
    }

    fn create_session(&self) -> Result<Session, EmbeddingError> {
        self.create_session_with(None)
    }

    /// Diagnostics: builds a throwaway session with verbose logging and reports how many
    /// graph nodes each execution provider received (and which op types fell back to CPU).
    /// Costs one extra model load; never used on the hot path.
    ///
    /// # Errors
    /// As session creation.
    pub fn placement(&self) -> Result<Placement, EmbeddingError> {
        let lines: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = Arc::clone(&lines);
        let logger: ort::logging::LoggerFunction =
            Arc::new(move |_level, _category, _id, _location, message: &str| {
                if let Ok(mut l) = sink.lock() {
                    l.push(message.to_owned());
                }
            });
        drop(self.create_session_with(Some(logger))?);
        let lines = lines.lock().map(|l| l.clone()).unwrap_or_default();
        Ok(Placement::parse(&lines))
    }

    fn create_session_with(
        &self,
        verbose_logger: Option<ort::logging::LoggerFunction>,
    ) -> Result<Session, EmbeddingError> {
        let started = Instant::now();
        let mut builder = Session::builder()
            .map_err(|e| backend_err("session builder", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| backend_err("optimization level", e))?;
        if let Some(logger) = verbose_logger {
            builder = builder
                .with_logger(logger)
                .map_err(|e| backend_err("logger", e))?
                .with_log_level(ort::logging::LogLevel::Verbose)
                .map_err(|e| backend_err("log level", e))?;
        }
        if let Some(threads) = self.config.threads {
            builder = builder
                .with_intra_threads(threads)
                .map_err(|e| backend_err("intra threads", e))?;
        }
        match self.config.device {
            Device::Cpu => {}
            Device::DirectMl { adapter } => {
                builder = directml(
                    builder,
                    DmlTarget::Adapter(adapter),
                    self.config.cpu_fallback,
                )?;
            }
            Device::DirectMlPreferred(pref) => {
                builder = directml(
                    builder,
                    DmlTarget::Preferred(pref),
                    self.config.cpu_fallback,
                )?;
            }
        }
        let session = builder
            .commit_from_file(self.config.onnx_path())
            .map_err(|e| backend_err("load model", e))?;
        if let Ok(mut slot) = self.last_load_ms.lock() {
            *slot = Some(started.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(session)
    }

    fn tokenize(&self, inputs: &[&str]) -> Result<(Vec<i64>, Vec<i64>, usize), EmbeddingError> {
        let encodings = inputs
            .iter()
            .map(|text| self.tokenizer.encode(*text, true))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| backend_err("tokenize", e))?;
        let seq = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0);
        let mut ids = vec![0_i64; inputs.len() * seq]; // pad id 0, right padding
        let mut mask = vec![0_i64; inputs.len() * seq];
        for (row, enc) in encodings.iter().enumerate() {
            for (col, &id) in enc.get_ids().iter().enumerate() {
                ids[row * seq + col] = i64::from(id);
                mask[row * seq + col] = 1;
            }
        }
        Ok((ids, mask, seq))
    }
}

#[cfg_attr(not(feature = "directml"), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
enum DmlTarget {
    Adapter(u32),
    Preferred(GpuPreference),
}

#[cfg(feature = "directml")]
fn directml(
    builder: ort::session::builder::SessionBuilder,
    target: DmlTarget,
    cpu_fallback: bool,
) -> Result<ort::session::builder::SessionBuilder, EmbeddingError> {
    use ort::ep::directml::{DeviceFilter, PerformancePreference};
    let ep = match target {
        DmlTarget::Adapter(adapter) => ort::ep::DirectML::default()
            .with_device_id(i32::try_from(adapter).map_err(|e| backend_err("adapter index", e))?),
        DmlTarget::Preferred(pref) => ort::ep::DirectML::default()
            .with_device_filter(DeviceFilter::Gpu)
            .with_performance_preference(match pref {
                GpuPreference::HighPerformance => PerformancePreference::HighPerformance,
                GpuPreference::MinimumPower => PerformancePreference::MinimumPower,
            }),
    };
    let mut builder = builder
        // DirectML requirements (ONNX Runtime docs): no memory pattern, sequential execution.
        .with_memory_pattern(false)
        .map_err(|e| backend_err("memory pattern", e))?
        .with_parallel_execution(false)
        .map_err(|e| backend_err("execution mode", e))?;
    if !cpu_fallback {
        builder = builder
            .with_config_entry("session.disable_cpu_ep_fallback", "1")
            .map_err(|e| backend_err("disable cpu fallback", e))?;
    }
    // A failing EP registration is an error, never a silent CPU-only session.
    builder
        .with_execution_providers([ep.build().error_on_failure()])
        .map_err(|e| backend_err("register DirectML", e))
}

#[cfg(not(feature = "directml"))]
fn directml(
    _builder: ort::session::builder::SessionBuilder,
    _target: DmlTarget,
    _cpu_fallback: bool,
) -> Result<ort::session::builder::SessionBuilder, EmbeddingError> {
    Err(EmbeddingError::Backend(
        "DirectML support not compiled in (enable feature `directml`)".into(),
    ))
}

impl EmbeddingBackend for OrtBackend {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn warm(&self, modality: Modality) -> Result<(), EmbeddingError> {
        if modality != Modality::Text {
            return Err(EmbeddingError::Unsupported(modality));
        }
        let mut slot = self
            .session
            .lock()
            .map_err(|_| EmbeddingError::Backend("session lock poisoned".into()))?;
        if slot.is_none() {
            *slot = Some(self.create_session()?);
        }
        Ok(())
    }

    fn unload(&self, modality: Modality) -> Result<(), EmbeddingError> {
        if modality == Modality::Text
            && let Ok(mut slot) = self.session.lock()
        {
            *slot = None;
        }
        Ok(())
    }

    fn is_warm(&self, modality: Modality) -> bool {
        modality == Modality::Text && self.session.lock().is_ok_and(|s| s.is_some())
    }

    fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let (ids, mask, seq) = self.tokenize(inputs)?;
        let batch = inputs.len();
        let tensor = |data: Vec<i64>| {
            Tensor::from_array(([batch, seq], data)).map_err(|e| backend_err("input tensor", e))
        };
        let empty = || {
            Tensor::<f32>::from_array(([0_usize, FEATURE_WIDTH], Vec::new()))
                .map_err(|e| backend_err("feature tensor", e))
        };
        let input_ids = tensor(ids)?;
        let attention_mask = tensor(mask)?;
        let (image, video, audio) = (empty()?, empty()?, empty()?);

        let mut slot = self
            .session
            .lock()
            .map_err(|_| EmbeddingError::Backend("session lock poisoned".into()))?;
        if slot.is_none() {
            *slot = Some(self.create_session()?);
        }
        let Some(session) = slot.as_mut() else {
            return Err(EmbeddingError::Backend("session unavailable".into()));
        };
        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids,
                "attention_mask" => attention_mask,
                "image_features" => image,
                "video_features" => video,
                "audio_features" => audio,
            ])
            .map_err(|e| backend_err("inference", e))?;
        let (shape, values) = outputs["sentence_embedding"]
            .try_extract_tensor::<f32>()
            .map_err(|e| backend_err("read sentence_embedding", e))?;
        let dims: Vec<i64> = shape.iter().copied().collect();
        if dims != [i64::try_from(batch).unwrap_or(-1), NATIVE_DIM as i64] {
            return Err(EmbeddingError::Backend(format!(
                "unexpected sentence_embedding shape {dims:?}"
            )));
        }
        Ok(values.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_names_round_trip() {
        for v in ModelVariant::ALL {
            assert_eq!(ModelVariant::parse(v.name()), Some(v));
        }
        assert_eq!(ModelVariant::parse("int3"), None);
        assert_eq!(ModelVariant::Q8.file_stem(), "model_quantized");
    }

    #[test]
    fn device_parse_display_round_trip() {
        for s in ["cpu", "dml:0", "dml:1", "dml:high", "dml:low"] {
            assert_eq!(Device::parse(s).map(|d| d.to_string()).as_deref(), Some(s));
        }
        assert_eq!(Device::parse("cuda"), None);
        assert_eq!(Device::parse("dml:x"), None);
    }

    #[test]
    fn parses_mixed_placement_log() {
        let log = [
            "Node placements".to_owned(),
            " Node(s) placed on [DmlExecutionProvider]. Number of nodes: 510".to_owned(),
            "  MatMul (/layers.0/MatMul)".to_owned(),
            " Node(s) placed on [CPUExecutionProvider]. Number of nodes: 3".to_owned(),
            "  Shape (/Shape)\n  Gather (/Gather_1)\n  Gather (/Gather_2)".to_owned(),
            "Some unrelated verbose message".to_owned(),
        ];
        let p = Placement::parse(&log);
        assert_eq!(p.nodes_per_provider["DmlExecutionProvider"], 510);
        assert_eq!(p.nodes_per_provider["CPUExecutionProvider"], 3);
        assert_eq!(p.cpu_op_types["Gather"], 2);
        assert_eq!(p.cpu_op_types["Shape"], 1);
        assert!((p.offloaded_fraction().unwrap() - 510.0 / 513.0).abs() < 1e-12);
    }

    #[test]
    fn parses_single_provider_placement() {
        let p = Placement::parse(&[
            "All nodes placed on [CPUExecutionProvider]. Number of nodes: 1061".to_owned(),
        ]);
        assert_eq!(p.nodes_per_provider["CPUExecutionProvider"], 1061);
        assert_eq!(p.offloaded_fraction(), Some(0.0));
        assert_eq!(Placement::parse(&[]).offloaded_fraction(), None);
    }

    #[test]
    fn backend_requires_initialized_runtime_and_files() {
        // Runtime is not initialized in unit tests (no dylib): construction must fail cleanly.
        let err = OrtBackend::new(OrtConfig::new(
            "/nonexistent",
            ModelVariant::Q4,
            Device::Cpu,
        ))
        .unwrap_err();
        assert!(matches!(err, EmbeddingError::Backend(_)), "{err:?}");
    }
}
