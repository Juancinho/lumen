//! Deterministic, dependency-free test backend.
//!
//! Feature hashing over lowercase word tokens and character trigrams: texts that
//! share words/subwords get similar vectors, so retrieval/fusion code can be tested
//! for plausible behaviour without a model. Stable across platforms and runs (FNV-1a).
//! Not a relevance proxy for EmbeddingGemma.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::backend::{EmbeddingBackend, EmbeddingError};
use crate::model::{Capabilities, ExecutionTarget, Modality, ModalitySet, ModelInfo};

const NATIVE_DIM: usize = 768;
const WORD_WEIGHT: f32 = 1.0;
const TRIGRAM_WEIGHT: f32 = 0.5;

/// Simulated costs, so the benchmark harness and schedulers can be exercised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockLatency {
    /// Added once per `warm(Text)` that actually loads.
    pub load: Duration,
    /// Added once per `embed_text` call.
    pub per_call: Duration,
    /// Added per input in a call.
    pub per_item: Duration,
}

#[derive(Debug)]
pub struct MockBackend {
    caps: Capabilities,
    latency: MockLatency,
    text_warm: AtomicBool,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    /// Zero-latency mock with EmbeddingGemma-like shape (768d, 128/256/512/768).
    #[must_use]
    pub fn new() -> Self {
        Self::with_latency(MockLatency::default())
    }

    #[must_use]
    pub fn with_latency(latency: MockLatency) -> Self {
        Self {
            caps: Capabilities {
                backend: "mock".into(),
                runtime_version: Some(env!("CARGO_PKG_VERSION").into()),
                model: ModelInfo {
                    id: "lumen-mock-hash".into(),
                    revision: "1".into(),
                    native_dim: NATIVE_DIM,
                    matryoshka_dims: vec![128, 256, 512, 768],
                    max_input_tokens: 8192,
                    modalities: ModalitySet::TEXT,
                },
                target: ExecutionTarget::Cpu,
                device: None,
                max_batch: 64,
                concurrent_calls: true,
                preprocessing_version: 1,
            },
            latency,
            text_warm: AtomicBool::new(false),
        }
    }

    fn embed_one(text: &str, out: &mut [f32]) {
        let lower = text.to_lowercase();
        let mut any = false;
        for word in lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            add_feature(out, fnv1a(word.as_bytes()), WORD_WEIGHT);
            let padded: Vec<char> = std::iter::once(' ')
                .chain(word.chars())
                .chain(std::iter::once(' '))
                .collect();
            for tri in padded.windows(3) {
                let s: String = tri.iter().collect();
                add_feature(
                    out,
                    fnv1a(s.as_bytes()) ^ 0x9e37_79b9_7f4a_7c15,
                    TRIGRAM_WEIGHT,
                );
            }
            any = true;
        }
        if !any {
            // Punctuation-only input: still a deterministic non-zero vector.
            add_feature(out, fnv1a(lower.as_bytes()), WORD_WEIGHT);
        }
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn add_feature(out: &mut [f32], hash: u64, weight: f32) {
    // Spread features over the whole vector, including the Matryoshka head that
    // truncation keeps.
    #[allow(clippy::cast_possible_truncation)]
    let index = (hash % out.len() as u64) as usize;
    let sign = if hash >> 63 == 0 { 1.0 } else { -1.0 };
    out[index] += sign * weight;
}

impl EmbeddingBackend for MockBackend {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn warm(&self, modality: Modality) -> Result<(), EmbeddingError> {
        if modality != Modality::Text {
            return Err(EmbeddingError::Unsupported(modality));
        }
        if !self.text_warm.swap(true, Ordering::AcqRel) && !self.latency.load.is_zero() {
            std::thread::sleep(self.latency.load);
        }
        Ok(())
    }

    fn unload(&self, modality: Modality) -> Result<(), EmbeddingError> {
        if modality == Modality::Text {
            self.text_warm.store(false, Ordering::Release);
        }
        Ok(())
    }

    fn is_warm(&self, modality: Modality) -> bool {
        modality == Modality::Text && self.text_warm.load(Ordering::Acquire)
    }

    fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
        self.warm(Modality::Text)?;
        let n = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
        let delay = self.latency.per_call + self.latency.per_item * n;
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        let mut out = vec![0.0; inputs.len() * NATIVE_DIM];
        for (text, row) in inputs.iter().zip(out.chunks_exact_mut(NATIVE_DIM)) {
            Self::embed_one(text, row);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;
    use crate::embedder::Embedder;
    use crate::space::EmbeddingProfile;
    use crate::vector::dot;

    fn embedder() -> Embedder {
        Embedder::new(Arc::new(MockBackend::new()), EmbeddingProfile::DEFAULT).unwrap()
    }

    #[test]
    fn deterministic() {
        let e = embedder();
        assert_eq!(
            e.embed_query("Docker connection refused", None),
            e.embed_query("Docker connection refused", None)
        );
    }

    #[test]
    fn shared_words_are_closer() {
        let e = embedder();
        let q = e.embed_query("docker connection refused", None).unwrap();
        let near = e
            .embed_query("docker error: connection refused on port 5432", None)
            .unwrap();
        let far = e.embed_query("spotify playlist", None).unwrap();
        assert!(
            dot(&q, &near) > dot(&q, &far) + 0.2,
            "{} vs {}",
            dot(&q, &near),
            dot(&q, &far)
        );
    }

    #[test]
    fn punctuation_only_is_not_zero() {
        let e = embedder();
        assert!(e.embed_query("???", None).is_ok());
    }

    #[test]
    fn warm_unload_lifecycle() {
        let b = MockBackend::new();
        assert!(!b.is_warm(Modality::Text));
        b.warm(Modality::Text).unwrap();
        assert!(b.is_warm(Modality::Text));
        b.unload(Modality::Text).unwrap();
        assert!(!b.is_warm(Modality::Text));
        // Lazy load on use.
        b.embed_text(&["x"]).unwrap();
        assert!(b.is_warm(Modality::Text));
        assert_eq!(
            b.warm(Modality::Image),
            Err(EmbeddingError::Unsupported(Modality::Image))
        );
        assert!(!b.is_warm(Modality::Audio));
    }

    #[test]
    fn simulated_latency_is_applied() {
        let b = MockBackend::with_latency(MockLatency {
            load: Duration::ZERO,
            per_call: Duration::from_millis(5),
            per_item: Duration::from_millis(1),
        });
        b.warm(Modality::Text).unwrap();
        let started = Instant::now();
        b.embed_text(&["a", "b", "c"]).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(8));
    }
}
