//! Device probe (T013): measures one backend instance through the real [`Embedder`] so the
//! policy compares devices on exactly what production runs (prompts, truncation, checks).
//!
//! The probe reports latency, throughput, run-to-run stability and agreement with CPU
//! vectors. Placement and device memory are backend/platform specific: the caller fills
//! [`ProbeMetrics::offloaded_fraction`] and the memory fields.

use std::time::Instant;

use crate::backend::EmbeddingError;
use crate::embedder::Embedder;
use crate::policy::ProbeMetrics;
use crate::prompt::{EmbeddingTask, TextInput};
use crate::vector::{EmbeddingBatch, dot};

/// Inputs. Use the same corpus for every device of one machine.
#[derive(Debug, Clone, Copy)]
pub struct ProbeCorpus<'a> {
    pub queries: &'a [&'a str],
    pub documents: &'a [&'a str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeConfig {
    /// Unmeasured queries after warm-up (first calls compile kernels / allocate).
    pub warmup_queries: usize,
    /// Measured queries (cycling through the corpus).
    pub measured_queries: usize,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            warmup_queries: 5,
            measured_queries: 40,
        }
    }
}

/// Vectors produced by a probe; the CPU's are the reference for other devices.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeVectors {
    pub queries: EmbeddingBatch,
    pub documents: EmbeddingBatch,
}

/// Two runs on the same device must agree at least this well to count as stable
/// (bit-identical is typical on CPU; GPUs may reorder float reductions).
pub const STABILITY_MIN_COSINE: f64 = 0.99999;

/// Runs the probe. `reference` = the CPU's [`ProbeVectors`] for the same corpus and space.
///
/// # Errors
/// Any embedding error (backend failure, NaN/zero vectors): the device is unusable and the
/// caller records [`crate::policy::ProbeOutcome::Failed`].
///
/// # Panics
/// If the corpus has no queries or no documents.
pub fn measure(
    embedder: &Embedder,
    corpus: &ProbeCorpus<'_>,
    reference: Option<&ProbeVectors>,
    cfg: &ProbeConfig,
) -> Result<(ProbeMetrics, ProbeVectors), EmbeddingError> {
    assert!(
        !corpus.queries.is_empty() && !corpus.documents.is_empty(),
        "probe corpus needs queries and documents"
    );
    embedder.warm_text()?;
    for q in corpus.queries.iter().cycle().take(cfg.warmup_queries) {
        embedder.embed_query(q, None)?;
    }
    let mut samples = Vec::with_capacity(cfg.measured_queries);
    for q in corpus
        .queries
        .iter()
        .cycle()
        .take(cfg.measured_queries.max(1))
    {
        let t = Instant::now();
        embedder.embed_query(q, None)?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);

    let query_inputs: Vec<_> = corpus.queries.iter().map(|q| TextInput::new(q)).collect();
    let doc_inputs: Vec<_> = corpus.documents.iter().map(|d| TextInput::new(d)).collect();
    let queries = embedder.embed(EmbeddingTask::SearchQuery, &query_inputs, None)?;
    let t = Instant::now();
    let documents = embedder.embed(EmbeddingTask::SearchDocument, &doc_inputs, None)?;
    let doc_secs = t.elapsed().as_secs_f64();
    let again = embedder.embed(EmbeddingTask::SearchDocument, &doc_inputs, None)?;
    let stable = min_cosine(&documents, &again) >= STABILITY_MIN_COSINE;

    let min_cosine_vs_cpu = reference
        .map(|r| min_cosine(&queries, &r.queries).min(min_cosine(&documents, &r.documents)));

    #[allow(clippy::cast_precision_loss)]
    let metrics = ProbeMetrics {
        query_p50_ms: percentile(&samples, 50.0),
        query_p95_ms: percentile(&samples, 95.0),
        index_chunks_per_s: doc_inputs.len() as f64 / doc_secs.max(1e-9),
        min_cosine_vs_cpu,
        stable,
        offloaded_fraction: None,
        device_memory_mib: None,
        device_memory_total_mib: None,
    };
    Ok((metrics, ProbeVectors { queries, documents }))
}

/// Lowest row-wise cosine (vectors are unit length); `-1.0` on a shape mismatch.
#[must_use]
pub fn min_cosine(a: &EmbeddingBatch, b: &EmbeddingBatch) -> f64 {
    if a.dim() != b.dim() || a.len() != b.len() {
        return -1.0;
    }
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| f64::from(dot(x, y)))
        .fold(1.0, f64::min)
}

/// Nearest-rank percentile of an ascending, non-empty slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::mock::MockBackend;
    use crate::space::EmbeddingProfile;

    fn embedder() -> Embedder {
        Embedder::new(Arc::new(MockBackend::new()), EmbeddingProfile::DEFAULT).unwrap()
    }

    const CORPUS: ProbeCorpus<'static> = ProbeCorpus {
        queries: &["spotify", "invoice from march", "rust async cancellation"],
        documents: &[
            "The quarterly budget spreadsheet lists marketing costs.",
            "fn main() { println!(\"hello\"); }",
        ],
    };

    #[test]
    fn mock_probe_is_stable_and_matches_itself() {
        let e = embedder();
        let (cpu, vectors) = measure(&e, &CORPUS, None, &ProbeConfig::default()).unwrap();
        assert!(cpu.stable);
        assert!(cpu.min_cosine_vs_cpu.is_none());
        assert!(cpu.query_p95_ms >= cpu.query_p50_ms && cpu.index_chunks_per_s > 0.0);
        assert_eq!(vectors.queries.len(), 3);
        assert_eq!(vectors.documents.len(), 2);

        let (other, _) = measure(&e, &CORPUS, Some(&vectors), &ProbeConfig::default()).unwrap();
        assert!(other.min_cosine_vs_cpu.unwrap() > 0.999_999);
    }

    #[test]
    fn different_vectors_lower_the_fidelity() {
        let e = embedder();
        let (_, mut reference) = measure(&e, &CORPUS, None, &ProbeConfig::default()).unwrap();
        // Swap the two documents: same vectors, wrong order.
        let swapped = EmbeddingBatch::from_flat(
            reference.documents.dim(),
            [
                reference.documents.get(1).unwrap(),
                reference.documents.get(0).unwrap(),
            ]
            .concat(),
        );
        reference.documents = swapped;
        let (m, _) = measure(&e, &CORPUS, Some(&reference), &ProbeConfig::default()).unwrap();
        assert!(m.min_cosine_vs_cpu.unwrap() < 0.99);
    }

    #[test]
    fn min_cosine_rejects_shape_mismatch() {
        let a = EmbeddingBatch::from_flat(2, vec![1.0, 0.0]);
        let b = EmbeddingBatch::from_flat(2, vec![1.0, 0.0, 0.0, 1.0]);
        assert!((min_cosine(&a, &b) + 1.0).abs() < f64::EPSILON);
        assert!((min_cosine(&a, &a) - 1.0).abs() < 1e-9);
    }
}
