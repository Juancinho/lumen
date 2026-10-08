use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lumen_core::{CancellationToken, QueryId, ResultId, ResultItem};

use crate::coordinator::{Coordinator, Outcome, Update};

/// One root-search request from the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: QueryId,
    pub text: String,
    pub typing: bool,
}

#[derive(Default)]
struct Slot {
    pending: Option<Request>,
    /// Highest id accepted so far (out-of-order IPC delivers older ids late).
    latest: Option<QueryId>,
    running: Option<CancellationToken>,
    shutdown: bool,
}

/// Pause after a typing run before the settled re-run (docs/SEARCH_AND_INDEXING.md §2.1:
/// 50–90 ms; the typing run itself takes a few ms).
pub const DEFAULT_SETTLE: Duration = Duration::from_millis(80);

/// Queries whose last results stay available to actions: the user acts on what they saw,
/// which can be a query or two behind what they are typing.
const RECENT_QUERIES: usize = 4;

/// The latest results of one recent query.
struct Recent {
    id: QueryId,
    text: String,
    results: Vec<ResultItem>,
}

struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
    recent: Mutex<VecDeque<Recent>>,
}

/// A single search thread where the newest query wins (docs/PERFORMANCE.md §3.4):
/// submitting cancels the running query and replaces any pending one, so work never
/// queues up per keystroke. Updates of a query that was superseded are not delivered.
///
/// **Settling (T205):** a typing query whose run completed is re-run as settled
/// (`typing == false`, same id) when nothing newer arrives within the settle delay — that
/// is when content FTS and semantic providers run (docs/SEARCH_AND_INDEXING.md §2.1).
pub struct SearchService {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl SearchService {
    /// Starts the thread. `sink` receives the updates of queries that were not superseded,
    /// on the search thread.
    ///
    /// # Errors
    /// The OS refused to spawn the thread.
    pub fn start(
        coordinator: Coordinator,
        sink: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::start_with_settle(coordinator, DEFAULT_SETTLE, sink)
    }

    /// As [`SearchService::start`] with a settle delay (`Duration::ZERO`: settle at once).
    ///
    /// # Errors
    /// The OS refused to spawn the thread.
    pub fn start_with_settle(
        coordinator: Coordinator,
        settle: Duration,
        mut sink: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            wake: Condvar::new(),
            recent: Mutex::new(VecDeque::with_capacity(RECENT_QUERIES + 1)),
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("lumen-search".into())
            .spawn(move || {
                let settles = coordinator.has_settled_providers();
                while let Some((mut request, mut token)) = next(&worker) {
                    loop {
                        let outcome = coordinator.run(
                            request.id,
                            &request.text,
                            request.typing,
                            &token,
                            &mut |u| {
                                if !token.is_cancelled() {
                                    remember(&worker, &request.text, &u);
                                    sink(u);
                                }
                            },
                        );
                        let completed = matches!(outcome, Outcome::Completed { .. });
                        if !(settles && request.typing && completed) {
                            break;
                        }
                        match wait_settled(&worker, settle) {
                            Some(t) => {
                                token = t;
                                request.typing = false;
                            }
                            None => break,
                        }
                    }
                    finish(&worker);
                }
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Asks for `request`; ignored when an equal or newer id was already submitted.
    /// Returns whether it was accepted.
    pub fn submit(&self, request: Request) -> bool {
        let mut slot = lock(&self.shared);
        if slot.latest.is_some_and(|l| !l.is_stale(request.id)) {
            return false;
        }
        slot.latest = Some(request.id);
        if let Some(running) = &slot.running {
            running.cancel();
        }
        slot.pending = Some(request);
        drop(slot);
        self.shared.wake.notify_one();
        true
    }
}

impl SearchService {
    /// A result Lumen showed recently, with the text of the query it answered: the newest
    /// copy from query `query` if that query is still recent, else from any recent query.
    #[must_use]
    pub fn lookup(&self, query: QueryId, result: &ResultId) -> Option<(String, ResultItem)> {
        let recent = self
            .shared
            .recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let find = |r: &Recent| {
            r.results
                .iter()
                .find(|it| &it.id == result)
                .map(|it| (r.text.clone(), it.clone()))
        };
        recent
            .iter()
            .rev()
            .filter(|r| r.id == query)
            .find_map(find)
            .or_else(|| recent.iter().rev().find_map(find))
    }
}

fn remember(shared: &Shared, text: &str, update: &Update) {
    let mut recent = shared.recent.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(last) = recent.back_mut()
        && last.id == update.query
    {
        last.results.clone_from(&update.results);
        return;
    }
    recent.push_back(Recent {
        id: update.query,
        text: text.to_owned(),
        results: update.results.clone(),
    });
    while recent.len() > RECENT_QUERIES {
        recent.pop_front();
    }
}

impl Drop for SearchService {
    fn drop(&mut self) {
        {
            let mut slot = lock(&self.shared);
            slot.shutdown = true;
            slot.pending = None;
            if let Some(running) = &slot.running {
                running.cancel();
            }
        }
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Slot> {
    shared.slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Blocks until a request is pending (or shutdown); marks it running.
fn next(shared: &Shared) -> Option<(Request, CancellationToken)> {
    let mut slot = lock(shared);
    loop {
        if slot.shutdown {
            return None;
        }
        if let Some(request) = slot.pending.take() {
            let token = CancellationToken::new();
            slot.running = Some(token.clone());
            return Some((request, token));
        }
        slot = shared
            .wake
            .wait(slot)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// After a completed typing run: waits up to `settle` for a newer request. `None` if one
/// arrived (or shutdown); otherwise a fresh token for the settled re-run.
fn wait_settled(shared: &Shared, settle: Duration) -> Option<CancellationToken> {
    let deadline = Instant::now() + settle;
    let mut slot = lock(shared);
    loop {
        if slot.shutdown || slot.pending.is_some() {
            return None;
        }
        let now = Instant::now();
        if now >= deadline {
            let token = CancellationToken::new();
            slot.running = Some(token.clone());
            return Some(token);
        }
        slot = shared
            .wake
            .wait_timeout(slot, deadline - now)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Only the worker sets `running`, so clearing it after a run is always ours.
fn finish(shared: &Shared) {
    lock(shared).running = None;
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use lumen_core::LatencyClass;

    use super::*;
    use crate::coordinator::tests::Fake;

    fn request(id: u64, text: &str) -> Request {
        Request {
            id: QueryId::new(id).unwrap(),
            text: text.to_owned(),
            typing: true,
        }
    }

    fn service(delay: Duration) -> (SearchService, mpsc::Receiver<Update>, Arc<Fake>) {
        let mut fake = Fake::new("test.slow", LatencyClass::Instant, &[("item:1", 0.5)]);
        fake.delay = delay;
        let fake = Arc::new(fake);
        let mut c = Coordinator::new(10);
        c.register(fake.clone());
        let (tx, rx) = mpsc::channel();
        let s = SearchService::start(c, move |u| {
            let _ = tx.send(u);
        })
        .unwrap();
        (s, rx, fake)
    }

    #[test]
    fn newest_query_wins_and_superseded_ones_deliver_nothing() {
        let (s, rx, fake) = service(Duration::from_millis(80));
        assert!(s.submit(request(1, "a")));
        std::thread::sleep(Duration::from_millis(20)); // 1 is running
        assert!(s.submit(request(2, "ab")));
        assert!(s.submit(request(3, "abc"))); // replaces 2 before it starts
        let u = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(u.query.get(), 3);
        assert!(u.done);
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        let calls = fake.calls.lock().unwrap().clone();
        // 1 ran (and was cancelled) unless the thread was slow to start; 2 never ran.
        assert_eq!(calls.last().map(String::as_str), Some("abc"));
        assert!(!calls.iter().any(|c| c == "ab"), "{calls:?}");
    }

    #[test]
    fn older_or_repeated_ids_are_ignored() {
        let (s, rx, _) = service(Duration::ZERO);
        assert!(s.submit(request(5, "x")));
        assert!(!s.submit(request(4, "late")));
        assert!(!s.submit(request(5, "x")));
        let u = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(u.query.get(), 5);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn recent_results_can_be_looked_up_for_actions() {
        let (s, rx, _) = service(Duration::ZERO);
        let id = ResultId::new("item:1").unwrap();
        for (n, text) in [(1, "a"), (2, "ab"), (3, "abc"), (4, "abcd"), (5, "abcde")] {
            assert!(s.submit(request(n, text)));
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let q = |n| QueryId::new(n).unwrap();
        assert_eq!(s.lookup(q(4), &id).map(|(t, _)| t).as_deref(), Some("abcd"));
        // Query 1 fell out of the window: the newest copy answers.
        assert_eq!(
            s.lookup(q(1), &id).map(|(t, _)| t).as_deref(),
            Some("abcde")
        );
        let other = ResultId::new("item:2").unwrap();
        assert!(s.lookup(q(5), &other).is_none());
    }

    #[test]
    fn a_quiet_query_is_re_run_as_settled() {
        let semantic = Arc::new(Fake::new(
            "test.semantic",
            LatencyClass::Semantic,
            &[("item:9", 0.8)],
        ));
        let mut c = Coordinator::new(10);
        c.register(Arc::new(Fake::new(
            "test.name",
            LatencyClass::Instant,
            &[("item:1", 0.9)],
        )));
        c.register(semantic.clone());
        let (tx, rx) = mpsc::channel();
        let s = SearchService::start_with_settle(c, Duration::from_millis(30), move |u| {
            let _ = tx.send(u);
        })
        .unwrap();
        // Typed quickly: the first query never settles.
        assert!(s.submit(request(1, "re")));
        assert!(s.submit(request(2, "rec")));
        let mut last = None;
        while let Ok(u) = rx.recv_timeout(Duration::from_millis(400)) {
            assert_eq!(u.query.get(), 2);
            last = Some(u);
        }
        let last = last.unwrap();
        assert!(last.done);
        let order: Vec<_> = last.results.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["item:1", "item:9"]);
        assert_eq!(*semantic.calls.lock().unwrap(), ["rec"]);
    }

    #[test]
    fn drop_stops_a_running_query() {
        let (s, _rx, _) = service(Duration::from_secs(30));
        assert!(s.submit(request(1, "a")));
        std::thread::sleep(Duration::from_millis(20));
        let t = Instant::now();
        drop(s);
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
