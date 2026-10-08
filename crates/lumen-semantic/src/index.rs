//! Persistent ANN generations (T203, ADR-031, docs/SEARCH_AND_INDEXING.md §16).
//!
//! The vectors of a generation live in SQLite (`chunk_vectors`, ADR-029). Search reads them
//! through two derived structures:
//!
//! - **the ANN file** — an HNSW index (`lumen-vector`, ADR-016) built from a consistent
//!   snapshot (every row with `seq <= built_through_seq`), written to a temporary path,
//!   renamed into place and then recorded in `ann_files`; opened memory-mapped and
//!   read-only;
//! - **the delta** — rows written after that snapshot, held in memory and searched exactly.
//!
//! Every candidate is checked against the canonical rows before it is returned: a file hit
//! whose row is gone (chunk deleted) or newer (chunk rewritten; SQLite may reuse a deleted
//! chunk id) is dropped, as is a delta row that was rewritten again. A missing, unreadable
//! or mismatching file degrades to "rebuild needed", never to wrong results.
//!
//! [`SemanticIndex::maintenance`] says when to rebuild: a delta past a fraction of the file,
//! or a file whose rows mostly changed. [`build_file`] does the build (on the indexing
//! thread); [`validate`] decides whether a building generation may replace the active one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lumen_core::CancellationToken;
use lumen_storage::{AnnFileRecord, GenerationInfo, StorageError, Store};
use lumen_vector::{HnswParams, IndexConfig, Scalar, VectorError, VectorIndex};

/// Rows read per page while building or loading the delta.
const PAGE: usize = 4096;
/// File name prefix of ANN files in the vector folder.
const FILE_PREFIX: &str = "gen-";
const FILE_SUFFIX: &str = ".usearch";

/// The ANN configuration of ADR-016 for `dim`-dimensional vectors.
#[must_use]
pub fn ann_config(dim: usize) -> IndexConfig {
    IndexConfig {
        params: HnswParams {
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 256,
        },
        ..IndexConfig::new(dim, Scalar::F16)
    }
}

/// When the delta / stale rows justify a rebuild, and how much delta is held at most.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexSettings {
    /// Rebuild once the delta has at least this many rows…
    pub rebuild_delta_min: usize,
    /// …and at least this fraction of the file's vectors (or there is no file).
    pub rebuild_delta_fraction: f64,
    /// Rebuild once this fraction of the file's vectors were deleted or rewritten.
    pub rebuild_stale_fraction: f64,
    /// Most rows held in memory (~1 KiB each at 256 d); beyond it search reports
    /// `incomplete` until a file is built.
    pub delta_limit: usize,
}

impl Default for IndexSettings {
    fn default() -> Self {
        Self {
            rebuild_delta_min: 2_000,
            rebuild_delta_fraction: 0.10,
            rebuild_stale_fraction: 0.20,
            delta_limit: 50_000,
        }
    }
}

#[derive(Debug)]
pub enum IndexError {
    Storage(StorageError),
    Vector(VectorError),
    Io(std::io::Error),
    Cancelled,
    /// Query vector of the wrong dimension, or a generation without vectors to build.
    Invalid(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "storage: {e}"),
            Self::Vector(e) => write!(f, "ann: {e}"),
            Self::Io(e) => write!(f, "ann file: {e}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<StorageError> for IndexError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

impl From<VectorError> for IndexError {
    fn from(e: VectorError) -> Self {
        Self::Vector(e)
    }
}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Why there is no usable ANN file (diagnostics; never fatal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Current,
    /// Never built, or recorded for another configuration.
    Missing,
    /// Recorded but missing on disk, unreadable or with the wrong vector count.
    Unusable(String),
}

/// One semantic hit: a chunk and its cosine similarity to the query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticHit {
    pub chunk_id: i64,
    pub similarity: f32,
}

/// Counts for status and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStatus {
    pub generation: i64,
    pub file: FileState,
    pub file_vectors: usize,
    pub delta_vectors: usize,
    /// The delta hit [`IndexSettings::delta_limit`]: some vectors are not searchable yet.
    pub incomplete: bool,
}

/// What the indexing thread should do next for this generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maintenance {
    None,
    Rebuild,
}

struct LoadedFile {
    index: VectorIndex,
    record: AnnFileRecord,
}

/// Rows written after the file's snapshot, searched exactly.
#[derive(Default)]
struct Delta {
    chunk_ids: Vec<i64>,
    seqs: Vec<i64>,
    /// Row-major, `dim` floats per row.
    vectors: Vec<f32>,
    /// Row of each chunk id (a chunk rewritten twice keeps only its newest row).
    rows: HashMap<i64, usize>,
    /// Highest `seq` loaded.
    through_seq: i64,
    incomplete: bool,
}

impl Delta {
    fn len(&self) -> usize {
        self.chunk_ids.len()
    }

    fn push(&mut self, dim: usize, chunk_id: i64, seq: i64, vector: &[f32]) {
        if let Some(&row) = self.rows.get(&chunk_id) {
            self.seqs[row] = seq;
            self.vectors[row * dim..(row + 1) * dim].copy_from_slice(vector);
        } else {
            self.rows.insert(chunk_id, self.chunk_ids.len());
            self.chunk_ids.push(chunk_id);
            self.seqs.push(seq);
            self.vectors.extend_from_slice(vector);
        }
        self.through_seq = self.through_seq.max(seq);
    }

    /// Exact top-`k` by dot product (vectors are L2-normalized).
    fn search(&self, dim: usize, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        let mut scored: Vec<(usize, f32)> = self
            .vectors
            .chunks_exact(dim)
            .enumerate()
            .map(|(row, v)| (row, v.iter().zip(query).map(|(a, b)| a * b).sum()))
            .collect();
        let k = k.min(scored.len());
        if k == 0 {
            return Vec::new();
        }
        scored.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
        scored.truncate(k);
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
    }
}

/// The searchable state of one generation: ANN file + delta. Search takes `&self` and is
/// safe from several threads; refresh and reopen take `&mut self` (behind a lock).
pub struct SemanticIndex {
    generation: GenerationInfo,
    config: IndexConfig,
    dir: PathBuf,
    settings: IndexSettings,
    file: Option<LoadedFile>,
    file_state: FileState,
    delta: Delta,
}

impl std::fmt::Debug for SemanticIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticIndex")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl SemanticIndex {
    /// Opens `generation`: maps its recorded ANN file when it is usable, then loads the
    /// delta. Never fails because of the file (see [`FileState`]).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn open(
        store: &Store,
        dir: &Path,
        generation: GenerationInfo,
        settings: IndexSettings,
    ) -> Result<Self, IndexError> {
        let config = ann_config(generation.dim);
        let mut index = Self {
            config,
            dir: dir.to_owned(),
            settings,
            file: None,
            file_state: FileState::Missing,
            delta: Delta::default(),
            generation,
        };
        index.reopen_file(store)?;
        Ok(index)
    }

    /// Re-reads the generation's file record (after a rebuild) and reloads the delta on
    /// top of it.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn reopen_file(&mut self, store: &Store) -> Result<(), IndexError> {
        self.file = None;
        self.file_state = FileState::Missing;
        if let Some(record) = store.ann_file(self.generation.id)?
            && record.index_config == self.config.fingerprint()
        {
            let path = self.dir.join(&record.file_name);
            match VectorIndex::view(&path, self.config) {
                Ok(index) if index.len() as u64 == record.vectors => {
                    index.set_expansion_search(self.config.params.expansion_search);
                    self.file = Some(LoadedFile { index, record });
                    self.file_state = FileState::Current;
                }
                Ok(index) => {
                    self.file_state = FileState::Unusable(format!(
                        "{} vectors in the file, {} recorded",
                        index.len(),
                        record.vectors
                    ));
                }
                Err(e) => self.file_state = FileState::Unusable(e.to_string()),
            }
        }
        self.delta = Delta {
            through_seq: self.file.as_ref().map_or(0, |f| f.record.built_through_seq),
            ..Delta::default()
        };
        self.refresh(store)?;
        Ok(())
    }

    /// Loads rows written since the last refresh into the delta. Returns rows loaded.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn refresh(&mut self, store: &Store) -> Result<usize, IndexError> {
        let dim = self.config.dim;
        let mut loaded = 0;
        loop {
            let room = self.settings.delta_limit.saturating_sub(self.delta.len());
            if room == 0 {
                // More rows may exist; search stays correct for what is loaded.
                let more = !store
                    .vectors_after_seq(self.generation.id, self.delta.through_seq, 1)?
                    .is_empty();
                self.delta.incomplete = more;
                break;
            }
            let rows = store.vectors_after_seq(
                self.generation.id,
                self.delta.through_seq,
                room.min(PAGE),
            )?;
            if rows.is_empty() {
                self.delta.incomplete = false;
                break;
            }
            for r in &rows {
                if r.vector.len() != dim {
                    return Err(IndexError::Invalid(format!(
                        "stored vector of {} dims in a {dim}-dim generation",
                        r.vector.len()
                    )));
                }
                self.delta.push(dim, r.chunk_id, r.seq, &r.vector);
            }
            loaded += rows.len();
        }
        Ok(loaded)
    }

    #[must_use]
    pub fn generation(&self) -> &GenerationInfo {
        &self.generation
    }

    #[must_use]
    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            generation: self.generation.id,
            file: self.file_state.clone(),
            file_vectors: self.file.as_ref().map_or(0, |f| f.index.len()),
            delta_vectors: self.delta.len(),
            incomplete: self.delta.incomplete,
        }
    }

    /// Up to `k` chunks most similar to `query` (normalized, the generation's dimension),
    /// best first, every one checked against the canonical rows.
    ///
    /// # Errors
    /// Wrong dimension; ANN or SQLite failure.
    pub fn search(
        &self,
        store: &Store,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<SemanticHit>, IndexError> {
        let dim = self.config.dim;
        if query.len() != dim {
            return Err(IndexError::Invalid(format!(
                "query has {} dimensions, generation {} has {dim}",
                query.len(),
                self.generation.id
            )));
        }
        if k == 0 {
            return Ok(Vec::new());
        }
        // Over-fetch: some file candidates may be stale.
        let mut fetch = k + k / 2 + 8;
        loop {
            let (hits, file_exhausted) = self.candidates(store, query, k, fetch)?;
            if hits.len() >= k || file_exhausted {
                return Ok(hits);
            }
            fetch *= 4;
        }
    }

    /// Valid hits from `fetch` file candidates and the delta; whether the file had fewer
    /// than `fetch` candidates (asking for more would not help).
    fn candidates(
        &self,
        store: &Store,
        query: &[f32],
        k: usize,
        fetch: usize,
    ) -> Result<(Vec<SemanticHit>, bool), IndexError> {
        let dim = self.config.dim;
        let (file_hits, exhausted) = match &self.file {
            Some(f) => {
                let n = f.index.search(query, fetch)?;
                let exhausted = n.len() < fetch;
                (n, exhausted)
            }
            None => (Vec::new(), true),
        };
        let delta_hits = self.delta.search(dim, query, k);
        let mut ids: Vec<i64> = file_hits
            .iter()
            .filter_map(|n| i64::try_from(n.key).ok())
            .chain(delta_hits.iter().map(|&(row, _)| self.delta.chunk_ids[row]))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let seqs = store.vector_seqs(self.generation.id, &ids)?;
        let through = self.file.as_ref().map_or(0, |f| f.record.built_through_seq);

        let mut seen = HashSet::new();
        let mut hits: Vec<SemanticHit> = Vec::with_capacity(k * 2);
        for n in &file_hits {
            let Ok(id) = i64::try_from(n.key) else {
                continue;
            };
            if seqs.get(&id).is_some_and(|&s| s <= through) && seen.insert(id) {
                hits.push(SemanticHit {
                    chunk_id: id,
                    similarity: n.similarity(),
                });
            }
        }
        for &(row, similarity) in &delta_hits {
            let id = self.delta.chunk_ids[row];
            if seqs.get(&id) == Some(&self.delta.seqs[row]) && seen.insert(id) {
                hits.push(SemanticHit {
                    chunk_id: id,
                    similarity,
                });
            }
        }
        hits.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        hits.truncate(k);
        Ok((hits, exhausted))
    }

    /// Whether the file should be rebuilt now (indexing thread, between queue slices).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn maintenance(&self, store: &Store) -> Result<Maintenance, IndexError> {
        let s = self.settings;
        let Some(f) = &self.file else {
            let stored = store.vector_count(self.generation.id)?;
            return Ok(
                if stored > 0 && (self.delta.incomplete || stored as usize >= s.rebuild_delta_min) {
                    Maintenance::Rebuild
                } else {
                    Maintenance::None
                },
            );
        };
        #[allow(clippy::cast_precision_loss)]
        let file_len = f.index.len() as f64;
        #[allow(clippy::cast_precision_loss)]
        let delta = self.delta.len() as f64;
        if self.delta.incomplete
            || (self.delta.len() >= s.rebuild_delta_min
                && delta >= s.rebuild_delta_fraction * file_len)
        {
            return Ok(Maintenance::Rebuild);
        }
        let current = store.vector_count_through(self.generation.id, f.record.built_through_seq)?;
        #[allow(clippy::cast_precision_loss)]
        let stale = file_len - current as f64;
        Ok(
            if file_len > 0.0 && stale >= s.rebuild_stale_fraction * file_len {
                Maintenance::Rebuild
            } else {
                Maintenance::None
            },
        )
    }
}

/// Builds the ANN file of `generation` from the vectors stored so far, writes it into
/// `dir` atomically and returns the record to store with [`Store::set_ann_file`]. Reads in
/// pages; `cancel` is checked between pages.
///
/// # Errors
/// [`IndexError::Cancelled`]; no stored vectors; ANN, I/O or SQLite failure.
pub fn build_file(
    store: &Store,
    dir: &Path,
    generation: i64,
    cancel: &CancellationToken,
    now_ms: i64,
) -> Result<AnnFileRecord, IndexError> {
    let info = store
        .generations()?
        .into_iter()
        .find(|g| g.id == generation)
        .ok_or_else(|| IndexError::Invalid(format!("no generation {generation}")))?;
    let through = info.max_seq;
    let config = ann_config(info.dim);
    let expected = store.vector_count_through(generation, through)?;
    let index = VectorIndex::new(config)?;
    index.reserve(usize::try_from(expected).unwrap_or(0).max(1), 1)?;
    let mut after = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(IndexError::Cancelled);
        }
        let page = store.vectors_through(generation, after, through, PAGE)?;
        let Some(last) = page.last() else { break };
        after = last.0;
        // Rows of the snapshot can only disappear (rewrites get newer seqs), so the
        // reservation above is enough.
        for (chunk_id, v) in &page {
            let key = u64::try_from(*chunk_id)
                .map_err(|_| IndexError::Invalid(format!("chunk id {chunk_id}")))?;
            index.add(key, v)?;
        }
    }
    std::fs::create_dir_all(dir)?;
    let file_name = file_name(generation, through);
    let path = dir.join(&file_name);
    let tmp = dir.join(format!("{file_name}.tmp"));
    index.save(&tmp)?;
    // Same name = same snapshot. An existing file is replaced (it may be a corrupt
    // leftover); if a reader has it mapped (Windows refuses), that mapped file is a
    // successfully opened copy of this very snapshot and stays.
    if path.exists() && std::fs::remove_file(&path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    } else {
        std::fs::rename(&tmp, &path)?;
    }
    Ok(AnnFileRecord {
        generation,
        file_name,
        built_through_seq: through,
        vectors: index.len() as u64,
        index_config: config.fingerprint(),
        built_at: now_ms,
    })
}

/// `gen-<generation>-<built_through_seq>.usearch`: a new name per build, so the file a
/// reader has mapped is never overwritten (Windows cannot replace a mapped file).
#[must_use]
pub fn file_name(generation: i64, through_seq: i64) -> String {
    format!("{FILE_PREFIX}{generation}-{through_seq}{FILE_SUFFIX}")
}

/// Deletes ANN files (and leftover `.tmp` files) in `dir` that no generation records.
/// Files still mapped by a reader fail to delete on Windows and are retried next time.
/// Returns how many were deleted.
///
/// # Errors
/// SQLite failure (I/O errors are skipped).
pub fn cleanup_files(store: &Store, dir: &Path) -> Result<usize, IndexError> {
    let keep: HashSet<String> = store.ann_file_names()?.into_iter().collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut deleted = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = name.starts_with(FILE_PREFIX)
            && (name.ends_with(FILE_SUFFIX) || name.ends_with(".tmp"));
        if ours && !keep.contains(&name) && std::fs::remove_file(entry.path()).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Whether a building generation may replace the active one.
#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    /// Every chunk has a result in the generation.
    pub complete: bool,
    pub chunks: u64,
    pub failed: u64,
    /// Sampled stored vectors that find themselves first (1.0 = all).
    pub self_recall: f64,
    pub sampled: usize,
    pub ok: bool,
}

/// At most this share of chunks may have failed.
const MAX_FAILED_FRACTION: f64 = 0.01;
/// A different chunk this similar is a duplicate (f16 storage keeps self-similarity
/// within ~1e-3 of 1).
const DUPLICATE_SIMILARITY: f32 = 0.999;
/// Sampled vectors must find themselves at least this often.
const MIN_SELF_RECALL: f64 = 0.95;

/// Checks `index` (a building generation, opened after its file was built): every chunk
/// has a result, few failed, and `sample` stored vectors spread over the generation each
/// find their own chunk first.
///
/// # Errors
/// ANN or SQLite failure.
pub fn validate(
    store: &Store,
    index: &SemanticIndex,
    sample: usize,
) -> Result<Validation, IndexError> {
    let g = index.generation().id;
    let counts = store.queue_counts(g)?;
    let complete = counts.pending() == 0;
    let stored = store.vector_count(g)?;
    let step = usize::try_from(stored).unwrap_or(0) / sample.max(1);
    let mut probes = Vec::new();
    let mut after = 0;
    while probes.len() < sample {
        let page = store.vectors_through(g, after, i64::MAX, step.max(1))?;
        let Some(last) = page.last() else { break };
        after = last.0;
        probes.push(last.clone());
    }
    let mut found = 0_u32;
    for (chunk_id, v) in &probes {
        let hits = index.search(store, v, 1)?;
        // An identical vector (duplicate text) ranking first counts as found.
        if hits
            .first()
            .is_some_and(|h| h.chunk_id == *chunk_id || h.similarity >= DUPLICATE_SIMILARITY)
        {
            found += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let self_recall = if probes.is_empty() {
        0.0
    } else {
        f64::from(found) / probes.len() as f64
    };
    #[allow(clippy::cast_precision_loss)]
    let failed_ok =
        counts.chunks == 0 || (counts.failed as f64) <= MAX_FAILED_FRACTION * counts.chunks as f64;
    Ok(Validation {
        complete,
        chunks: counts.chunks,
        failed: counts.failed,
        self_recall,
        sampled: probes.len(),
        ok: complete && failed_ok && !probes.is_empty() && self_recall >= MIN_SELF_RECALL,
    })
}
