use std::sync::Arc;
use std::time::{Duration, Instant};

use lumen_core::{
    CancellationToken, LatencyClass, Provider, ProviderError, ProviderId, ProviderQuery, QueryId,
    ResultItem,
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

/// The provider registry and merge policy for the root search.
pub struct Coordinator {
    providers: Vec<Arc<dyn Provider>>,
    limit: usize,
}

impl Coordinator {
    /// `limit`: maximum merged results per update (and per provider request).
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            providers: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Registration order breaks confidence ties (earlier wins).
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
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
            let merged = merge(&lists, self.limit);
            let changed = emitted.as_ref().is_none_or(|e| !same_ids(e, &merged));
            if last || changed {
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

/// Global ranking (initial policy): provider-normalized confidence, then provider
/// registration order, then the provider's own order; the first occurrence of a result id
/// wins. Strong intent rules and fusion (T205) build on this.
#[must_use]
pub fn merge(lists: &[(usize, Vec<ResultItem>)], limit: usize) -> Vec<ResultItem> {
    let mut all: Vec<(usize, usize, &ResultItem)> = lists
        .iter()
        .flat_map(|(p, items)| items.iter().enumerate().map(move |(r, it)| (*p, r, it)))
        .collect();
    all.sort_by(|a, b| {
        b.2.score
            .confidence
            .cmp(&a.2.score.confidence)
            .then(a.0.cmp(&b.0))
            .then(a.1.cmp(&b.1))
    });
    let mut seen = std::collections::HashSet::new();
    all.into_iter()
        .filter(|(_, _, it)| seen.insert(it.id.clone()))
        .take(limit)
        .map(|(_, _, it)| it.clone())
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
    fn merges_by_confidence_then_registration_order_and_dedupes() {
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.fast",
            LatencyClass::Fast,
            &[("item:2", 0.9), ("item:1", 0.5)],
        )));
        c.register(Arc::new(Fake::new(
            "test.instant",
            LatencyClass::Instant,
            &[("item:1", 0.5), ("item:3", 0.95)],
        )));
        let (updates, outcome) = run(&c, true);
        assert_eq!(outcome, Outcome::Completed { failed: vec![] });
        // Instant runs first and is shown before the fast provider answers.
        assert_eq!(updates.len(), 2);
        assert_eq!(ids(&updates[0]), ["item:3", "item:1"]);
        assert!(!updates[0].done);
        // Tie at 0.5: the earlier-registered provider's copy is kept (deduplicated).
        assert_eq!(ids(&updates[1]), ["item:3", "item:2", "item:1"]);
        assert_eq!(updates[1].results[2].provider.as_str(), "test.fast");
        assert!(updates[1].done);
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
