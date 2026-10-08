use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;

use lumen_core::{CancellationToken, QueryId};

use crate::coordinator::{Coordinator, Update};

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

struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
}

/// A single search thread where the newest query wins (docs/PERFORMANCE.md §3.4):
/// submitting cancels the running query and replaces any pending one, so work never
/// queues up per keystroke. Updates of a query that was superseded are not delivered.
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
        mut sink: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("lumen-search".into())
            .spawn(move || {
                while let Some((request, token)) = next(&worker) {
                    let _ = coordinator.run(
                        request.id,
                        &request.text,
                        request.typing,
                        &token,
                        &mut |u| {
                            if !token.is_cancelled() {
                                sink(u);
                            }
                        },
                    );
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
    fn drop_stops_a_running_query() {
        let (s, _rx, _) = service(Duration::from_secs(30));
        assert!(s.submit(request(1, "a")));
        std::thread::sleep(Duration::from_millis(20));
        let t = std::time::Instant::now();
        drop(s);
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
