//! Static description of a backend and the model it runs.

use std::fmt;

/// Input modality. EmbeddingGemma 2 maps all of them into one space; encoders can
/// be loaded/unloaded independently (docs/ARCHITECTURE.md §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Modality {
    Text = 0,
    Image = 1,
    Audio = 2,
    Video = 3,
}

impl Modality {
    pub const ALL: [Self; 4] = [Self::Text, Self::Image, Self::Audio, Self::Video];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Small set of modalities.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModalitySet(u8);

impl ModalitySet {
    pub const TEXT: Self = Self::of(&[Modality::Text]);

    #[must_use]
    pub const fn of(modalities: &[Modality]) -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < modalities.len() {
            bits |= modalities[i].bit();
            i += 1;
        }
        Self(bits)
    }

    #[must_use]
    pub const fn contains(self, modality: Modality) -> bool {
        self.0 & modality.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Modality> {
        Modality::ALL.into_iter().filter(move |m| self.contains(*m))
    }
}

impl fmt::Debug for ModalitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// Where inference runs. Informational (diagnostics, benchmark reports, memory policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionTarget {
    Cpu,
    Gpu,
    Npu,
    Other,
}

impl ExecutionTarget {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Npu => "npu",
            Self::Other => "other",
        }
    }
}

/// The model a backend serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Stable model identifier, e.g. `embeddinggemma-2`.
    pub id: String,
    /// Exact weights revision, including quantization variant (e.g. `q8-2026-06`).
    /// Different weights MUST have different revisions: they produce different spaces.
    pub revision: String,
    /// Native output dimension (768 for EmbeddingGemma).
    pub native_dim: usize,
    /// Dimensions the model is trained to be truncated to (Matryoshka), incl. native.
    pub matryoshka_dims: Vec<usize>,
    /// Context limit in tokens; chunkers keep inputs well below it.
    pub max_input_tokens: usize,
    pub modalities: ModalitySet,
}

/// Static description of a backend instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Backend implementation name, e.g. `mock`, `onnxruntime`, `openvino`.
    pub backend: String,
    /// Runtime/library version string for reports, if known.
    pub runtime_version: Option<String>,
    pub model: ModelInfo,
    pub target: ExecutionTarget,
    /// Human-readable device, e.g. `Intel Core Ultra 7 NPU`.
    pub device: Option<String>,
    /// Largest batch the backend accepts per call (>= 1). `Embedder` splits larger inputs.
    pub max_batch: usize,
    /// Whether concurrent `embed_*` calls run in parallel. If `false` the backend
    /// serializes internally; callers must prioritise query work (T204).
    pub concurrent_calls: bool,
    /// Version of input preprocessing done inside the backend (tokenizer config,
    /// image resize policy). Part of [`crate::EmbeddingSpace`].
    pub preprocessing_version: u32,
}
