//! Runtime-independent front end over any [`EmbeddingBackend`].

use std::sync::Arc;

use lumen_core::CancellationToken;

use crate::backend::{EmbeddingBackend, EmbeddingError, ImageInput};
use crate::model::Modality;
use crate::prompt::{EmbeddingTask, TextInput};
use crate::space::{EmbeddingProfile, EmbeddingSpace};
use crate::vector::{EmbeddingBatch, truncate_and_normalize};

/// Applies prompts, batching, cancellation, validation, truncation and
/// normalization identically for every backend.
#[derive(Clone)]
pub struct Embedder {
    backend: Arc<dyn EmbeddingBackend>,
    profile: EmbeddingProfile,
    space: EmbeddingSpace,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedder")
            .field("backend", &self.backend.capabilities().backend)
            .field("space", &self.space.key())
            .finish()
    }
}

impl Embedder {
    /// # Errors
    /// `InvalidProfile` if `profile` does not fit the backend's model.
    pub fn new(
        backend: Arc<dyn EmbeddingBackend>,
        profile: EmbeddingProfile,
    ) -> Result<Self, EmbeddingError> {
        profile.validate(backend.capabilities())?;
        let space = EmbeddingSpace::new(backend.capabilities(), &profile);
        Ok(Self {
            backend,
            profile,
            space,
        })
    }

    #[must_use]
    pub fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    #[must_use]
    pub fn profile(&self) -> &EmbeddingProfile {
        &self.profile
    }

    #[must_use]
    pub fn backend(&self) -> &dyn EmbeddingBackend {
        self.backend.as_ref()
    }

    /// Loads the text encoder (keep the query path warm, docs/PERFORMANCE.md §3.2).
    ///
    /// # Errors
    /// As [`EmbeddingBackend::warm`].
    pub fn warm_text(&self) -> Result<(), EmbeddingError> {
        self.backend.warm(Modality::Text)
    }

    /// Embeds `inputs` for `task`, returning one normalized `profile.dim` vector per
    /// input, in order.
    ///
    /// Inputs are split into backend-sized batches; `cancel` is checked before each
    /// batch, so a superseded query or paused indexing stops at the next boundary.
    ///
    /// # Errors
    /// `EmptyInput` (checked up front, nothing is embedded), `Cancelled`, backend
    /// errors, and output validation errors (`OutputShape`, `NonFinite`, `ZeroVector`).
    pub fn embed(
        &self,
        task: EmbeddingTask,
        inputs: &[TextInput<'_>],
        cancel: Option<&CancellationToken>,
    ) -> Result<EmbeddingBatch, EmbeddingError> {
        if let Some(index) = inputs.iter().position(|i| i.text.trim().is_empty()) {
            return Err(EmbeddingError::EmptyInput { index });
        }
        let caps = self.backend.capabilities();
        let native_dim = caps.model.native_dim;
        let mut out = EmbeddingBatch::empty(self.profile.dim);

        for (chunk_index, chunk) in inputs.chunks(caps.max_batch).enumerate() {
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                return Err(EmbeddingError::Cancelled);
            }
            let formatted: Vec<String> = chunk
                .iter()
                .map(|input| self.profile.prompts.format(task, *input))
                .collect();
            let refs: Vec<&str> = formatted.iter().map(String::as_str).collect();
            let raw = self.backend.embed_text(&refs)?;

            let expected = chunk.len() * native_dim;
            if raw.len() != expected {
                return Err(EmbeddingError::OutputShape {
                    expected,
                    actual: raw.len(),
                });
            }
            let offset = chunk_index * caps.max_batch;
            out.extend(truncate_and_normalize(
                &raw,
                native_dim,
                self.profile.dim,
                offset,
            )?);
        }
        Ok(out)
    }

    /// Validate and normalize native image outputs, one image per backend call.
    ///
    /// # Errors
    /// Invalid RGB shape, cancellation, invalid native vectors or backend failure.
    pub fn embed_images(
        &self,
        inputs: &[ImageInput<'_>],
        cancel: Option<&CancellationToken>,
    ) -> Result<EmbeddingBatch, EmbeddingError> {
        for (index, input) in inputs.iter().enumerate() {
            if input.width == 0
                || input.height == 0
                || u64::from(input.width) * u64::from(input.height) * 3 != input.rgb.len() as u64
            {
                return Err(EmbeddingError::InvalidImage { index });
            }
        }
        let mut out = EmbeddingBatch::empty(self.profile.dim);
        // Never retain a decoded batch of pictures. Preempt at each image boundary.
        for (index, input) in inputs.iter().enumerate() {
            if cancel.is_some_and(CancellationToken::is_cancelled) {
                return Err(EmbeddingError::Cancelled);
            }
            let raw = self.backend.embed_images(std::slice::from_ref(input))?;
            let native = self.backend.capabilities().model.native_dim;
            if raw.len() != native {
                return Err(EmbeddingError::OutputShape {
                    expected: native,
                    actual: raw.len(),
                });
            }
            out.extend(truncate_and_normalize(
                &raw,
                native,
                self.profile.dim,
                index,
            )?);
        }
        Ok(out)
    }

    /// Convenience for one search query.
    ///
    /// # Errors
    /// As [`Embedder::embed`].
    pub fn embed_query(
        &self,
        query: &str,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<f32>, EmbeddingError> {
        self.embed(EmbeddingTask::SearchQuery, &[TextInput::new(query)], cancel)
            .map(EmbeddingBatch::into_flat)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::mock::MockBackend;
    use crate::model::Capabilities;
    use crate::prompt::PromptFormat;
    use crate::vector::dot;

    /// Records calls; returns a fixed raw vector per input, optionally malformed.
    struct Recording {
        caps: Capabilities,
        calls: Mutex<Vec<Vec<String>>>,
        malformed: Option<Vec<f32>>,
        cancel_after_first: Option<CancellationToken>,
    }

    impl Recording {
        fn new(max_batch: usize) -> Self {
            let mut caps = MockBackend::new().capabilities().clone();
            caps.max_batch = max_batch;
            Self {
                caps,
                calls: Mutex::new(Vec::new()),
                malformed: None,
                cancel_after_first: None,
            }
        }
    }

    impl EmbeddingBackend for Recording {
        fn capabilities(&self) -> &Capabilities {
            &self.caps
        }
        fn warm(&self, _: Modality) -> Result<(), EmbeddingError> {
            Ok(())
        }
        fn unload(&self, _: Modality) -> Result<(), EmbeddingError> {
            Ok(())
        }
        fn is_warm(&self, _: Modality) -> bool {
            true
        }
        fn embed_images(&self, inputs: &[ImageInput<'_>]) -> Result<Vec<f32>, EmbeddingError> {
            assert_eq!(inputs.len(), 1);
            if !self.caps.model.modalities.contains(Modality::Image) {
                return Err(EmbeddingError::Unsupported(Modality::Image));
            }
            self.calls.lock().unwrap().push(vec!["image".into()]);
            if let Some(cancel) = &self.cancel_after_first {
                cancel.cancel();
            }
            Ok(self.malformed.clone().unwrap_or_else(|| {
                let mut vector = vec![0.0; self.caps.model.native_dim];
                vector[0] = 2.0;
                vector
            }))
        }
        fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
            assert!(inputs.len() <= self.caps.max_batch, "batch limit exceeded");
            self.calls
                .lock()
                .unwrap()
                .push(inputs.iter().map(|s| (*s).to_owned()).collect());
            if let Some(token) = &self.cancel_after_first {
                token.cancel();
            }
            if let Some(bad) = &self.malformed {
                return Ok(bad.clone());
            }
            let mut row = vec![0.0; self.caps.model.native_dim];
            row[0] = 2.0;
            Ok(row.repeat(inputs.len()))
        }
    }

    #[test]
    fn image_shape_normalization_output_validation_and_cancellation() {
        let input = ImageInput {
            width: 1,
            height: 1,
            rgb: &[1, 2, 3],
        };
        let make = |malformed, cancel| {
            let mut backend = Recording::new(8);
            backend.caps.model.modalities =
                crate::ModalitySet::of(&[Modality::Text, Modality::Image]);
            backend.malformed = malformed;
            backend.cancel_after_first = cancel;
            Embedder::new(Arc::new(backend), EmbeddingProfile::DEFAULT).unwrap()
        };
        let e = make(None, None);
        let v = e.embed_images(&[input, input], None).unwrap();
        assert_eq!(v.iter().count(), 2);
        assert_eq!(v.iter().next().unwrap()[0], 1.0);
        assert_eq!(
            e.embed_images(&[ImageInput { rgb: &[1], ..input }], None),
            Err(EmbeddingError::InvalidImage { index: 0 })
        );
        assert!(matches!(
            make(Some(vec![1.0]), None).embed_images(&[input], None),
            Err(EmbeddingError::OutputShape { .. })
        ));
        assert!(
            make(Some(vec![f32::NAN; 768]), None)
                .embed_images(&[input], None)
                .is_err()
        );
        let cancel = CancellationToken::new();
        let e = make(None, Some(cancel.clone()));
        assert_eq!(
            e.embed_images(&[input, input], Some(&cancel)),
            Err(EmbeddingError::Cancelled)
        );
        let e = Embedder::new(Arc::new(MockBackend::new()), EmbeddingProfile::DEFAULT).unwrap();
        assert_eq!(
            e.embed_images(&[input], None),
            Err(EmbeddingError::Unsupported(Modality::Image))
        );
    }

    fn texts(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("chunk {i}")).collect()
    }

    fn inputs(texts: &[String]) -> Vec<TextInput<'_>> {
        texts.iter().map(|t| TextInput::new(t)).collect()
    }

    #[test]
    fn splits_into_backend_batches_and_applies_prompts() {
        let backend = Arc::new(Recording::new(4));
        let embedder = Embedder::new(backend.clone(), EmbeddingProfile::DEFAULT).unwrap();
        let t = texts(10);

        let batch = embedder
            .embed(EmbeddingTask::SearchDocument, &inputs(&t), None)
            .unwrap();
        assert_eq!(batch.len(), 10);
        assert_eq!(batch.dim(), 256);
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls.iter().map(Vec::len).collect::<Vec<_>>(), [4, 4, 2]);
        assert_eq!(calls[0][0], "title: none | text: chunk 0");
        assert_eq!(calls[2][1], "title: none | text: chunk 9");
    }

    #[test]
    fn output_is_unit_length_at_profile_dimension() {
        let embedder =
            Embedder::new(Arc::new(MockBackend::new()), EmbeddingProfile::DEFAULT).unwrap();
        let v = embedder.embed_query("bluetooth settings", None).unwrap();
        assert_eq!(v.len(), 256);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn empty_inputs_are_rejected_before_any_backend_call() {
        let backend = Arc::new(Recording::new(8));
        let embedder = Embedder::new(backend.clone(), EmbeddingProfile::DEFAULT).unwrap();
        let err = embedder
            .embed(
                EmbeddingTask::SearchQuery,
                &[TextInput::new("ok"), TextInput::new("  \n")],
                None,
            )
            .unwrap_err();
        assert_eq!(err, EmbeddingError::EmptyInput { index: 1 });
        assert!(backend.calls.lock().unwrap().is_empty());
        assert!(
            embedder
                .embed(EmbeddingTask::SearchQuery, &[], None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn cancellation_stops_at_next_batch_boundary() {
        let token = CancellationToken::new();
        let mut recording = Recording::new(2);
        recording.cancel_after_first = Some(token.clone());
        let backend = Arc::new(recording);
        let embedder = Embedder::new(backend.clone(), EmbeddingProfile::DEFAULT).unwrap();
        let t = texts(6);

        let err = embedder
            .embed(EmbeddingTask::SearchDocument, &inputs(&t), Some(&token))
            .unwrap_err();
        assert_eq!(err, EmbeddingError::Cancelled);
        assert_eq!(backend.calls.lock().unwrap().len(), 1);

        // Already-cancelled token: nothing runs.
        let before = backend.calls.lock().unwrap().len();
        assert_eq!(
            embedder.embed_query("x", Some(&token)),
            Err(EmbeddingError::Cancelled)
        );
        assert_eq!(backend.calls.lock().unwrap().len(), before);
    }

    #[test]
    fn malformed_backend_output_is_caught() {
        let mut short = Recording::new(4);
        short.malformed = Some(vec![1.0; 10]);
        let embedder = Embedder::new(Arc::new(short), EmbeddingProfile::DEFAULT).unwrap();
        assert_eq!(
            embedder.embed_query("x", None),
            Err(EmbeddingError::OutputShape {
                expected: 768,
                actual: 10
            })
        );

        let mut nan = Recording::new(4);
        let mut row = vec![1.0; 768];
        row[5] = f32::NAN;
        nan.malformed = Some(row);
        let embedder = Embedder::new(Arc::new(nan), EmbeddingProfile::DEFAULT).unwrap();
        assert_eq!(
            embedder.embed_query("x", None),
            Err(EmbeddingError::NonFinite { index: 0 })
        );
    }

    #[test]
    fn error_indices_account_for_batching() {
        // Second batch's first vector is zero in its head -> index 3 with max_batch 3.
        struct ZeroSecondBatch(Capabilities, Mutex<usize>);
        impl EmbeddingBackend for ZeroSecondBatch {
            fn capabilities(&self) -> &Capabilities {
                &self.0
            }
            fn warm(&self, _: Modality) -> Result<(), EmbeddingError> {
                Ok(())
            }
            fn unload(&self, _: Modality) -> Result<(), EmbeddingError> {
                Ok(())
            }
            fn is_warm(&self, _: Modality) -> bool {
                true
            }
            fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
                let mut calls = self.1.lock().unwrap();
                *calls += 1;
                let value = if *calls == 2 { 0.0 } else { 1.0 };
                Ok(vec![value; inputs.len() * self.0.model.native_dim])
            }
        }
        let mut caps = MockBackend::new().capabilities().clone();
        caps.max_batch = 3;
        let embedder = Embedder::new(
            Arc::new(ZeroSecondBatch(caps, Mutex::new(0))),
            EmbeddingProfile::DEFAULT,
        )
        .unwrap();
        let t = texts(5);
        assert_eq!(
            embedder.embed(EmbeddingTask::SearchDocument, &inputs(&t), None),
            Err(EmbeddingError::ZeroVector { index: 3 })
        );
    }

    #[test]
    fn invalid_profile_is_rejected_at_construction() {
        let profile = EmbeddingProfile {
            dim: 300,
            prompts: PromptFormat::RAW,
        };
        assert!(matches!(
            Embedder::new(Arc::new(MockBackend::new()), profile),
            Err(EmbeddingError::InvalidProfile(_))
        ));
    }
}
