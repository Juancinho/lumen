use std::sync::Arc;
use std::time::{Duration, Instant};

use std::collections::HashMap;

use lumen_core::{
    CancellationToken, Confidence, LatencyClass, MatchKind, Provider, ProviderError, ProviderId,
    ProviderQuery, QueryId, ResultId, ResultItem,
};

/// Merged results for one query so far.
#[derive(Debug, Clone)]
pub struct Update {
    pub query: QueryId,
    /// Best first, deduplicated, at most the coordinator's limit.
    pub results: Vec<ResultItem>,
    /// No further update will follow for this query.
    pub done: bool,
    /// Since the coordinator started this query.
    pub elapsed: Duration,
    /// Providers that failed so far (their results are simply missing).
    pub failed: Vec<ProviderId>,
}

/// How a run ended. Provider failures never fail the query; they are reported here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Completed { failed: Vec<ProviderId> },
    Cancelled,
}

/// A settled run (`typing == false`) re-shows the typing run's rows first; its
/// intermediate lists are held back for this long so content and meaning arrive as one
/// refinement instead of two reorders (DESIGN_SYSTEM §11, T206).
pub const SETTLED_BATCH: Duration = Duration::from_millis(150);

/// Reciprocal-rank-fusion constant (`w / (K + rank)`, rank from 1): the usual 60 keeps the
/// head of every list close together, so agreement between lanes decides the order.
pub const RRF_K: f32 = 60.0;

/// The provider registry and fusion policy for the root search.
pub struct Coordinator {
    providers: Vec<Arc<dyn Provider>>,
    /// Fusion weight per provider (same index).
    weights: Vec<f32>,
    limit: usize,
}

impl Coordinator {
    /// `limit`: maximum merged results per update (and per provider request).
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            providers: Vec::new(),
            weights: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Registers with fusion weight 1. Registration order breaks fusion ties (earlier wins).
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.register_weighted(provider, 1.0);
    }

    /// Registers with a fusion weight (docs/SEARCH_AND_INDEXING.md §5; tuned with the
    /// evaluation harness, ADR-032).
    pub fn register_weighted(&mut self, provider: Arc<dyn Provider>, weight: f32) {
        self.providers.push(provider);
        self.weights.push(if weight.is_finite() {
            weight.max(0.0)
        } else {
            0.0
        });
    }

    /// Whether any provider waits for a settled query (the service re-runs settled
    /// queries only then).
    #[must_use]
    pub fn has_settled_providers(&self) -> bool {
        self.providers.iter().any(|p| {
            matches!(
                p.latency_class(),
                LatencyClass::Semantic | LatencyClass::Deferred
            )
        })
    }

    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Runs one query. `typing`: the user may still be typing — semantic and deferred
    /// providers wait for a settled query (`typing == false`).
    ///
    /// Providers run in latency-class order (instant, fast, semantic, deferred); after
    /// each one that changed the merged list an update is emitted, and a final `done`
    /// update always ends a completed run. A cancelled run emits nothing more.
    pub fn run(
        &self,
        query: QueryId,
        text: &str,
        typing: bool,
        cancel: &CancellationToken,
        emit: &mut dyn FnMut(Update),
    ) -> Outcome {
        let started = Instant::now();
        let mut order: Vec<usize> = (0..self.providers.len())
            .filter(|&i| {
                !typing
                    || matches!(
                        self.providers[i].latency_class(),
                        LatencyClass::Instant | LatencyClass::Fast
                    )
            })
            .collect();
        order.sort_by_key(|&i| self.providers[i].latency_class()); // stable

        let request = ProviderQuery {
            id: query,
            text,
            typing,
            limit: self.limit,
        };
        let mut lists: Vec<(usize, Vec<ResultItem>)> = Vec::new();
        let mut failed = Vec::new();
        let mut emitted: Option<Vec<ResultItem>> = None;
        for (pos, &i) in order.iter().enumerate() {
            if cancel.is_cancelled() {
                return Outcome::Cancelled;
            }
            let provider = &self.providers[i];
            match provider.search(&request, cancel) {
                Ok(items) => lists.push((i, items)),
                Err(ProviderError::Cancelled) => return Outcome::Cancelled,
                Err(_) => failed.push(provider.id().clone()),
            }
            if cancel.is_cancelled() {
                return Outcome::Cancelled;
            }
            let last = pos + 1 == order.len();
            let merged = fuse(&lists, &self.weights, self.limit);
            let changed = emitted.as_ref().is_none_or(|e| !same_ids(e, &merged));
            let batching = !typing && started.elapsed() < SETTLED_BATCH;
            if last || (changed && !batching) {
                emit(Update {
                    query,
                    results: merged.clone(),
                    done: last,
                    elapsed: started.elapsed(),
                    failed: failed.clone(),
                });
                emitted = Some(merged);
            }
        }
        if order.is_empty() {
            emit(Update {
                query,
                results: Vec::new(),
                done: true,
                elapsed: started.elapsed(),
                failed: Vec::new(),
            });
        }
        Outcome::Completed { failed }
    }
}

fn same_ids(a: &[ResultItem], b: &[ResultItem]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id)
}

/// Global ranking (T205, ADR-032): weighted reciprocal-rank fusion over the providers'
/// own orders — `Σ w_p / (RRF_K + rank_p)` per result id — with two rules on top:
///
/// - **intent first:** results some provider matched exactly (`MatchKind::Exact`, the
///   query equals the name) or by deterministic intent rank before fused ones;
/// - **one row per entity:** the copy from the provider contributing most is shown (ties:
///   earlier registration, then better rank); a missing subtitle (snippet) is taken from
///   another copy, and the confidence is the highest one.
///
/// `lists` holds `(provider index, results best first)`; `weights[provider index]`.
#[must_use]
pub fn fuse(lists: &[(usize, Vec<ResultItem>)], weights: &[f32], limit: usize) -> Vec<ResultItem> {
    struct Entry<'a> {
        score: f32,
        best: f32,
        best_key: (usize, usize),
        item: &'a ResultItem,
        intent: bool,
        confidence: Confidence,
        subtitle: Option<&'a String>,
        passage: Option<(&'a ResultItem, f32)>,
    }
    let mut entries: Vec<Entry<'_>> = Vec::new();
    let mut by_id: HashMap<&ResultId, usize> = HashMap::new();
    for (provider, items) in lists {
        let w = weights.get(*provider).copied().unwrap_or(1.0);
        if w <= 0.0 {
            // A lane switched off contributes nothing, not even unranked rows.
            continue;
        }
        for (rank, item) in items.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let contribution = w / (RRF_K + (rank + 1) as f32);
            let intent = matches!(item.score.match_kind, MatchKind::Exact | MatchKind::Intent);
            let key = (*provider, rank);
            match by_id.get(&item.id) {
                Some(&i) => {
                    let e = &mut entries[i];
                    e.score += contribution;
                    e.intent |= intent;
                    e.confidence = e.confidence.max(item.score.confidence);
                    if e.subtitle.is_none() {
                        e.subtitle = item.subtitle.as_ref();
                    }
                    if matches!(
                        item.payload,
                        lumen_core::Payload::Code(_) | lumen_core::Payload::Pdf(_)
                    ) && e.passage.is_none_or(|(_, best)| contribution > best)
                    {
                        e.passage = Some((item, contribution));
                    }
                    if contribution > e.best || (contribution == e.best && key < e.best_key) {
                        e.best = contribution;
                        e.best_key = key;
                        e.item = item;
                    }
                }
                None => {
                    by_id.insert(&item.id, entries.len());
                    entries.push(Entry {
                        score: contribution,
                        best: contribution,
                        best_key: key,
                        item,
                        intent,
                        confidence: item.score.confidence,
                        subtitle: item.subtitle.as_ref(),
                        passage: matches!(
                            item.payload,
                            lumen_core::Payload::Code(_) | lumen_core::Payload::Pdf(_)
                        )
                        .then_some((item, contribution)),
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| {
        b.intent
            .cmp(&a.intent)
            .then(b.score.total_cmp(&a.score))
            .then(a.best_key.cmp(&b.best_key))
    });
    entries
        .into_iter()
        .take(limit)
        .map(|e| {
            let mut item = e.item.clone();
            item.score.confidence = e.confidence;
            if item.subtitle.is_none() {
                item.subtitle = e.subtitle.cloned();
            }
            // Keep contextual actions even if the name lane's copy wins a tie. The
            // passage and its payload must come from the same code/PDF hit.
            if let Some((passage, _)) = e.passage {
                item.payload = passage.payload.clone();
                item.capabilities = passage.capabilities;
                item.secondary_actions = passage.secondary_actions.clone();
                if !matches!(item.score.match_kind, MatchKind::Exact | MatchKind::Intent) {
                    item.kind = passage.kind;
                    item.subtitle = passage.subtitle.clone();
                }
            }
            item
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use lumen_core::builtin::OPEN;
    use lumen_core::{
        CapabilitySet, Confidence, IconRef, MatchKind, Payload, ResultId, ResultKind, Score,
    };

    use super::*;

    pub(crate) fn item(id: &str, provider: &ProviderId, confidence: f32) -> ResultItem {
        ResultItem {
            id: ResultId::new(id).unwrap(),
            provider: provider.clone(),
            kind: ResultKind::File,
            title: id.to_owned(),
            subtitle: None,
            detail: None,
            icon: IconRef::KindDefault,
            score: Score::new(Confidence::new(confidence).unwrap(), MatchKind::Prefix),
            capabilities: CapabilitySet::default(),
            primary_action: OPEN,
            secondary_actions: Vec::new(),
            payload: Payload::Text(id.into()),
        }
    }

    /// Returns fixed results (or an error), optionally sleeping, and records calls.
    pub(crate) struct Fake {
        pub(crate) id: ProviderId,
        pub(crate) class: LatencyClass,
        pub(crate) results: Vec<(String, f32)>,
        pub(crate) fail: bool,
        pub(crate) delay: Duration,
        pub(crate) calls: Mutex<Vec<String>>,
    }

    impl Fake {
        pub(crate) fn new(id: &str, class: LatencyClass, results: &[(&str, f32)]) -> Self {
            Self {
                id: ProviderId::new(id).unwrap(),
                class,
                results: results.iter().map(|(s, c)| ((*s).to_owned(), *c)).collect(),
                fail: false,
                delay: Duration::ZERO,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Provider for Fake {
        fn id(&self) -> &ProviderId {
            &self.id
        }

        fn latency_class(&self) -> LatencyClass {
            self.class
        }

        fn search(
            &self,
            query: &ProviderQuery<'_>,
            cancel: &CancellationToken,
        ) -> Result<Vec<ResultItem>, ProviderError> {
            self.calls.lock().unwrap().push(query.text.to_owned());
            let until = Instant::now() + self.delay;
            while Instant::now() < until {
                if cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if self.fail {
                return Err(ProviderError::Unavailable("down".into()));
            }
            Ok(self
                .results
                .iter()
                .take(query.limit)
                .map(|(id, c)| item(id, &self.id, *c))
                .collect())
        }
    }

    fn qid(n: u64) -> QueryId {
        QueryId::new(n).unwrap()
    }

    fn ids(update: &Update) -> Vec<&str> {
        update.results.iter().map(|r| r.id.as_str()).collect()
    }

    fn run(c: &Coordinator, typing: bool) -> (Vec<Update>, Outcome) {
        let mut updates = Vec::new();
        let outcome = c.run(qid(1), "q", typing, &CancellationToken::new(), &mut |u| {
            updates.push(u);
        });
        (updates, outcome)
    }

    #[test]
    fn fuses_by_reciprocal_rank_and_dedupes() {
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.fast",
            LatencyClass::Fast,
            &[("item:2", 0.9), ("item:1", 0.5)],
        )));
        c.register(Arc::new(Fake::new(
            "test.instant",
            LatencyClass::Instant,
            &[("item:1", 0.4), ("item:3", 0.95)],
        )));
        let (updates, outcome) = run(&c, true);
        assert_eq!(outcome, Outcome::Completed { failed: vec![] });
        // Instant runs first and is shown before the fast provider answers, in its order.
        assert_eq!(updates.len(), 2);
        assert_eq!(ids(&updates[0]), ["item:1", "item:3"]);
        assert!(!updates[0].done);
        // item:1 is in both lists (1/62 + 1/61) and wins; item:2 (fast #1, registered
        // first) beats item:3 (instant #2).
        assert_eq!(ids(&updates[1]), ["item:1", "item:2", "item:3"]);
        // The copy shown is the one contributing most (instant rank 1), with the highest
        // confidence of all copies.
        assert_eq!(updates[1].results[0].provider.as_str(), "test.instant");
        assert!((updates[1].results[0].score.confidence.get() - 0.5).abs() < 1e-6);
        assert!(updates[1].done);
    }

    #[test]
    fn weights_exact_matches_and_snippets() {
        let p = |s| ProviderId::new(s).unwrap();
        let (name, content) = (p("test.name"), p("test.content"));
        let mut exact = item("item:7", &name, 1.0);
        exact.score.match_kind = MatchKind::Exact;
        let mut snippet = item("item:8", &content, 0.6);
        snippet.subtitle = Some("…matching words…".into());
        let lists = vec![
            (0, vec![item("item:8", &name, 0.7), exact]),
            (
                1,
                vec![
                    snippet,
                    item("item:9", &content, 0.5),
                    item("item:6", &content, 0.4),
                ],
            ),
        ];
        let fused = fuse(&lists, &[1.0, 3.0], 10);
        let order: Vec<_> = fused.iter().map(|r| r.id.as_str()).collect();
        // Exact first despite rank 2; then the heavier content lane decides.
        assert_eq!(order, ["item:7", "item:8", "item:9", "item:6"]);
        // item:8's best contribution is content (3/61 > 1/61): its copy, its snippet.
        assert_eq!(fused[1].provider.as_str(), "test.content");
        assert_eq!(fused[1].subtitle.as_deref(), Some("…matching words…"));
        let name_only: Vec<_> = fuse(&lists, &[1.0, 0.0], 10)
            .iter()
            .map(|r| r.id.as_str().to_owned())
            .collect();
        assert_eq!(
            name_only,
            ["item:7", "item:8"],
            "a zero weight drops the lane"
        );
    }

    #[test]
    fn code_context_and_actions_survive_a_name_lane_tie() {
        use lumen_core::{Capability, CodeTarget, builtin, validate_result};
        let name = ProviderId::new("test.name").unwrap();
        let content = ProviderId::new("test.content").unwrap();
        let mut file = item("item:7", &name, 0.9);
        file.payload = Payload::Path("/repo/client.py".into());
        file.capabilities = CapabilitySet::of(&[Capability::LocalPath]);
        let mut code = file.clone();
        code.provider = content;
        code.kind = ResultKind::Code;
        code.score.match_kind = MatchKind::FullText;
        code.capabilities = code.capabilities.with(Capability::CodeSymbol);
        code.secondary_actions = vec![builtin::COPY_SYMBOL];
        code.subtitle = Some("retry failed requests".into());
        code.payload = Payload::Code(Box::new(CodeTarget {
            path: "/repo/client.py".into(),
            symbol: Some("retry".into()),
            language: "python".into(),
            repository: None,
            start_offset: Some(30),
            end_offset: Some(80),
            passage: "def retry(): pass".into(),
        }));
        let lists = vec![(0, vec![file.clone()]), (1, vec![code.clone()])];
        let result = fuse(&lists, &[1.0, 1.0], 10).remove(0);
        assert_eq!(result.id, file.id);
        assert_eq!(result.kind, ResultKind::Code);
        assert_eq!(result.payload, code.payload);
        assert_eq!(result.subtitle, code.subtitle);
        assert!(result.offers(&builtin::COPY_SYMBOL));
        assert!(validate_result(&result, &builtin::DESCRIPTORS).is_empty());
        file.score.match_kind = MatchKind::Exact;
        let exact = fuse(&[(0, vec![file]), (1, vec![code])], &[1.0, 1.0], 10).remove(0);
        assert_eq!(
            exact.kind,
            ResultKind::File,
            "exact file navigation keeps its file presentation"
        );
        assert!(exact.offers(&builtin::COPY_SYMBOL));
        assert_eq!(exact.score.match_kind, MatchKind::Exact);
    }

    #[test]
    fn pdf_page_and_passage_stay_paired_when_file_identity_is_fused() {
        use lumen_core::{Capability, PdfTarget, builtin, validate_result};
        let name = ProviderId::new("test.name").unwrap();
        let mut file = item("item:7", &name, 0.9);
        file.kind = ResultKind::File;
        file.payload = Payload::Path("/docs/guide.pdf".into());
        file.capabilities = CapabilitySet::of(&[Capability::LocalPath]);
        let page = |n, text: &str| {
            let mut result = file.clone();
            result.kind = ResultKind::PdfPage;
            result.score.match_kind = MatchKind::FullText;
            result.subtitle = Some(text.into());
            result.payload = Payload::Pdf(Box::new(PdfTarget {
                path: "/docs/guide.pdf".into(),
                page_number: std::num::NonZeroU32::new(n).unwrap(),
                passage: text.into(),
            }));
            result
        };
        let lexical = page(2, "solar panels");
        let semantic = page(7, "ocean conservation");
        let results = fuse(
            &[
                (0, vec![file.clone()]),
                (1, vec![lexical]),
                (2, vec![semantic.clone()]),
            ],
            &[1.0, 0.7, 0.85],
            10,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, file.id);
        assert_eq!(results[0].subtitle, semantic.subtitle);
        assert_eq!(results[0].payload, semantic.payload);
        assert_eq!(results[0].kind, ResultKind::PdfPage);
        assert!(validate_result(&results[0], &builtin::DESCRIPTORS).is_empty());
        file.score.match_kind = MatchKind::Exact;
        let exact = fuse(
            &[(0, vec![file]), (2, vec![semantic.clone()])],
            &[1.0, 0.7, 0.85],
            10,
        )
        .remove(0);
        assert_eq!(exact.kind, ResultKind::File);
        assert_eq!(exact.score.match_kind, MatchKind::Exact);
        assert_eq!(exact.payload, semantic.payload);
    }

    #[test]
    fn limit_applies_to_the_merged_list() {
        let mut c = Coordinator::new(2);
        c.register(Arc::new(Fake::new(
            "test.a",
            LatencyClass::Instant,
            &[("item:1", 0.3), ("item:2", 0.2), ("item:3", 0.1)],
        )));
        let (updates, _) = run(&c, true);
        assert_eq!(ids(&updates[0]), ["item:1", "item:2"]);
    }

    #[test]
    fn failing_provider_is_skipped_and_reported() {
        let mut c = Coordinator::new(10);
        let mut down = Fake::new("test.down", LatencyClass::Instant, &[("item:9", 1.0)]);
        down.fail = true;
        c.register(Arc::new(down));
        c.register(Arc::new(Fake::new(
            "test.ok",
            LatencyClass::Fast,
            &[("item:1", 0.4)],
        )));
        let (updates, outcome) = run(&c, true);
        let last = updates.last().unwrap();
        assert!(last.done);
        assert_eq!(ids(last), ["item:1"]);
        assert_eq!(last.failed, [ProviderId::new("test.down").unwrap()]);
        assert_eq!(
            outcome,
            Outcome::Completed {
                failed: vec![ProviderId::new("test.down").unwrap()]
            }
        );
    }

    #[test]
    fn semantic_providers_wait_for_a_settled_query() {
        let mut c = Coordinator::new(10);
        let semantic = Arc::new(Fake::new(
            "test.semantic",
            LatencyClass::Semantic,
            &[("item:5", 0.7)],
        ));
        c.register(semantic.clone());
        let (updates, _) = run(&c, true);
        assert_eq!(updates.len(), 1);
        assert!(updates[0].done && updates[0].results.is_empty());
        assert!(semantic.calls.lock().unwrap().is_empty());
        let (updates, _) = run(&c, false);
        assert_eq!(ids(updates.last().unwrap()), ["item:5"]);
    }

    #[test]
    fn settled_runs_refine_in_one_update() {
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.name",
            LatencyClass::Instant,
            &[("item:1", 0.9)],
        )));
        c.register(Arc::new(Fake::new(
            "test.content",
            LatencyClass::Fast,
            &[("item:2", 0.5)],
        )));
        let mut semantic = Fake::new("test.semantic", LatencyClass::Semantic, &[("item:3", 0.7)]);
        semantic.delay = Duration::from_millis(20);
        c.register(Arc::new(semantic));
        let (updates, _) = run(&c, false);
        assert_eq!(updates.len(), 1, "one refinement burst");
        assert!(updates[0].done);
        assert_eq!(updates[0].results.len(), 3);
        // A slow lane past the batch window still lets earlier lanes show first.
        let mut c = Coordinator::new(10);
        let mut slow = Fake::new("test.slow", LatencyClass::Fast, &[("item:2", 0.5)]);
        slow.delay = SETTLED_BATCH + Duration::from_millis(20);
        c.register(Arc::new(slow));
        let mut semantic = Fake::new("test.semantic", LatencyClass::Semantic, &[("item:3", 0.7)]);
        semantic.delay = Duration::from_millis(50);
        c.register(Arc::new(semantic));
        let (updates, _) = run(&c, false);
        assert_eq!(updates.len(), 2);
        // (Typing runs are not batched: `fuses_by_reciprocal_rank_and_dedupes`.)
    }

    #[test]
    fn unchanged_intermediate_lists_are_not_re_emitted() {
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.a",
            LatencyClass::Instant,
            &[("item:1", 0.9)],
        )));
        c.register(Arc::new(Fake::new("test.b", LatencyClass::Fast, &[])));
        c.register(Arc::new(Fake::new("test.c", LatencyClass::Fast, &[])));
        let (updates, _) = run(&c, true);
        // First result, then only the final marker.
        assert_eq!(updates.len(), 2);
        assert!(updates[1].done);
    }

    #[test]
    fn cancelled_runs_emit_nothing_more() {
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.a",
            LatencyClass::Instant,
            &[("item:1", 0.9)],
        )));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut n = 0;
        let outcome = c.run(qid(1), "q", true, &cancel, &mut |_| n += 1);
        assert_eq!(outcome, Outcome::Cancelled);
        assert_eq!(n, 0);
    }
}
