//! `lumen-bench ann-gen` (T203): a persistent ANN generation end to end on the real
//! storage path — vectors written to `chunk_vectors`, the HNSW file built from SQLite,
//! opened memory-mapped, searched with the in-memory delta and per-hit validation against
//! SQLite — at a given scale, with recall@k against exact search over the canonical rows.
//!
//! Scenarios: the fresh file; plus a delta of new vectors; plus a share of chunks
//! re-embedded (their file vectors are stale and must be skipped); and the rebuild.
//! Synthetic embedding-like vectors (ADR-016); counts and timings only.

use std::path::PathBuf;
use std::time::Instant;

use lumen_core::CancellationToken;
use lumen_semantic::{IndexSettings, SemanticIndex, build_file};
use lumen_storage::{GenerationSpec, NewChunk, NewItem, Store, VectorWrite};
use serde::Serialize;

use crate::machine::{self, MachineInfo, MemorySnapshot};
use crate::stats::Summary;
use crate::synth::{Dataset, Generator, ground_truth};

#[derive(Debug, Clone)]
pub(crate) struct GenerationOptions {
    pub(crate) vectors: usize,
    pub(crate) delta: usize,
    /// Share of chunks re-embedded after the build (stale file entries).
    pub(crate) rewrite_fraction: f64,
    pub(crate) dim: usize,
    pub(crate) queries: usize,
    pub(crate) k: usize,
    pub(crate) work_dir: PathBuf,
    pub(crate) label: Option<String>,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            vectors: 100_000,
            delta: 5_000,
            rewrite_fraction: 0.05,
            dim: 256,
            queries: 300,
            k: 10,
            work_dir: std::env::temp_dir().join("lumen-bench-ann-gen"),
            label: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Scenario {
    name: &'static str,
    file_vectors: usize,
    delta_vectors: usize,
    search: Summary,
    recall_at_k: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct GenerationReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    vectors: usize,
    delta: usize,
    rewritten: usize,
    dim: usize,
    k: usize,
    write_vectors_per_s: f64,
    build_s: f64,
    build_vectors_per_s: f64,
    file_mib: f64,
    db_mib: f64,
    open_ms: f64,
    refresh_delta_ms: f64,
    rebuild_s: f64,
    memory_before_open: Option<MemorySnapshot>,
    memory_after_searches: Option<MemorySnapshot>,
    scenarios: Vec<Scenario>,
}

struct Bench {
    store: Store,
    generation: i64,
    item: i64,
    next_ordinal: i64,
    /// Row `r` of the canonical data belongs to `chunk_ids[r]`.
    chunk_ids: Vec<i64>,
    data: Vec<f32>,
    dim: usize,
}

impl Bench {
    fn add(&mut self, vectors: &[f32]) -> Result<f64, String> {
        let dim = self.dim;
        let n = vectors.len() / dim;
        let started = Instant::now();
        for batch in vectors.chunks(1000 * dim) {
            let rows = batch.len() / dim;
            let chunks: Vec<NewChunk<'_>> = (0..rows)
                .map(|i| NewChunk {
                    item_id: self.item,
                    ordinal: self.next_ordinal + i64::try_from(i).unwrap_or(0),
                    chunk_kind: "text",
                    text: "x",
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                })
                .collect();
            self.next_ordinal += i64::try_from(rows).unwrap_or(0);
            let ids = self
                .store
                .insert_chunks(&chunks)
                .map_err(|e| e.to_string())?;
            let writes: Vec<VectorWrite<'_>> = ids
                .iter()
                .zip(batch.chunks_exact(dim))
                .map(|(&chunk_id, v)| VectorWrite {
                    chunk_id,
                    result: Ok(v),
                })
                .collect();
            self.store
                .write_vectors(self.generation, &writes, 0)
                .map_err(|e| e.to_string())?;
            self.chunk_ids.extend(ids);
        }
        self.data.extend_from_slice(vectors);
        #[allow(clippy::cast_precision_loss)]
        Ok(n as f64 / started.elapsed().as_secs_f64())
    }

    /// Re-embeds the chunks at `rows` with `vectors`.
    fn rewrite(&mut self, rows: &[usize], vectors: &[f32]) -> Result<(), String> {
        let dim = self.dim;
        let writes: Vec<VectorWrite<'_>> = rows
            .iter()
            .zip(vectors.chunks_exact(dim))
            .map(|(&r, v)| VectorWrite {
                chunk_id: self.chunk_ids[r],
                result: Ok(v),
            })
            .collect();
        self.store
            .write_vectors(self.generation, &writes, 1)
            .map_err(|e| e.to_string())?;
        for (&r, v) in rows.iter().zip(vectors.chunks_exact(dim)) {
            self.data[r * dim..(r + 1) * dim].copy_from_slice(v);
        }
        Ok(())
    }

    fn scenario(
        &self,
        name: &'static str,
        index: &SemanticIndex,
        queries: &[f32],
        k: usize,
    ) -> Result<Scenario, String> {
        let truth = ground_truth(&self.data, queries, self.dim, k);
        let mut samples = Vec::new();
        let mut found = 0_usize;
        for (q, expected) in queries.chunks_exact(self.dim).zip(&truth) {
            let started = Instant::now();
            let hits = index.search(&self.store, q, k).map_err(|e| e.to_string())?;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            let expected: Vec<i64> = expected
                .iter()
                .map(|&row| self.chunk_ids[usize::try_from(row).unwrap_or(0)])
                .collect();
            found += hits
                .iter()
                .filter(|h| expected.contains(&h.chunk_id))
                .count();
        }
        let status = index.status();
        #[allow(clippy::cast_precision_loss)]
        let recall = found as f64 / (truth.len() * k) as f64;
        Ok(Scenario {
            name,
            file_vectors: status.file_vectors,
            delta_vectors: status.delta_vectors,
            search: Summary::of(&samples).ok_or("no queries")?,
            recall_at_k: recall,
        })
    }
}

fn mib(bytes: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    b / (1024.0 * 1024.0)
}

pub(crate) fn run(opts: &GenerationOptions) -> Result<GenerationReport, String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let _ = std::fs::remove_dir_all(&opts.work_dir);
    std::fs::create_dir_all(&opts.work_dir).map_err(|e| err(&e))?;
    let db = opts.work_dir.join("bench.db");
    let dir = opts.work_dir.join("vectors");
    let store = Store::open_writer(&db).map_err(|e| err(&e))?;
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: "bench",
                chunker_version: 1,
                dim: opts.dim,
            },
            0,
        )
        .map_err(|e| err(&e))?;
    let item = store
        .insert_item(&NewItem::file("/bench/corpus.md", "corpus.md"))
        .map_err(|e| err(&e))?;
    let synth = Generator::new(Dataset::EmbeddingLike, opts.dim, 64, 7);
    let mut bench = Bench {
        store,
        generation,
        item,
        next_ordinal: 0,
        chunk_ids: Vec::with_capacity(opts.vectors + opts.delta),
        data: Vec::with_capacity((opts.vectors + opts.delta) * opts.dim),
        dim: opts.dim,
    };
    let write_vectors_per_s = bench.add(&synth.sample(opts.vectors, 11))?;
    let queries = synth.sample(opts.queries, 99);
    let token = CancellationToken::new();

    let started = Instant::now();
    let record = build_file(&bench.store, &dir, generation, &token, 0).map_err(|e| err(&e))?;
    let build_s = started.elapsed().as_secs_f64();
    bench.store.set_ann_file(&record).map_err(|e| err(&e))?;
    let file_mib = mib(std::fs::metadata(dir.join(&record.file_name))
        .map(|m| m.len())
        .unwrap_or(0));
    bench.store.checkpoint().map_err(|e| err(&e))?;
    let db_mib = mib(std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0));

    let info = bench
        .store
        .generations()
        .map_err(|e| err(&e))?
        .into_iter()
        .find(|g| g.id == generation)
        .ok_or("generation vanished")?;
    let memory_before_open = machine::memory();
    let started = Instant::now();
    let mut index = SemanticIndex::open(&bench.store, &dir, info, IndexSettings::default())
        .map_err(|e| err(&e))?;
    let open_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut scenarios = vec![bench.scenario("file", &index, &queries, opts.k)?];

    bench.add(&synth.sample(opts.delta, 12))?;
    let started = Instant::now();
    index.refresh(&bench.store).map_err(|e| err(&e))?;
    let refresh_delta_ms = started.elapsed().as_secs_f64() * 1000.0;
    scenarios.push(bench.scenario("file+delta", &index, &queries, opts.k)?);

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rewritten = (opts.vectors as f64 * opts.rewrite_fraction) as usize;
    let step = (opts.vectors / rewritten.max(1)).max(1);
    let rows: Vec<usize> = (0..rewritten).map(|i| i * step).collect();
    bench.rewrite(&rows, &synth.sample(rows.len(), 13))?;
    index.refresh(&bench.store).map_err(|e| err(&e))?;
    scenarios.push(bench.scenario("file+delta+rewritten", &index, &queries, opts.k)?);

    let started = Instant::now();
    let record = build_file(&bench.store, &dir, generation, &token, 1).map_err(|e| err(&e))?;
    bench.store.set_ann_file(&record).map_err(|e| err(&e))?;
    index.reopen_file(&bench.store).map_err(|e| err(&e))?;
    let rebuild_s = started.elapsed().as_secs_f64();
    scenarios.push(bench.scenario("rebuilt", &index, &queries, opts.k)?);
    let memory_after_searches = machine::memory();
    drop(index);
    let _ = std::fs::remove_dir_all(&opts.work_dir);

    #[allow(clippy::cast_precision_loss)]
    let build_vectors_per_s = opts.vectors as f64 / build_s;
    Ok(GenerationReport {
        schema_version: 1,
        kind: "ann-gen",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        vectors: opts.vectors,
        delta: opts.delta,
        rewritten,
        dim: opts.dim,
        k: opts.k,
        write_vectors_per_s,
        build_s,
        build_vectors_per_s,
        file_mib,
        db_mib,
        open_ms,
        refresh_delta_ms,
        rebuild_s,
        memory_before_open,
        memory_after_searches,
        scenarios,
    })
}

pub(crate) fn summarize(r: &GenerationReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "ann generation: {} vectors × {}d (+{} delta, {} rewritten), k={}",
        r.vectors, r.dim, r.delta, r.rewritten, r.k
    );
    let _ = writeln!(
        s,
        "  write {:.0} vectors/s · build {:.1} s ({:.0}/s) · file {:.1} MiB · db {:.1} MiB",
        r.write_vectors_per_s, r.build_s, r.build_vectors_per_s, r.file_mib, r.db_mib
    );
    let _ = writeln!(
        s,
        "  open {:.1} ms · delta refresh {:.1} ms · rebuild {:.1} s",
        r.open_ms, r.refresh_delta_ms, r.rebuild_s
    );
    for sc in &r.scenarios {
        let _ = writeln!(
            s,
            "  {:<22} file {:>7} delta {:>6} · p50 {:.2} · p95 {:.2} ms · recall@{} {:.3}",
            sc.name,
            sc.file_vectors,
            sc.delta_vectors,
            sc.search.p50_ms,
            sc.search.p95_ms,
            r.k,
            sc.recall_at_k
        );
    }
    if let (Some(a), Some(b)) = (r.memory_before_open, r.memory_after_searches) {
        let _ = writeln!(
            s,
            "  resident {:.0} → {:.0} MiB",
            a.resident_mib, b.resident_mib
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_run_is_consistent() {
        let opts = GenerationOptions {
            vectors: 600,
            delta: 100,
            rewrite_fraction: 0.1,
            dim: 32,
            queries: 20,
            k: 5,
            work_dir: std::env::temp_dir()
                .join(format!("lumen-bench-ann-gen-{}", std::process::id())),
            label: None,
        };
        let r = run(&opts).unwrap();
        assert_eq!(r.scenarios.len(), 4);
        assert_eq!(r.scenarios[1].delta_vectors, 100);
        assert_eq!(r.rewritten, 60);
        assert_eq!(r.scenarios[3].file_vectors, 700);
        for sc in &r.scenarios {
            assert!(sc.recall_at_k > 0.9, "{}: {}", sc.name, sc.recall_at_k);
        }
    }
}
