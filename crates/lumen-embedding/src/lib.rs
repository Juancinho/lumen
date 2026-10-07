//! Lumen embedding abstraction (ADR-005, T005) and device policy (ADR-019, T013).
//!
//! ```text
//!   callers (query service T204, index queue T202)
//!        │  TextInput + EmbeddingTask
//!        ▼
//!   Embedder ── prompts (versioned) ── batching ── cancellation ── output validation
//!        │                                        └─ Matryoshka truncation + L2 renorm
//!        ▼
//!   dyn EmbeddingBackend  (MockBackend now; runtime backends selected by T006)
//! ```
//!
//! Correctness that must be identical for every runtime (prompt strings, truncation,
//! normalization, shape checks, the [`EmbeddingSpace`] compatibility key) lives in
//! [`Embedder`], not in backends. Backends only turn formatted strings into raw
//! native-dimension vectors.
//!
//! The trait is synchronous by design (ADR-014): inference is CPU/accelerator
//! bound; scheduling (query lane preempting index lane) belongs to the services
//! that own worker threads, not to the backend interface.

#![forbid(unsafe_code)]

mod backend;
mod embedder;
mod mock;
mod model;
pub mod policy;
pub mod probe;
mod prompt;
mod space;
mod vector;

pub use backend::{EmbeddingBackend, EmbeddingError};
pub use embedder::Embedder;
pub use mock::{MockBackend, MockLatency};
pub use model::{Capabilities, ExecutionTarget, Modality, ModalitySet, ModelInfo};
pub use prompt::{EmbeddingTask, PromptFormat, TextInput};
pub use space::{EmbeddingProfile, EmbeddingSpace, Normalization};
pub use vector::{EmbeddingBatch, dot, truncate_and_normalize};
