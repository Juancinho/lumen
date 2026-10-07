//! Latency statistics (nearest-rank percentiles, milliseconds).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Summary {
    pub(crate) n: usize,
    pub(crate) min_ms: f64,
    pub(crate) mean_ms: f64,
    pub(crate) p50_ms: f64,
    pub(crate) p95_ms: f64,
    pub(crate) p99_ms: f64,
    pub(crate) max_ms: f64,
}

/// Nearest-rank percentile of an ascending-sorted, non-empty slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    debug_assert!(!sorted.is_empty() && (0.0..=100.0).contains(&p));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

impl Summary {
    /// `None` for an empty sample.
    pub(crate) fn of(samples_ms: &[f64]) -> Option<Self> {
        if samples_ms.is_empty() {
            return None;
        }
        let mut sorted = samples_ms.to_vec();
        sorted.sort_by(f64::total_cmp);
        #[allow(clippy::cast_precision_loss)]
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        Some(Self {
            n: sorted.len(),
            min_ms: sorted[0],
            mean_ms: mean,
            p50_ms: percentile(&sorted, 50.0),
            p95_ms: percentile(&sorted, 95.0),
            p99_ms: percentile(&sorted, 99.0),
            max_ms: sorted[sorted.len() - 1],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = Summary::of(&samples).unwrap();
        assert_eq!((s.n, s.min_ms, s.max_ms), (100, 1.0, 100.0));
        assert_eq!((s.p50_ms, s.p95_ms, s.p99_ms), (50.0, 95.0, 99.0));
        assert!((s.mean_ms - 50.5).abs() < 1e-9);
    }

    #[test]
    fn order_independent_and_small_samples() {
        let s = Summary::of(&[30.0, 10.0, 20.0]).unwrap();
        assert_eq!((s.p50_ms, s.p95_ms, s.p99_ms), (20.0, 30.0, 30.0));
        let one = Summary::of(&[7.0]).unwrap();
        assert_eq!((one.p50_ms, one.p99_ms), (7.0, 7.0));
        assert!(Summary::of(&[]).is_none());
    }
}
