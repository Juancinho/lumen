//! The identity of a vector space. Vectors from different spaces must never be
//! mixed in one index generation (docs/SEARCH_AND_INDEXING.md §16).

use std::fmt;

use crate::backend::EmbeddingError;
use crate::model::{Capabilities, Modality};
use crate::prompt::PromptFormat;

/// How vectors are normalized. Only L2 for now; kept explicit because it is part of
/// the space identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Normalization {
    L2,
}

/// Lumen's choice of how to use a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmbeddingProfile {
    /// Output dimension after Matryoshka truncation (ADR-006: 256).
    pub dim: usize,
    pub prompts: PromptFormat,
}

impl EmbeddingProfile {
    /// ADR-006 default: 256d, L2-normalized, EmbeddingGemma retrieval prompts.
    pub const DEFAULT: Self = Self {
        dim: 256,
        prompts: PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1,
    };

    /// Checks the profile against what a backend/model supports.
    ///
    /// # Errors
    /// `InvalidProfile` if the dimension is not a supported Matryoshka truncation,
    /// exceeds the native dimension, or the model has no text encoder / `max_batch` is 0.
    pub fn validate(&self, caps: &Capabilities) -> Result<(), EmbeddingError> {
        let model = &caps.model;
        if self.dim == 0 || self.dim > model.native_dim {
            return Err(EmbeddingError::InvalidProfile(format!(
                "dimension {} outside 1..={}",
                self.dim, model.native_dim
            )));
        }
        if !model.matryoshka_dims.contains(&self.dim) {
            return Err(EmbeddingError::InvalidProfile(format!(
                "dimension {} is not a supported truncation of {} (supported: {:?})",
                self.dim, model.id, model.matryoshka_dims
            )));
        }
        if !model.modalities.contains(Modality::Text) {
            return Err(EmbeddingError::InvalidProfile(
                "model has no text encoder".into(),
            ));
        }
        if caps.max_batch == 0 {
            return Err(EmbeddingError::InvalidProfile(
                "backend max_batch is 0".into(),
            ));
        }
        Ok(())
    }
}

/// Everything that makes two vectors comparable. Stored with every index generation.
///
/// Deliberately excludes the backend implementation and execution target: the same
/// weights on CPU or NPU are one space. Different weights (incl. quantization) must
/// differ in `model_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddingSpace {
    pub model_id: String,
    pub model_revision: String,
    pub preprocessing_version: u32,
    pub prompt_id: &'static str,
    pub prompt_version: u32,
    pub dim: usize,
    pub normalization: Normalization,
}

impl EmbeddingSpace {
    #[must_use]
    pub fn new(caps: &Capabilities, profile: &EmbeddingProfile) -> Self {
        Self {
            model_id: caps.model.id.clone(),
            model_revision: caps.model.revision.clone(),
            preprocessing_version: caps.preprocessing_version,
            prompt_id: profile.prompts.id,
            prompt_version: profile.prompts.version,
            dim: profile.dim,
            normalization: Normalization::L2,
        }
    }

    /// Stable, human-readable key for index metadata, e.g.
    /// `embeddinggemma-2@q8-1/pre1/embeddinggemma-retrieval@1/d256/l2`.
    #[must_use]
    pub fn key(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for EmbeddingSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let norm = match self.normalization {
            Normalization::L2 => "l2",
        };
        write!(
            f,
            "{}@{}/pre{}/{}@{}/d{}/{norm}",
            self.model_id,
            self.model_revision,
            self.preprocessing_version,
            self.prompt_id,
            self.prompt_version,
            self.dim
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExecutionTarget, ModalitySet, ModelInfo};

    fn caps() -> Capabilities {
        Capabilities {
            backend: "test".into(),
            runtime_version: None,
            model: ModelInfo {
                id: "embeddinggemma-2".into(),
                revision: "q8-1".into(),
                native_dim: 768,
                matryoshka_dims: vec![128, 256, 512, 768],
                max_input_tokens: 8192,
                modalities: ModalitySet::TEXT,
            },
            target: ExecutionTarget::Cpu,
            device: None,
            max_batch: 16,
            concurrent_calls: false,
            preprocessing_version: 1,
        }
    }

    #[test]
    fn default_profile_is_256d_gemma_prompts() {
        assert_eq!(EmbeddingProfile::DEFAULT.dim, 256);
        assert_eq!(EmbeddingProfile::DEFAULT.validate(&caps()), Ok(()));
        assert_eq!(
            EmbeddingSpace::new(&caps(), &EmbeddingProfile::DEFAULT).key(),
            "embeddinggemma-2@q8-1/pre1/embeddinggemma-retrieval@1/d256/l2"
        );
    }

    #[test]
    fn rejects_unsupported_dimensions_and_models() {
        for dim in [0, 300, 1024] {
            let profile = EmbeddingProfile {
                dim,
                ..EmbeddingProfile::DEFAULT
            };
            assert!(profile.validate(&caps()).is_err(), "dim {dim}");
        }
        let mut no_text = caps();
        no_text.model.modalities = ModalitySet::of(&[Modality::Image]);
        assert!(EmbeddingProfile::DEFAULT.validate(&no_text).is_err());
        let mut no_batch = caps();
        no_batch.max_batch = 0;
        assert!(EmbeddingProfile::DEFAULT.validate(&no_batch).is_err());
    }

    #[test]
    fn space_changes_with_anything_that_changes_vectors() {
        let base = EmbeddingSpace::new(&caps(), &EmbeddingProfile::DEFAULT);

        let mut other_weights = caps();
        other_weights.model.revision = "f16-1".into();
        let mut other_pre = caps();
        other_pre.preprocessing_version = 2;
        let raw = EmbeddingProfile {
            prompts: PromptFormat::RAW,
            ..EmbeddingProfile::DEFAULT
        };
        let d512 = EmbeddingProfile {
            dim: 512,
            ..EmbeddingProfile::DEFAULT
        };
        for different in [
            EmbeddingSpace::new(&other_weights, &EmbeddingProfile::DEFAULT),
            EmbeddingSpace::new(&other_pre, &EmbeddingProfile::DEFAULT),
            EmbeddingSpace::new(&caps(), &raw),
            EmbeddingSpace::new(&caps(), &d512),
        ] {
            assert_ne!(base, different);
            assert_ne!(base.key(), different.key());
        }
    }

    #[test]
    fn space_ignores_backend_and_device() {
        let base = EmbeddingSpace::new(&caps(), &EmbeddingProfile::DEFAULT);
        let mut npu = caps();
        npu.backend = "openvino".into();
        npu.target = ExecutionTarget::Npu;
        npu.max_batch = 1;
        assert_eq!(base, EmbeddingSpace::new(&npu, &EmbeddingProfile::DEFAULT));
    }
}
