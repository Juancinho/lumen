//! Approximate-nearest-neighbour index for Lumen's semantic layer (ADR-004).
//!
//! A thin, typed wrapper over USearch's HNSW index. The ANN index is **derived data**: SQLite
//! stays canonical (ADR-003) and an index can always be rebuilt from stored chunks. Keys are
//! `VectorId`s (u64) that the storage layer maps to chunks; generation management and
//! persistence policy are T203.
//!
//! Inputs are always `f32` (what the embedder produces); the index stores them in the
//! configured [`Scalar`] type (f32/f16/bf16/i8 quantization happens inside USearch).

#![forbid(unsafe_code)]

use std::fmt;
use std::path::Path;

use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

/// Storage type of vectors inside the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scalar {
    F32,
    F16,
    BF16,
    I8,
}

impl Scalar {
    pub const ALL: [Self; 4] = [Self::F32, Self::F16, Self::BF16, Self::I8];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F16 => "f16",
            Self::BF16 => "bf16",
            Self::I8 => "i8",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == s)
    }

    /// Bytes per stored component (vector payload only, excluding graph links).
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::F32 => 4,
            Self::F16 | Self::BF16 => 2,
            Self::I8 => 1,
        }
    }

    const fn kind(self) -> ScalarKind {
        match self {
            Self::F32 => ScalarKind::F32,
            Self::F16 => ScalarKind::F16,
            Self::BF16 => ScalarKind::BF16,
            Self::I8 => ScalarKind::I8,
        }
    }
}

/// Similarity metric. Lumen's vectors are L2-normalized, so both rank identically; inner
/// product skips norm computation, cosine is required for `i8` quantization to be meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    Cosine,
    InnerProduct,
}

impl Metric {
    const fn kind(self) -> MetricKind {
        match self {
            Self::Cosine => MetricKind::Cos,
            Self::InnerProduct => MetricKind::IP,
        }
    }
}

/// HNSW parameters. Defaults are a starting point; T008 measures them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HnswParams {
    /// Links per node (`M`). Higher = better recall, more memory, slower build.
    pub connectivity: usize,
    /// Candidate list size while inserting (`ef_construction`).
    pub expansion_add: usize,
    /// Candidate list size while searching (`ef`); adjustable at runtime.
    pub expansion_search: usize,
}

impl Default for HnswParams {
    fn default() -> Self {
        Self {
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
        }
    }
}

/// Full index configuration. Part of the index generation identity (T203).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexConfig {
    pub dim: usize,
    pub metric: Metric,
    pub scalar: Scalar,
    pub params: HnswParams,
}

impl IndexConfig {
    /// ADR-006 profile: 256d, cosine, given storage type, default HNSW params.
    #[must_use]
    pub fn new(dim: usize, scalar: Scalar) -> Self {
        Self {
            dim,
            metric: Metric::Cosine,
            scalar,
            params: HnswParams::default(),
        }
    }

    fn options(&self) -> IndexOptions {
        IndexOptions {
            dimensions: self.dim,
            metric: self.metric.kind(),
            quantization: self.scalar.kind(),
            connectivity: self.params.connectivity,
            expansion_add: self.params.expansion_add,
            expansion_search: self.params.expansion_search,
            multi: false,
        }
    }
}

/// Index errors (messages come from USearch or argument validation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorError(String);

impl VectorError {
    fn from_cxx(context: &str, err: impl fmt::Display) -> Self {
        Self(format!("{context}: {err}"))
    }
}

impl fmt::Display for VectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VectorError {}

/// One search hit. `distance` is USearch's distance (`1 - cosine` for [`Metric::Cosine`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbor {
    pub key: u64,
    pub distance: f32,
}

impl Neighbor {
    /// Similarity in `[-1, 1]` for cosine / inner product on normalized vectors.
    #[must_use]
    pub fn similarity(&self) -> f32 {
        1.0 - self.distance
    }
}

/// An HNSW index. Thread-safe: `add`, `search` and `remove` may be called concurrently
/// after [`VectorIndex::reserve`] was given enough capacity and threads.
pub struct VectorIndex {
    inner: Index,
    config: IndexConfig,
}

impl fmt::Debug for VectorIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VectorIndex")
            .field("config", &self.config)
            .field("len", &self.len())
            .finish()
    }
}

impl VectorIndex {
    /// # Errors
    /// Invalid configuration (zero dimension, unsupported combination).
    pub fn new(config: IndexConfig) -> Result<Self, VectorError> {
        if config.dim == 0 {
            return Err(VectorError("dimension must be > 0".into()));
        }
        let inner =
            Index::new(&config.options()).map_err(|e| VectorError::from_cxx("create index", e))?;
        Ok(Self { inner, config })
    }

    /// Opens a saved index fully into memory.
    ///
    /// # Errors
    /// Missing/corrupt file or a file built with a different configuration.
    pub fn load(path: &Path, config: IndexConfig) -> Result<Self, VectorError> {
        let index = Self::new(config)?;
        index
            .inner
            .load(&path_str(path)?)
            .map_err(|e| VectorError::from_cxx("load index", e))?;
        index.check_loaded()?;
        Ok(index)
    }

    /// Memory-maps a saved index read-only (fast open; pages fault in on first use).
    /// The index cannot be modified.
    ///
    /// # Errors
    /// As [`VectorIndex::load`].
    pub fn view(path: &Path, config: IndexConfig) -> Result<Self, VectorError> {
        let index = Self::new(config)?;
        index
            .inner
            .view(&path_str(path)?)
            .map_err(|e| VectorError::from_cxx("view index", e))?;
        index.check_loaded()?;
        Ok(index)
    }

    fn check_loaded(&self) -> Result<(), VectorError> {
        if self.inner.dimensions() != self.config.dim
            || self.inner.scalar_kind() != self.config.scalar.kind()
        {
            return Err(VectorError(
                "index file does not match the expected configuration".into(),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn config(&self) -> &IndexConfig {
        &self.config
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.size()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pre-allocates for `capacity` vectors and `threads` concurrent writers/readers.
    ///
    /// # Errors
    /// Allocation failure.
    pub fn reserve(&self, capacity: usize, threads: usize) -> Result<(), VectorError> {
        self.inner
            .reserve_capacity_and_threads(capacity, threads.max(1))
            .map_err(|e| VectorError::from_cxx("reserve", e))
    }

    fn check_dim(&self, v: &[f32]) -> Result<(), VectorError> {
        if v.len() == self.config.dim {
            Ok(())
        } else {
            Err(VectorError(format!(
                "vector has {} dimensions, index expects {}",
                v.len(),
                self.config.dim
            )))
        }
    }

    /// Inserts `vector` under `key`. Keys must be unique (`multi = false`); remove first to
    /// update.
    ///
    /// # Errors
    /// Wrong dimension, duplicate key, capacity exhausted.
    pub fn add(&self, key: u64, vector: &[f32]) -> Result<(), VectorError> {
        self.check_dim(vector)?;
        if self.inner.contains(key) {
            return Err(VectorError(format!("key {key} already present")));
        }
        self.inner
            .add(key, vector)
            .map_err(|e| VectorError::from_cxx("add", e))
    }

    /// Up to `k` approximate nearest neighbours, best first.
    ///
    /// # Errors
    /// Wrong dimension.
    pub fn search(&self, query: &[f32], k: usize) -> Result<Vec<Neighbor>, VectorError> {
        self.check_dim(query)?;
        let matches = self
            .inner
            .search(query, k)
            .map_err(|e| VectorError::from_cxx("search", e))?;
        Ok(matches
            .keys
            .into_iter()
            .zip(matches.distances)
            .map(|(key, distance)| Neighbor { key, distance })
            .collect())
    }

    /// Changes the search candidate list size (recall/latency trade-off).
    pub fn set_expansion_search(&self, ef: usize) {
        self.inner.change_expansion_search(ef);
    }

    /// Removes `key`. Returns whether it was present. USearch marks the slot deleted; space is
    /// reclaimed on rebuild/compaction (T203).
    ///
    /// # Errors
    /// Read-only (viewed) index.
    pub fn remove(&self, key: u64) -> Result<bool, VectorError> {
        self.inner
            .remove(key)
            .map(|n| n > 0)
            .map_err(|e| VectorError::from_cxx("remove", e))
    }

    #[must_use]
    pub fn contains(&self, key: u64) -> bool {
        self.inner.contains(key)
    }

    /// Bytes allocated by the index (graph + vectors).
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.inner.memory_usage()
    }

    /// Bytes the index occupies when saved.
    #[must_use]
    pub fn serialized_bytes(&self) -> usize {
        self.inner.serialized_length()
    }

    /// Saves to `path` (overwrites). Callers write to a temporary path and rename atomically
    /// when promoting a generation (docs/ARCHITECTURE.md §14).
    ///
    /// # Errors
    /// I/O failure.
    pub fn save(&self, path: &Path) -> Result<(), VectorError> {
        self.inner
            .save(&path_str(path)?)
            .map_err(|e| VectorError::from_cxx("save index", e))
    }
}

fn path_str(path: &Path) -> Result<String, VectorError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| VectorError(format!("non-UTF-8 path: {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIM: usize = 8;

    fn unit(i: usize) -> Vec<f32> {
        let mut v = [0.05_f32; DIM];
        v[i % DIM] = 1.0;
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    fn config(scalar: Scalar) -> IndexConfig {
        IndexConfig::new(DIM, scalar)
    }

    #[test]
    fn add_search_round_trip_for_every_scalar() {
        for scalar in Scalar::ALL {
            let index = VectorIndex::new(config(scalar)).unwrap();
            index.reserve(16, 1).unwrap();
            for i in 0..DIM {
                index.add(100 + i as u64, &unit(i)).unwrap();
            }
            assert_eq!(index.len(), DIM);
            let hits = index.search(&unit(3), 3).unwrap();
            assert_eq!(hits[0].key, 103, "{scalar:?}: {hits:?}");
            assert!(hits[0].similarity() > 0.95, "{scalar:?}: {hits:?}");
            assert!(hits.windows(2).all(|w| w[0].distance <= w[1].distance));
        }
    }

    #[test]
    fn validates_dimensions_and_duplicates() {
        let index = VectorIndex::new(config(Scalar::F32)).unwrap();
        index.reserve(4, 1).unwrap();
        assert!(index.add(1, &[1.0; 3]).is_err());
        index.add(1, &unit(0)).unwrap();
        assert!(
            index
                .add(1, &unit(1))
                .unwrap_err()
                .to_string()
                .contains("already present")
        );
        assert!(index.search(&[1.0; 3], 1).is_err());
        assert!(VectorIndex::new(IndexConfig::new(0, Scalar::F32)).is_err());
    }

    #[test]
    fn removed_keys_are_not_returned_and_can_be_re_added() {
        let index = VectorIndex::new(config(Scalar::F16)).unwrap();
        index.reserve(16, 1).unwrap();
        for i in 0..DIM {
            index.add(i as u64, &unit(i)).unwrap();
        }
        assert!(index.remove(2).unwrap());
        assert!(!index.remove(2).unwrap());
        assert!(!index.contains(2));
        let hits = index.search(&unit(2), DIM).unwrap();
        assert!(hits.iter().all(|h| h.key != 2), "{hits:?}");
        index.add(2, &unit(2)).unwrap();
        assert_eq!(index.search(&unit(2), 1).unwrap()[0].key, 2);
    }

    #[test]
    fn save_load_and_view_preserve_results() {
        let dir = std::env::temp_dir().join(format!("lumen-vector-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("index.usearch");
        let index = VectorIndex::new(config(Scalar::F32)).unwrap();
        index.reserve(16, 1).unwrap();
        for i in 0..DIM {
            index.add(i as u64, &unit(i)).unwrap();
        }
        index.save(&path).unwrap();
        assert!(index.serialized_bytes() > 0 && index.memory_bytes() > 0);

        let loaded = VectorIndex::load(&path, config(Scalar::F32)).unwrap();
        let viewed = VectorIndex::view(&path, config(Scalar::F32)).unwrap();
        for i in 0..DIM {
            assert_eq!(loaded.search(&unit(i), 1).unwrap()[0].key, i as u64);
            assert_eq!(viewed.search(&unit(i), 1).unwrap()[0].key, i as u64);
        }
        // A file opened with the wrong configuration is rejected.
        assert!(VectorIndex::load(&path, config(Scalar::F16)).is_err());
        drop(viewed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scalar_names_round_trip() {
        for s in Scalar::ALL {
            assert_eq!(Scalar::parse(s.name()), Some(s));
        }
        assert_eq!(Scalar::F16.bytes(), 2);
    }
}
