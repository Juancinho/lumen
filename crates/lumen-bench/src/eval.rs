//! `lumen-bench eval` (T205, docs/SEARCH_AND_INDEXING.md §19): relevance of the root
//! search lanes — names, file contents, meaning — alone and fused, over the committed
//! synthetic set `fixtures/eval/` (documents + judged queries).
//!
//! The whole real path runs on a temporary database: catalog sync → content pass →
//! embedding queue → ANN file → the three providers; each query is asked once per lane
//! (settled), then the lanes are fused with `lumen_search::fuse` for every weight setting
//! (cheap, so `--sweep` can try a grid). Metrics: Recall@1/@5/@10, MRR@10, NDCG@10,
//! top-1 success, by category; latency per lane.
//!
//! Privacy: the fixture is synthetic; the report has aggregates and per-query ranks by
//! index, no query text or paths.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use lumen_catalog::{CatalogProvider, ContentProvider, IndexLocations, sync_files};
use lumen_content::{Control, PassConfig, QueueConfig, QueueJob, run_content_pass, run_queue};
use lumen_core::{CancellationToken, Payload, Provider, ProviderQuery, QueryId, ResultItem};
use lumen_embedding::{Embedder, EmbeddingProfile};
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_search::fuse;
use lumen_semantic::{
    IndexSettings, QueryConfig, QueryEmbedder, SemanticConfig, SemanticIndex, SemanticProvider,
    SharedIndex, build_file,
};
use lumen_storage::{GenerationSpec, Store};
use serde::{Deserialize, Serialize};

use crate::embed::{EmbedOptions, make_backend};
use crate::machine::MachineInfo;
use crate::stats::Summary;

/// Lanes in provider order (fusion weights follow it).
const LANES: [&str; 3] = ["name", "content", "semantic"];
/// Results asked per lane and kept after fusion.
const LIMIT: usize = 30;

#[derive(Debug, Clone)]
pub(crate) struct EvalOptions {
    pub(crate) embed: EmbedOptions,
    /// Folder with `corpus/` and `queries.json`.
    pub(crate) fixture: PathBuf,
    /// Fusion weights name, content, semantic.
    pub(crate) weights: [f32; 3],
    pub(crate) sweep: bool,
    /// Print the lanes' top results for queries the fused list misses at rank 1
    /// (stderr; the fixture is synthetic).
    pub(crate) explain: bool,
    pub(crate) work_dir: Option<PathBuf>,
    pub(crate) label: Option<String>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            embed: EmbedOptions::default(),
            fixture: PathBuf::from("fixtures/eval"),
            weights: [1.0, 1.0, 2.0],
            sweep: false,
            explain: false,
            work_dir: None,
            label: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Fixture {
    queries: Vec<Judged>,
}

#[derive(Debug, Deserialize)]
struct Judged {
    q: String,
    /// Grade 2: the answer.
    relevant: Vec<String>,
    /// Grade 1: acceptable, not what was asked (`fixtures/eval-hard`).
    #[serde(default)]
    related: Vec<String>,
    category: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub(crate) struct Metrics {
    queries: usize,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    top1: f64,
}

impl Metrics {
    /// `relevant` (grade 2) decide recall, MRR and top-1; NDCG uses graded gains
    /// (`2^grade - 1`: 3 for relevant, 1 for `related`).
    fn add(&mut self, ranked: &[String], relevant: &HashSet<&str>, related: &HashSet<&str>) {
        let hit = |i: usize| ranked.get(i).is_some_and(|r| relevant.contains(r.as_str()));
        let recall = |k: usize| {
            #[allow(clippy::cast_precision_loss)]
            let found = (0..k).filter(|&i| hit(i)).count() as f64;
            #[allow(clippy::cast_precision_loss)]
            let total = relevant.len().min(k).max(1) as f64;
            found / total
        };
        self.queries += 1;
        self.recall_at_1 += recall(1);
        self.recall_at_5 += recall(5);
        self.recall_at_10 += recall(10);
        if let Some(first) = (0..10).find(|&i| hit(i)) {
            #[allow(clippy::cast_precision_loss)]
            let rr = 1.0 / (first + 1) as f64;
            self.mrr_at_10 += rr;
        }
        #[allow(clippy::cast_precision_loss)]
        let discount = |i: usize| 1.0 / ((i + 2) as f64).log2();
        let grade = |r: &str| {
            if relevant.contains(r) {
                3.0
            } else if related.contains(r) {
                1.0
            } else {
                0.0
            }
        };
        let dcg: f64 = ranked
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, r)| grade(r) * discount(i))
            .sum();
        let mut ideal_gains: Vec<f64> = std::iter::repeat_n(3.0, relevant.len())
            .chain(std::iter::repeat_n(1.0, related.len()))
            .collect();
        ideal_gains.truncate(10);
        let ideal: f64 = ideal_gains
            .iter()
            .enumerate()
            .map(|(i, g)| g * discount(i))
            .sum();
        if ideal > 0.0 {
            self.ndcg_at_10 += dcg / ideal;
        }
        if hit(0) {
            self.top1 += 1.0;
        }
    }

    fn mean(mut self) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let n = self.queries.max(1) as f64;
        self.recall_at_1 /= n;
        self.recall_at_5 /= n;
        self.recall_at_10 /= n;
        self.mrr_at_10 /= n;
        self.ndcg_at_10 /= n;
        self.top1 /= n;
        self
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigResult {
    name: String,
    weights: [f32; 3],
    overall: Metrics,
    by_category: BTreeMap<String, Metrics>,
    /// Per query (fixture order): 1-based rank of the first relevant result, 0 = not in
    /// the top 10.
    first_relevant_rank: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LaneLatency {
    lane: &'static str,
    latency: Summary,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvalReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    backend: String,
    space: String,
    documents: u64,
    chunks: u64,
    vectors: u64,
    index_seconds: f64,
    queries: usize,
    latency: Vec<LaneLatency>,
    configs: Vec<ConfigResult>,
    /// With `--sweep`: the weight setting with the best mean NDCG@10.
    best: Option<String>,
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// Fixture-relative path of a result (`/` separators); folders included.
fn relative(item: &ResultItem, root: &Path) -> Option<String> {
    let Payload::Path(p) = &item.payload else {
        return None;
    };
    let rel = p.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn make_embedder(opts: &EmbedOptions) -> Result<Embedder, String> {
    let (backend, _) = make_backend(&opts.backend, opts)?;
    Embedder::new(
        backend,
        EmbeddingProfile {
            dim: opts.dim,
            ..EmbeddingProfile::DEFAULT
        },
    )
    .map_err(|e| e.to_string())
}

/// Builds the index for the fixture; returns (store path, shared index, counts, seconds).
#[allow(clippy::type_complexity)]
fn build_index(
    opts: &EvalOptions,
    root: &Path,
    work: &Path,
) -> Result<(PathBuf, SharedIndex, String, u64, u64, u64, f64), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let db = work.join("lumen.db");
    let started = Instant::now();
    let mut store = Store::open_writer(&db).map_err(|e| err(&e))?;
    let locations = IndexLocations::standard(&[root.to_owned()], now_ms());
    sync_files(&mut store, &locations.scan_options(false), None).map_err(|e| err(&e))?;
    let cancel = CancellationToken::new();
    let pass = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &cancel,
        &now_ms,
        &mut |_| {},
    )
    .map_err(|e| err(&e))?;
    let embedder = make_embedder(&opts.embed)?;
    embedder.warm_text().map_err(|e| err(&e))?;
    let space = embedder.space().key();
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: &space,
                chunker_version: EXTRACTOR_VERSION,
                dim: opts.embed.dim,
            },
            now_ms(),
        )
        .map_err(|e| err(&e))?;
    store
        .promote_first(generation, now_ms())
        .map_err(|e| err(&e))?;
    let control = Control::new();
    let job = QueueJob {
        embedder: &embedder,
        generation,
        control: &control,
        cancel: &cancel,
        cfg: QueueConfig {
            max_run: Duration::from_secs(3600),
            ..QueueConfig::default()
        },
    };
    run_queue(&mut store, &job, &now_ms, &mut |_| {}).map_err(|e| err(&e))?;
    let dir = work.join("vectors");
    let record = build_file(&store, &dir, generation, &cancel, now_ms()).map_err(|e| err(&e))?;
    store.set_ann_file(&record).map_err(|e| err(&e))?;
    let counts = store.queue_counts(generation).map_err(|e| err(&e))?;
    let info = store
        .active_generation()
        .map_err(|e| err(&e))?
        .ok_or("no active generation")?;
    let index =
        SemanticIndex::open(&store, &dir, info, IndexSettings::default()).map_err(|e| err(&e))?;
    store.checkpoint().map_err(|e| err(&e))?;
    Ok((
        db,
        Arc::new(RwLock::new(Some(index))),
        space,
        pass.indexed,
        counts.chunks,
        record.vectors,
        started.elapsed().as_secs_f64(),
    ))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(opts: &EvalOptions) -> Result<EvalReport, String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let root = std::fs::canonicalize(opts.fixture.join("corpus"))
        .map_err(|e| format!("fixture corpus: {e}"))?;
    let fixture: Fixture = serde_json::from_str(
        &std::fs::read_to_string(opts.fixture.join("queries.json"))
            .map_err(|e| format!("queries.json: {e}"))?,
    )
    .map_err(|e| format!("queries.json: {e}"))?;
    let work = opts.work_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("lumen-bench-eval-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| err(&e))?;

    let (db, shared, space, documents, chunks, vectors, index_seconds) =
        build_index(opts, &root, &work)?;
    let embed_opts = opts.embed.clone();
    let query_embedder = Arc::new(
        QueryEmbedder::start(
            Box::new(move || make_embedder(&embed_opts)),
            None,
            QueryConfig::default(),
        )
        .map_err(|e| err(&e))?,
    );
    let reader = || Store::open_reader(&db).map_err(|e| err(&e));
    let providers: [Arc<dyn Provider>; 3] = [
        Arc::new(CatalogProvider::new(reader()?)),
        Arc::new(ContentProvider::new(reader()?)),
        Arc::new(SemanticProvider::new(
            Arc::clone(&query_embedder),
            Arc::clone(&shared),
            reader()?,
            SemanticConfig::default(),
        )),
    ];
    query_embedder.warm();

    // Every query once per lane (settled).
    let cancel = CancellationToken::new();
    let mut lane_ms: [Vec<f64>; 3] = Default::default();
    let mut lists: Vec<Vec<(usize, Vec<ResultItem>)>> = Vec::new();
    for (i, judged) in fixture.queries.iter().enumerate() {
        let request = ProviderQuery {
            id: QueryId::new(i as u64 + 1).ok_or("query id")?,
            text: &judged.q,
            typing: false,
            limit: LIMIT,
        };
        let mut per_lane = Vec::new();
        for (lane, provider) in providers.iter().enumerate() {
            let t = Instant::now();
            let items = provider
                .search(&request, &cancel)
                .map_err(|e| format!("{} lane: {e}", LANES[lane]))?;
            lane_ms[lane].push(t.elapsed().as_secs_f64() * 1000.0);
            per_lane.push((lane, items));
        }
        lists.push(per_lane);
    }

    let evaluate = |name: String, weights: [f32; 3]| -> ConfigResult {
        let mut overall = Metrics::default();
        let mut by_category: BTreeMap<String, Metrics> = BTreeMap::new();
        let mut first_relevant_rank = Vec::new();
        for (judged, per_lane) in fixture.queries.iter().zip(&lists) {
            let fused = fuse(per_lane, &weights, LIMIT);
            let mut seen = HashSet::new();
            let ranked: Vec<String> = fused
                .iter()
                .filter_map(|r| relative(r, &root))
                .filter(|r| seen.insert(r.clone()))
                .collect();
            let relevant: HashSet<&str> = judged.relevant.iter().map(String::as_str).collect();
            let related: HashSet<&str> = judged.related.iter().map(String::as_str).collect();
            overall.add(&ranked, &relevant, &related);
            by_category
                .entry(judged.category.clone())
                .or_default()
                .add(&ranked, &relevant, &related);
            first_relevant_rank.push(
                ranked
                    .iter()
                    .take(10)
                    .position(|r| relevant.contains(r.as_str()))
                    .map_or(0, |p| p + 1),
            );
        }
        ConfigResult {
            name,
            weights,
            overall: overall.mean(),
            by_category: by_category
                .into_iter()
                .map(|(k, m)| (k, m.mean()))
                .collect(),
            first_relevant_rank,
        }
    };

    let mut configs = vec![
        evaluate("name only".into(), [1.0, 0.0, 0.0]),
        evaluate("content only".into(), [0.0, 1.0, 0.0]),
        evaluate("semantic only".into(), [0.0, 0.0, 1.0]),
        evaluate("lexical (name + content)".into(), [1.0, 1.0, 0.0]),
        evaluate(
            format!(
                "fused {}/{}/{}",
                opts.weights[0], opts.weights[1], opts.weights[2]
            ),
            opts.weights,
        ),
    ];
    if opts.explain {
        let top = |items: &[ResultItem], n: usize| {
            items
                .iter()
                .take(n)
                .map(|r| {
                    format!(
                        "{} ({:?} {:.2})",
                        relative(r, &root).unwrap_or_default(),
                        r.score.match_kind,
                        r.score.confidence.get()
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        for (judged, per_lane) in fixture.queries.iter().zip(&lists) {
            let fused = fuse(per_lane, &opts.weights, LIMIT);
            let first = fused.first().and_then(|r| relative(r, &root));
            if first.is_some_and(|f| judged.relevant.contains(&f)) {
                continue;
            }
            eprintln!(
                "MISS [{}] {:?} -> {:?}",
                judged.category, judged.q, judged.relevant
            );
            for (lane, items) in per_lane {
                eprintln!("  {:<8} {}", LANES[*lane], top(items, 3));
            }
            eprintln!("  fused    {}", top(&fused, 3));
        }
    }
    let mut best = None;
    if opts.sweep {
        let mut sweep = Vec::new();
        for name in [0.5_f32, 1.0, 2.0] {
            for content in [0.5_f32, 1.0, 2.0, 3.0] {
                for semantic in [0.5_f32, 1.0, 2.0, 3.0] {
                    sweep.push(evaluate(
                        format!("sweep {name}/{content}/{semantic}"),
                        [name, content, semantic],
                    ));
                }
            }
        }
        // First of the best (the grid starts at light weights: ties keep the simpler).
        best = sweep
            .iter()
            .fold(None::<&ConfigResult>, |best, c| match best {
                Some(b) if b.overall.ndcg_at_10 >= c.overall.ndcg_at_10 => Some(b),
                _ => Some(c),
            })
            .map(|c| c.name.clone());
        configs.extend(sweep);
    }

    let latency = LANES
        .iter()
        .zip(&lane_ms)
        .filter_map(|(lane, ms)| Summary::of(ms).map(|latency| LaneLatency { lane, latency }))
        .collect();
    drop(providers);
    drop(shared);
    if opts.work_dir.is_none() {
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(EvalReport {
        schema_version: 1,
        kind: "eval",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        backend: opts.embed.backend.clone(),
        space,
        documents,
        chunks,
        vectors,
        index_seconds,
        queries: fixture.queries.len(),
        latency,
        configs,
        best,
    })
}

pub(crate) fn summarize(r: &EvalReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "eval ({}): {} documents, {} chunks, {} vectors, indexed in {:.1} s; {} queries",
        r.backend, r.documents, r.chunks, r.vectors, r.index_seconds, r.queries
    );
    for l in &r.latency {
        let _ = writeln!(
            s,
            "  lane {:<8} p50 {:.2} · p95 {:.2} ms",
            l.lane, l.latency.p50_ms, l.latency.p95_ms
        );
    }
    let _ = writeln!(
        s,
        "  {:<28} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "config", "R@1", "R@10", "MRR", "NDCG", "top1"
    );
    for c in &r.configs {
        if c.name.starts_with("sweep") && r.best.as_deref() != Some(c.name.as_str()) {
            continue;
        }
        let m = c.overall;
        let _ = writeln!(
            s,
            "  {:<28} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>6.3}",
            c.name, m.recall_at_1, m.recall_at_10, m.mrr_at_10, m.ndcg_at_10, m.top1
        );
        if !c.name.starts_with("sweep") && !c.name.starts_with("fused") {
            continue;
        }
        for (cat, m) in &c.by_category {
            let _ = writeln!(
                s,
                "    {:<26} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  (n={})",
                cat, m.recall_at_1, m.recall_at_10, m.mrr_at_10, m.ndcg_at_10, m.top1, m.queries
            );
        }
    }
    if let Some(b) = &r.best {
        let _ = writeln!(s, "  best by NDCG@10: {b}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_follow_their_definitions() {
        let rel: HashSet<&str> = ["a", "b"].into_iter().collect();
        let mut m = Metrics::default();
        m.add(
            &["x".into(), "a".into(), "y".into(), "b".into()],
            &rel,
            &HashSet::new(),
        );
        let m = m.mean();
        assert!((m.recall_at_1 - 0.0).abs() < 1e-9);
        assert!((m.recall_at_5 - 1.0).abs() < 1e-9);
        assert!((m.mrr_at_10 - 0.5).abs() < 1e-9);
        let dcg = 3.0 / 3f64.log2() + 3.0 / 5f64.log2();
        let ideal = 3.0 + 3.0 / 3f64.log2();
        assert!((m.ndcg_at_10 - dcg / ideal).abs() < 1e-9);
        assert!(m.top1.abs() < 1e-9);
        // A related result in first place earns partial gain, not a top-1.
        let related: HashSet<&str> = ["x"].into_iter().collect();
        let mut g = Metrics::default();
        g.add(&["x".into(), "a".into()], &rel, &related);
        let g = g.mean();
        let dcg = 1.0 + 3.0 / 3f64.log2();
        let ideal = 3.0 + 3.0 / 3f64.log2() + 1.0 / 2.0;
        assert!((g.ndcg_at_10 - dcg / ideal).abs() < 1e-9 && g.top1.abs() < 1e-9);
    }

    #[test]
    fn mock_run_over_the_committed_fixture() {
        let opts = EvalOptions {
            fixture: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/eval"),
            ..EvalOptions::default()
        };
        let r = run(&opts).unwrap();
        assert_eq!(r.queries, 56);
        assert!(r.documents >= 36 && r.vectors >= r.documents);
        // Lexical lanes do not depend on the model: names find exact queries, contents
        // the lexical ones.
        let by = |name: &str| r.configs.iter().find(|c| c.name == name).unwrap();
        assert!(by("name only").by_category["exact"].top1 >= 0.8);
        assert!(by("content only").by_category["lexical"].recall_at_10 >= 0.8);
        assert!(
            by("lexical (name + content)").overall.mrr_at_10 > by("name only").overall.mrr_at_10
        );
    }
}
