//! Deterministic synthetic vector datasets for the ANN benchmark (T008).
//!
//! `EmbeddingLike` mimics the geometry of real EmbeddingGemma 2 vectors measured on the
//! fidelity corpus (`fixtures/embedding/`): strongly anisotropic (norm of the mean vector
//! 0.76) with pairwise cosine ≈ 0.58 ± 0.06. Points are
//! `normalize(a·g + b·c_k + c·noise)` with a shared direction `g`, cluster centres `c_k` and
//! isotropic noise; a² = 0.57, b² = 0.18, c² = 0.25 gives ≈ 0.57 cosine across clusters and
//! ≈ 0.75 within a cluster. `Uniform` (random directions) is the HNSW worst case.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dataset {
    EmbeddingLike,
    Uniform,
}

impl Dataset {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "embedding-like" => Some(Self::EmbeddingLike),
            "uniform" => Some(Self::Uniform),
            _ => None,
        }
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmbeddingLike => "embedding-like",
            Self::Uniform => "uniform",
        })
    }
}

/// SplitMix64 + Box–Muller. Deterministic across platforms.
pub(crate) struct Rng {
    state: u64,
    spare: Option<f32>,
}

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
            spare: None,
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1].
    fn unit(&mut self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let x = ((self.next_u64() >> 11) + 1) as f64 / (1_u64 << 53) as f64;
        x
    }

    pub(crate) fn gaussian(&mut self) -> f32 {
        if let Some(s) = self.spare.take() {
            return s;
        }
        let r = (-2.0 * self.unit().ln()).sqrt();
        let theta = std::f64::consts::TAU * self.unit();
        #[allow(clippy::cast_possible_truncation)]
        {
            self.spare = Some((r * theta.sin()) as f32);
            (r * theta.cos()) as f32
        }
    }
}

fn normalize(v: &mut [f32]) {
    let n = v
        .iter()
        .map(|x| f64::from(*x) * f64::from(*x))
        .sum::<f64>()
        .sqrt();
    if n > 0.0 {
        #[allow(clippy::cast_possible_truncation)]
        let inv = (1.0 / n) as f32;
        v.iter_mut().for_each(|x| *x *= inv);
    }
}

fn random_unit(rng: &mut Rng, dim: usize) -> Vec<f32> {
    let mut v: Vec<f32> = (0..dim).map(|_| rng.gaussian()).collect();
    normalize(&mut v);
    v
}

/// Generator with fixed structure (shared direction + cluster centres) so that dataset and
/// queries come from the same distribution.
pub(crate) struct Generator {
    kind: Dataset,
    dim: usize,
    global: Vec<f32>,
    centres: Vec<Vec<f32>>,
}

const A: f32 = 0.754_983_4; // sqrt(0.57)
const B: f32 = 0.424_264_07; // sqrt(0.18)
const C: f32 = 0.5; // sqrt(0.25)

impl Generator {
    /// `clusters` only matters for `EmbeddingLike`.
    pub(crate) fn new(kind: Dataset, dim: usize, clusters: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let global = random_unit(&mut rng, dim);
        let centres = (0..clusters.max(1))
            .map(|_| random_unit(&mut rng, dim))
            .collect();
        Self {
            kind,
            dim,
            global,
            centres,
        }
    }

    /// `n` normalized vectors, row-major.
    pub(crate) fn sample(&self, n: usize, seed: u64) -> Vec<f32> {
        let mut rng = Rng::new(seed);
        let mut out = Vec::with_capacity(n * self.dim);
        for _ in 0..n {
            let noise = random_unit(&mut rng, self.dim);
            match self.kind {
                Dataset::Uniform => out.extend_from_slice(&noise),
                Dataset::EmbeddingLike => {
                    #[allow(clippy::cast_possible_truncation)]
                    let k = (rng.next_u64() % self.centres.len() as u64) as usize;
                    let c = &self.centres[k];
                    let mut v: Vec<f32> = (0..self.dim)
                        .map(|i| A * self.global[i] + B * c[i] + C * noise[i])
                        .collect();
                    normalize(&mut v);
                    out.extend_from_slice(&v);
                }
            }
        }
        out
    }
}

/// Exact top-`k` keys (row indices) by inner product for each query; multi-threaded.
pub(crate) fn ground_truth(data: &[f32], queries: &[f32], dim: usize, k: usize) -> Vec<Vec<u64>> {
    let nq = queries.len() / dim;
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(nq.max(1));
    let per = nq.div_ceil(threads.max(1));
    let mut results: Vec<Vec<u64>> = vec![Vec::new(); nq];
    std::thread::scope(|s| {
        for (chunk_index, chunk) in results.chunks_mut(per.max(1)).enumerate() {
            s.spawn(move || {
                for (offset, slot) in chunk.iter_mut().enumerate() {
                    let qi = chunk_index * per + offset;
                    let q = &queries[qi * dim..(qi + 1) * dim];
                    let mut best: Vec<(f32, u64)> = Vec::with_capacity(k + 1);
                    for (row, v) in data.chunks_exact(dim).enumerate() {
                        let score: f32 = q.iter().zip(v).map(|(a, b)| a * b).sum();
                        if best.len() < k || score > best[best.len() - 1].0 {
                            let pos = best.partition_point(|(s2, _)| *s2 >= score);
                            best.insert(pos, (score, row as u64));
                            best.truncate(k);
                        }
                    }
                    *slot = best.into_iter().map(|(_, key)| key).collect();
                }
            });
        }
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn deterministic_and_normalized() {
        let g = Generator::new(Dataset::EmbeddingLike, 32, 4, 7);
        let a = g.sample(10, 1);
        assert_eq!(a, g.sample(10, 1));
        assert_ne!(a, g.sample(10, 2));
        for v in a.chunks_exact(32) {
            assert!((cos(v, v) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn embedding_like_matches_measured_geometry() {
        // 256d, many clusters: mean pairwise cosine should land near the 0.58 measured on
        // real EmbeddingGemma 2 vectors.
        let g = Generator::new(Dataset::EmbeddingLike, 256, 500, 3);
        let v = g.sample(400, 9);
        let rows: Vec<&[f32]> = v.chunks_exact(256).collect();
        let mut sum = 0.0;
        let mut n = 0.0;
        for i in 0..rows.len() {
            for j in i + 1..rows.len() {
                sum += cos(rows[i], rows[j]);
                n += 1.0;
            }
        }
        let mean = sum / n;
        assert!((0.53..0.63).contains(&mean), "mean pairwise cosine {mean}");
        let u = Generator::new(Dataset::Uniform, 256, 1, 3).sample(50, 9);
        let r: Vec<&[f32]> = u.chunks_exact(256).collect();
        assert!(cos(r[0], r[1]).abs() < 0.3);
    }

    #[test]
    fn ground_truth_finds_exact_neighbours() {
        let dim = 16;
        let g = Generator::new(Dataset::Uniform, dim, 1, 5);
        let data = g.sample(200, 1);
        // Query = an existing row: it must be its own nearest neighbour.
        let q: Vec<f32> = data[37 * dim..38 * dim].to_vec();
        let truth = ground_truth(&data, &q, dim, 5);
        assert_eq!(truth[0][0], 37);
        assert_eq!(truth[0].len(), 5);
    }
}
