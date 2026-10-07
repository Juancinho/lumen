//! `lumen-bench ann`: USearch/HNSW build, search latency, recall@k, memory, disk, load/mmap
//! and delete behaviour at 256d across scalar types (T008, ADR-004).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use lumen_vector::{HnswParams, IndexConfig, Metric, Scalar, VectorIndex};
use serde::Serialize;

use crate::machine::{self, MachineInfo, MemorySnapshot};
use crate::stats::Summary;
use crate::synth::{Dataset, Generator, ground_truth};

#[derive(Debug, Clone)]
pub(crate) struct AnnOptions {
    pub(crate) sizes: Vec<usize>,
    pub(crate) dim: usize,
    pub(crate) dataset: Dataset,
    pub(crate) scalars: Vec<Scalar>,
    pub(crate) connectivity: usize,
    pub(crate) expansion_add: usize,
    pub(crate) efs: Vec<usize>,
    pub(crate) queries: usize,
    pub(crate) k: usize,
    pub(crate) threads: usize,
    pub(crate) work_dir: PathBuf,
    pub(crate) label: Option<String>,
    pub(crate) seed: u64,
    /// Real vectors instead of synthetic ones: raw little-endian f32 files (`docs`, `queries`),
    /// row-major `dim` floats per vector (e.g. from `scripts/embedding/embed_corpus.py`).
    pub(crate) vectors: Option<(PathBuf, PathBuf)>,
}

impl Default for AnnOptions {
    fn default() -> Self {
        Self {
            sizes: vec![100_000],
            dim: 256,
            dataset: Dataset::EmbeddingLike,
            scalars: vec![Scalar::F32, Scalar::F16, Scalar::BF16, Scalar::I8],
            connectivity: 16,
            expansion_add: 128,
            efs: vec![16, 32, 64, 128, 256],
            queries: 500,
            k: 10,
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            work_dir: std::env::temp_dir().join("lumen-bench-ann"),
            label: None,
            seed: 42,
            vectors: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct EfResult {
    pub(crate) ef: usize,
    pub(crate) recall_at_k: f64,
    pub(crate) latency: Summary,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeleteResult {
    removed: usize,
    remove_us_per_key: f64,
    removed_keys_returned: usize,
    re_added: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScalarResult {
    pub(crate) scalar: &'static str,
    pub(crate) build_s: f64,
    pub(crate) vectors_per_s: f64,
    pub(crate) index_memory_mib: f64,
    pub(crate) resident_after_build: Option<MemorySnapshot>,
    pub(crate) file_mib: f64,
    pub(crate) save_ms: f64,
    pub(crate) load_ms: f64,
    pub(crate) view_ms: f64,
    pub(crate) view_first_query_ms: f64,
    pub(crate) view_warm: Summary,
    pub(crate) per_ef: Vec<EfResult>,
    pub(crate) deletes: DeleteResult,
}

#[derive(Debug, Serialize)]
pub(crate) struct SizeResult {
    pub(crate) n: usize,
    pub(crate) generate_s: f64,
    pub(crate) ground_truth_s: f64,
    pub(crate) raw_f32_mib: f64,
    pub(crate) scalars: Vec<ScalarResult>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnnReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    usearch: String,
    hardware_acceleration: String,
    dim: usize,
    dataset: String,
    metric: &'static str,
    connectivity: usize,
    expansion_add: usize,
    queries: usize,
    k: usize,
    build_threads: usize,
    pub(crate) sizes: Vec<SizeResult>,
}

fn read_f32(path: &std::path::Path, dim: usize) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() % (4 * dim) != 0 {
        return Err(format!(
            "{}: size is not a multiple of {dim} f32",
            path.display()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[allow(clippy::cast_precision_loss)]
fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Runs the benchmark.
///
/// # Errors
/// Index failures or I/O, as a message.
pub(crate) fn run(opts: &AnnOptions) -> Result<AnnReport, String> {
    if opts.queries == 0 || opts.k == 0 || opts.sizes.is_empty() {
        return Err("--queries, --k and --sizes must be non-empty/positive".into());
    }
    std::fs::create_dir_all(&opts.work_dir)
        .map_err(|e| format!("{}: {e}", opts.work_dir.display()))?;
    let dim = opts.dim;
    let mut sizes = Vec::new();
    let mut accel = String::new();

    let file_data = match &opts.vectors {
        Some((docs, queries)) => Some((read_f32(docs, dim)?, read_f32(queries, dim)?)),
        None => None,
    };
    let sizes_to_run: Vec<usize> = match &file_data {
        Some((docs, _)) => vec![docs.len() / dim],
        None => opts.sizes.clone(),
    };
    for n in sizes_to_run {
        let started = Instant::now();
        let (data, queries) = match &file_data {
            Some((docs, queries)) => {
                let q = (queries.len() / dim).min(opts.queries);
                (docs.clone(), queries[..q * dim].to_vec())
            }
            None => {
                let generator = Generator::new(opts.dataset, dim, (n / 200).max(16), opts.seed);
                (
                    generator.sample(n, opts.seed + 1),
                    generator.sample(opts.queries, opts.seed + 2),
                )
            }
        };
        let nq = queries.len() / dim;
        let generate_s = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let truth = ground_truth(&data, &queries, dim, opts.k);
        let ground_truth_s = started.elapsed().as_secs_f64();
        let truth_sets: Vec<HashSet<u64>> =
            truth.iter().map(|t| t.iter().copied().collect()).collect();

        let mut scalars = Vec::new();
        for &scalar in &opts.scalars {
            eprintln!("  ann n={n} scalar={} ...", scalar.name());
            let config = IndexConfig {
                dim,
                metric: Metric::Cosine,
                scalar,
                params: HnswParams {
                    connectivity: opts.connectivity,
                    expansion_add: opts.expansion_add,
                    expansion_search: 64,
                },
            };
            let index = VectorIndex::new(config).map_err(|e| e.to_string())?;
            index.reserve(n, opts.threads).map_err(|e| e.to_string())?;
            if accel.is_empty() {
                accel = usearch::hardware_acceleration_available();
            }

            // Build: rows split across threads (USearch is thread-safe after reserve).
            let started = Instant::now();
            let per = n.div_ceil(opts.threads.max(1));
            let errors: Vec<String> = std::thread::scope(|s| {
                let handles: Vec<_> = data
                    .chunks(per * dim)
                    .enumerate()
                    .map(|(t, chunk)| {
                        let index = &index;
                        s.spawn(move || {
                            for (i, v) in chunk.chunks_exact(dim).enumerate() {
                                let key = (t * per + i) as u64;
                                if let Err(e) = index.add(key, v) {
                                    return Some(e.to_string());
                                }
                            }
                            None
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .filter_map(|h| h.join().ok().flatten())
                    .collect()
            });
            if let Some(e) = errors.first() {
                return Err(format!("build: {e}"));
            }
            let build_s = started.elapsed().as_secs_f64();
            #[allow(clippy::cast_precision_loss)]
            let vectors_per_s = n as f64 / build_s.max(1e-9);
            let resident_after_build = machine::memory();
            let index_memory_mib = mib(index.memory_bytes());

            // Recall/latency sweep: single-threaded sequential queries (interactive path).
            let mut per_ef = Vec::new();
            for &ef in &opts.efs {
                index.set_expansion_search(ef);
                let mut lat = Vec::with_capacity(nq);
                let mut hits = 0_usize;
                for (qi, q) in queries.chunks_exact(dim).enumerate() {
                    let t = Instant::now();
                    let res = index.search(q, opts.k).map_err(|e| e.to_string())?;
                    lat.push(ms(t));
                    hits += res
                        .iter()
                        .filter(|h| truth_sets[qi].contains(&h.key))
                        .count();
                }
                #[allow(clippy::cast_precision_loss)]
                let recall_at_k = hits as f64 / (nq * opts.k) as f64;
                per_ef.push(EfResult {
                    ef,
                    recall_at_k,
                    latency: Summary::of(&lat).ok_or("no queries")?,
                });
            }

            // Persistence: save, full load, memory-mapped view.
            let path = opts
                .work_dir
                .join(format!("ann-{n}-{}.usearch", scalar.name()));
            let t = Instant::now();
            index.save(&path).map_err(|e| e.to_string())?;
            let save_ms = ms(t);
            let file_mib = mib(usize::try_from(
                std::fs::metadata(&path).map_err(|e| e.to_string())?.len(),
            )
            .unwrap_or(usize::MAX));
            let t = Instant::now();
            let loaded = VectorIndex::load(&path, config).map_err(|e| e.to_string())?;
            let load_ms = ms(t);
            drop(loaded);
            let t = Instant::now();
            let viewed = VectorIndex::view(&path, config).map_err(|e| e.to_string())?;
            let view_ms = ms(t);
            viewed.set_expansion_search(64);
            let first = &queries[..dim];
            let t = Instant::now();
            viewed.search(first, opts.k).map_err(|e| e.to_string())?;
            let view_first_query_ms = ms(t);
            let mut lat = Vec::new();
            for q in queries.chunks_exact(dim) {
                let t = Instant::now();
                viewed.search(q, opts.k).map_err(|e| e.to_string())?;
                lat.push(ms(t));
            }
            let view_warm = Summary::of(&lat).ok_or("no queries")?;
            drop(viewed);

            // Deletes: remove 1% of keys, they must never come back; then re-add them.
            index.set_expansion_search(64);
            let removed_keys: Vec<u64> = (0..n as u64).step_by(100).collect();
            let t = Instant::now();
            for &key in &removed_keys {
                index.remove(key).map_err(|e| e.to_string())?;
            }
            #[allow(clippy::cast_precision_loss)]
            let remove_us_per_key = ms(t) * 1000.0 / removed_keys.len().max(1) as f64;
            let removed: HashSet<u64> = removed_keys.iter().copied().collect();
            let mut returned = 0;
            for &key in removed_keys.iter().take(200) {
                let row = usize::try_from(key).unwrap_or(0);
                let v = &data[row * dim..(row + 1) * dim];
                let res = index.search(v, opts.k).map_err(|e| e.to_string())?;
                returned += res.iter().filter(|h| removed.contains(&h.key)).count();
            }
            let mut re_added = 0;
            for &key in removed_keys.iter().take(200) {
                let row = usize::try_from(key).unwrap_or(0);
                index
                    .add(key, &data[row * dim..(row + 1) * dim])
                    .map_err(|e| format!("re-add: {e}"))?;
                re_added += 1;
            }
            let _ = std::fs::remove_file(&path);

            scalars.push(ScalarResult {
                scalar: scalar.name(),
                build_s,
                vectors_per_s,
                index_memory_mib,
                resident_after_build,
                file_mib,
                save_ms,
                load_ms,
                view_ms,
                view_first_query_ms,
                view_warm,
                per_ef,
                deletes: DeleteResult {
                    removed: removed_keys.len(),
                    remove_us_per_key,
                    removed_keys_returned: returned,
                    re_added,
                },
            });
        }
        sizes.push(SizeResult {
            n,
            generate_s,
            ground_truth_s,
            raw_f32_mib: mib(n * dim * 4),
            scalars,
        });
    }

    Ok(AnnReport {
        schema_version: 1,
        kind: "ann",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        usearch: usearch::version().to_owned(),
        hardware_acceleration: accel,
        dim,
        dataset: match &opts.vectors {
            Some((docs, _)) => format!("file:{}", docs.display()),
            None => opts.dataset.to_string(),
        },
        metric: "cos",
        connectivity: opts.connectivity,
        expansion_add: opts.expansion_add,
        queries: opts.queries,
        k: opts.k,
        build_threads: opts.threads,
        sizes,
    })
}

/// Human-readable summary (stderr).
pub(crate) fn summarize(r: &AnnReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "ann · usearch {} [{}] · {}d {} · M={} ef_add={} · {} queries k={} · build threads {}",
        r.usearch,
        r.hardware_acceleration,
        r.dim,
        r.dataset,
        r.connectivity,
        r.expansion_add,
        r.queries,
        r.k,
        r.build_threads
    );
    for size in &r.sizes {
        let _ = writeln!(
            s,
            "  n={} (raw f32 {:.0} MiB; ground truth {:.1}s)",
            size.n, size.raw_f32_mib, size.ground_truth_s
        );
        for sc in &size.scalars {
            let _ = writeln!(
                s,
                "    {:<4} build {:>7.0} vec/s · index {:>6.1} MiB · file {:>6.1} MiB · load {:.0} ms · view {:.1} ms (first q {:.2} ms, p50 {:.3} ms)",
                sc.scalar,
                sc.vectors_per_s,
                sc.index_memory_mib,
                sc.file_mib,
                sc.load_ms,
                sc.view_ms,
                sc.view_first_query_ms,
                sc.view_warm.p50_ms
            );
            let sweep: Vec<String> = sc
                .per_ef
                .iter()
                .map(|e| {
                    format!(
                        "ef{} R@{} {:.3} p50 {:.3}ms p95 {:.3}ms",
                        e.ef, r.k, e.recall_at_k, e.latency.p50_ms, e.latency.p95_ms
                    )
                })
                .collect();
            let _ = writeln!(s, "         {}", sweep.join(" | "));
            let _ = writeln!(
                s,
                "         deletes: {} removed ({:.1} µs/key), {} returned after delete, {} re-added",
                sc.deletes.removed,
                sc.deletes.remove_us_per_key,
                sc.deletes.removed_keys_returned,
                sc.deletes.re_added
            );
        }
    }
    if r.machine.build_profile != "release" {
        let _ = writeln!(
            s,
            "  WARNING: debug build — not acceptance evidence (use --release)"
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_run_has_high_recall_and_clean_deletes() {
        let opts = AnnOptions {
            sizes: vec![3_000],
            dim: 64,
            scalars: vec![Scalar::F32, Scalar::I8],
            efs: vec![16, 128],
            queries: 50,
            threads: 2,
            work_dir: std::env::temp_dir().join(format!("lumen-ann-test-{}", std::process::id())),
            ..AnnOptions::default()
        };
        let report = run(&opts).unwrap();
        let size = &report.sizes[0];
        for sc in &size.scalars {
            let best = sc.per_ef.last().unwrap().recall_at_k;
            assert!(best > 0.9, "{}: recall {best}", sc.scalar);
            assert_eq!(sc.deletes.removed_keys_returned, 0, "{}", sc.scalar);
            assert_eq!(sc.deletes.removed, 30);
            assert!(sc.file_mib > 0.0);
        }
        // Higher ef never hurts recall much.
        let f32 = &size.scalars[0];
        assert!(f32.per_ef[1].recall_at_k + 1e-9 >= f32.per_ef[0].recall_at_k - 0.02);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["kind"], "ann");
        assert!(summarize(&report).contains("R@10"));
        let _ = std::fs::remove_dir_all(&opts.work_dir);
    }
}
