//! Vector math shared by all backends.

use crate::backend::EmbeddingError;

/// Norm below which a vector is considered degenerate.
const MIN_NORM: f64 = 1e-12;

/// Row-major batch of L2-normalized vectors of equal dimension. One allocation per batch.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    dim: usize,
    data: Vec<f32>,
}

impl EmbeddingBatch {
    /// # Panics
    /// If `dim == 0` or `data.len()` is not a multiple of `dim` (internal invariant).
    pub(crate) fn from_flat(dim: usize, data: Vec<f32>) -> Self {
        assert!(
            dim > 0 && data.len().is_multiple_of(dim),
            "malformed embedding batch"
        );
        Self { dim, data }
    }

    /// Rebuilds a batch from a row-major buffer (e.g. reference vectors read from disk).
    /// `None` if `dim == 0` or `data` is not a whole number of rows.
    #[must_use]
    pub fn try_from_flat(dim: usize, data: Vec<f32>) -> Option<Self> {
        (dim > 0 && data.len().is_multiple_of(dim)).then_some(Self { dim, data })
    }

    pub(crate) fn empty(dim: usize) -> Self {
        Self {
            dim,
            data: Vec::new(),
        }
    }

    pub(crate) fn extend(&mut self, other: Self) {
        debug_assert_eq!(self.dim, other.dim);
        self.data.extend_from_slice(&other.data);
    }

    #[must_use]
    pub fn dim(&self) -> usize {
        self.dim
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len() / self.dim
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<&[f32]> {
        self.data.chunks_exact(self.dim).nth(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = &[f32]> {
        self.data.chunks_exact(self.dim)
    }

    /// Flat row-major view (for bulk ANN insertion).
    #[must_use]
    pub fn as_flat(&self) -> &[f32] {
        &self.data
    }

    #[must_use]
    pub fn into_flat(self) -> Vec<f32> {
        self.data
    }
}

/// Matryoshka truncation + L2 renormalization of a row-major batch.
///
/// Takes the first `target_dim` components of each `native_dim` row and rescales
/// to unit length (docs/SEARCH_AND_INDEXING.md §4). `index_offset` is added to
/// reported indices so errors point at the caller's input.
///
/// # Errors
/// `OutputShape` if `raw` is not a whole number of rows, `NonFinite`, `ZeroVector`.
pub fn truncate_and_normalize(
    raw: &[f32],
    native_dim: usize,
    target_dim: usize,
    index_offset: usize,
) -> Result<EmbeddingBatch, EmbeddingError> {
    assert!(
        target_dim > 0 && target_dim <= native_dim,
        "target_dim must be in 1..=native_dim (validated by EmbeddingProfile)"
    );
    if !raw.len().is_multiple_of(native_dim) {
        return Err(EmbeddingError::OutputShape {
            expected: raw.len().next_multiple_of(native_dim),
            actual: raw.len(),
        });
    }
    let rows = raw.len() / native_dim;
    let mut out = Vec::with_capacity(rows * target_dim);
    for (row, chunk) in raw.chunks_exact(native_dim).enumerate() {
        let index = index_offset + row;
        let head = &chunk[..target_dim];
        if head.iter().any(|v| !v.is_finite()) {
            return Err(EmbeddingError::NonFinite { index });
        }
        // f64 accumulation: 768 squared terms in f32 lose precision noticeably.
        let norm = head
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>()
            .sqrt();
        if norm < MIN_NORM {
            return Err(EmbeddingError::ZeroVector { index });
        }
        #[allow(clippy::cast_possible_truncation)]
        out.extend(head.iter().map(|&v| (f64::from(v) / norm) as f32));
    }
    Ok(EmbeddingBatch::from_flat(target_dim, out))
}

/// Dot product; equals cosine similarity for normalized vectors.
///
/// # Panics
/// If lengths differ.
#[must_use]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "dot of vectors with different dimensions");
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(v: &[f32]) -> f32 {
        dot(v, v).sqrt()
    }

    #[test]
    fn truncates_then_normalizes_each_row() {
        // Two rows of native dim 4, truncated to 2.
        let raw = [3.0, 4.0, 100.0, -7.0, 0.0, 2.0, 9.0, 9.0];
        let batch = truncate_and_normalize(&raw, 4, 2, 0).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.dim(), 2);
        assert_eq!(batch.get(0).unwrap(), &[0.6, 0.8]);
        assert_eq!(batch.get(1).unwrap(), &[0.0, 1.0]);
        assert!(batch.get(2).is_none());
        for v in batch.iter() {
            assert!((norm(v) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn full_dimension_is_plain_normalization() {
        let raw = [1.0, 1.0, 1.0, 1.0];
        let batch = truncate_and_normalize(&raw, 4, 4, 0).unwrap();
        assert_eq!(batch.as_flat(), &[0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn rejects_degenerate_rows_with_caller_indices() {
        let zero_head = [0.0, 0.0, 5.0, 5.0];
        assert_eq!(
            truncate_and_normalize(&zero_head, 4, 2, 10),
            Err(EmbeddingError::ZeroVector { index: 10 })
        );
        let nan = [1.0, 1.0, 1.0, 1.0, f32::NAN, 1.0, 1.0, 1.0];
        assert_eq!(
            truncate_and_normalize(&nan, 4, 2, 3),
            Err(EmbeddingError::NonFinite { index: 4 })
        );
        // Non-finite values outside the kept head do not matter.
        let tail_inf = [1.0, 0.0, f32::INFINITY, 0.0];
        assert!(truncate_and_normalize(&tail_inf, 4, 2, 0).is_ok());
    }

    #[test]
    fn rejects_partial_rows() {
        assert_eq!(
            truncate_and_normalize(&[1.0; 5], 4, 2, 0),
            Err(EmbeddingError::OutputShape {
                expected: 8,
                actual: 5
            })
        );
    }

    #[test]
    fn dot_of_normalized_is_cosine() {
        let a = [0.6, 0.8];
        let b = [0.8, 0.6];
        assert!((dot(&a, &a) - 1.0).abs() < 1e-6);
        assert!((dot(&a, &b) - 0.96).abs() < 1e-6);
    }
}
