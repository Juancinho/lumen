//! The runtime-facing trait.

use std::fmt;

use crate::model::{Capabilities, Modality};

/// Errors from embedding. Backends map runtime errors into [`EmbeddingError::Backend`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EmbeddingError {
    /// Invalid oriented RGB image shape; no source data is included.
    InvalidImage { index: usize },
    /// The backend/model does not support this modality.
    Unsupported(Modality),
    /// Input at `index` is empty or whitespace only.
    EmptyInput { index: usize },
    /// The profile is incompatible with the model (dimension not supported, ...).
    InvalidProfile(String),
    /// Cancelled between batches via `CancellationToken`.
    Cancelled,
    /// Backend returned the wrong number of values.
    OutputShape { expected: usize, actual: usize },
    /// Vector at `index` contains NaN/inf.
    NonFinite { index: usize },
    /// Vector at `index` has (near-)zero norm after truncation; cannot normalize.
    ZeroVector { index: usize },
    /// Runtime failure (model missing, device lost, ...). Message is for logs only and
    /// must not contain input text.
    Backend(String),
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidImage { index } => write!(f, "invalid image at {index}"),
            Self::Unsupported(m) => write!(f, "modality {m:?} is not supported by this backend"),
            Self::EmptyInput { index } => write!(f, "input {index} is empty"),
            Self::InvalidProfile(why) => write!(f, "invalid embedding profile: {why}"),
            Self::Cancelled => f.write_str("embedding cancelled"),
            Self::OutputShape { expected, actual } => {
                write!(f, "backend returned {actual} values, expected {expected}")
            }
            Self::NonFinite { index } => write!(f, "vector {index} contains non-finite values"),
            Self::ZeroVector { index } => write!(f, "vector {index} has zero norm"),
            Self::Backend(msg) => write!(f, "embedding backend error: {msg}"),
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// An embedding runtime + model.
///
/// Contract for implementors:
/// - `embed_text` receives **fully formatted** strings (prompts already applied by
///   `Embedder`) and returns `inputs.len() * capabilities().model.native_dim` values,
///   row-major, in input order. No truncation/normalization needed.
/// - Never receives more than `capabilities().max_batch` inputs per call.
/// - If the modality is not warm, load it lazily (and report the cost via `warm`).
/// - Thread-safe (`Send + Sync`). If the runtime is not re-entrant, serialize
///   internally and set `concurrent_calls = false`.
/// - Never log input text (docs/ARCHITECTURE.md §18).
pub trait EmbeddingBackend: Send + Sync {
    fn capabilities(&self) -> &Capabilities;

    /// Loads the encoder for `modality` so the next call is warm. Idempotent.
    ///
    /// # Errors
    /// `Unsupported` for unknown modalities, `Backend` for load failures.
    fn warm(&self, modality: Modality) -> Result<(), EmbeddingError>;

    /// Releases the encoder for `modality` (memory profiles, docs/PERFORMANCE.md §15).
    /// Idempotent; a later `embed_*` call reloads lazily.
    ///
    /// # Errors
    /// `Backend` if the runtime fails to release resources.
    fn unload(&self, modality: Modality) -> Result<(), EmbeddingError>;

    fn is_warm(&self, modality: Modality) -> bool;

    /// Raw native-dimension text embeddings, row-major.
    ///
    /// # Errors
    /// `Backend` for runtime failures.
    fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError>;

    /// Raw native-dimension image vectors, in input order, without text prompts.
    /// # Errors
    /// Unsupported modality or runtime failure. Existing text-only backends defer images.
    fn embed_images(&self, _inputs: &[ImageInput<'_>]) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Unsupported(Modality::Image))
    }
}

/// One decoded, EXIF-oriented RGB image. Decoding/resource admission belongs to the caller.
#[derive(Debug, Clone, Copy)]
pub struct ImageInput<'a> {
    pub width: u32,
    pub height: u32,
    pub rgb: &'a [u8],
}
