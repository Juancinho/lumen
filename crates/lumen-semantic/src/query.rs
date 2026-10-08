//! The warm query embedder (T204, docs/PERFORMANCE.md §3.2–3.4).
//!
//! One worker thread owns the query-lane [`Embedder`] — its own runtime session, separate
//! from the indexing session, so a query never waits for an indexing batch inside the
//! runtime. Callers block in [`QueryEmbedder::embed`] (the settled-query lane runs on its
//! own thread):
//!
//! - **latest wins:** a request that has not started is superseded by a newer one (its
//!   caller gets [`QueryError::Superseded`]), so typing never queues work per keystroke;
//!   a request already inside the runtime finishes (a single short input cannot be
//!   interrupted) but its caller may stop waiting (cancellation);
//! - **cache:** recent query vectors are reused (in memory only, cleared on request);
//! - **warm:** [`QueryEmbedder::warm`] loads the model in the background (overlay shown);
//!   the model can be unloaded after an idle period (memory profiles, §15);
//! - **preemption:** while queries arrive — and for a short linger after the last one — the
//!   indexing queue is held at its next batch boundary (`lumen_content::Control::hold`),
//!   so typing gets the CPU (docs/SEARCH_AND_INDEXING.md §17).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use lumen_content::{Control, Hold};
use lumen_core::CancellationToken;
use lumen_embedding::{Embedder, EmbeddingError, Modality};

/// Builds the query-lane embedder on first use (loading may take a second).
pub type MakeEmbedder = Box<dyn FnMut() -> Result<Embedder, String> + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryConfig {
    /// Query vectors kept for reuse.
    pub cache: usize,
    /// Indexing stays held this long after the last query (typing bursts).
    pub linger: Duration,
    /// Unload the model after this long without queries (`None` = keep warm).
    pub idle_unload: Option<Duration>,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            cache: 64,
            linger: Duration::from_millis(1500),
            idle_unload: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryError {
    /// A newer query replaced this one before it started.
    Superseded,
    /// The caller's token was cancelled while waiting.
    Cancelled,
    /// The model could not be created or loaded (message for logs).
    Unavailable(String),
    Embedding(EmbeddingError),
    /// The service is shutting down.
    Stopped,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Superseded => f.write_str("superseded by a newer query"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Unavailable(why) => write!(f, "query embedding unavailable: {why}"),
            Self::Embedding(e) => write!(f, "query embedding failed: {e}"),
            Self::Stopped => f.write_str("query embedder stopped"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Counters and the last runtime latency (diagnostics; no query text).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct QueryStats {
    pub requests: u64,
    pub cache_hits: u64,
    pub superseded: u64,
    pub embedded: u64,
    pub last_embed: Option<Duration>,
    pub warm: bool,
}

type Vector = Arc<[f32]>;
type Outcome = Result<Vector, QueryError>;

#[derive(Default)]
struct State {
    next_ticket: u64,
    pending: Option<(u64, String)>,
    /// Tickets whose callers still wait.
    waiting: HashSet<u64>,
    done: HashMap<u64, Outcome>,
    cache: VecDeque<(String, Vector)>,
    warm_wanted: bool,
    unload_wanted: bool,
    shutdown: bool,
    failed: Option<String>,
    stats: QueryStats,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cfg: QueryConfig,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// See the module docs.
pub struct QueryEmbedder {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for QueryEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryEmbedder")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl QueryEmbedder {
    /// Starts the worker. `indexing`, when given, is held while queries run.
    ///
    /// # Errors
    /// The OS refused to spawn the thread.
    pub fn start(
        make: MakeEmbedder,
        indexing: Option<Control>,
        cfg: QueryConfig,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            cfg,
        });
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("lumen-query-embed".into())
            .spawn(move || run(&worker, make, indexing))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Embeds a search query (normalized `profile.dim` vector). Blocks until done,
    /// superseded by a newer call, or `cancel` fires.
    ///
    /// # Errors
    /// See [`QueryError`].
    pub fn embed(&self, text: &str, cancel: &CancellationToken) -> Result<Vector, QueryError> {
        let key = text.trim();
        let mut s = self.shared.lock();
        s.stats.requests += 1;
        if let Some(pos) = s.cache.iter().position(|(k, _)| k == key) {
            s.stats.cache_hits += 1;
            let hit = s.cache.remove(pos);
            if let Some((k, v)) = hit {
                s.cache.push_front((k, Arc::clone(&v)));
                return Ok(v);
            }
        }
        if let Some(why) = &s.failed {
            return Err(QueryError::Unavailable(why.clone()));
        }
        if s.shutdown {
            return Err(QueryError::Stopped);
        }
        let ticket = s.next_ticket;
        s.next_ticket += 1;
        if let Some((old, _)) = s.pending.replace((ticket, key.to_owned())) {
            s.stats.superseded += 1;
            if s.waiting.contains(&old) {
                s.done.insert(old, Err(QueryError::Superseded));
            }
        }
        s.waiting.insert(ticket);
        self.shared.wake.notify_all();
        loop {
            if let Some(outcome) = s.done.remove(&ticket) {
                s.waiting.remove(&ticket);
                return outcome;
            }
            if cancel.is_cancelled() || s.shutdown {
                s.waiting.remove(&ticket);
                if s.pending.as_ref().is_some_and(|(t, _)| *t == ticket) {
                    s.pending = None;
                }
                return Err(if s.shutdown {
                    QueryError::Stopped
                } else {
                    QueryError::Cancelled
                });
            }
            // Cancellation tokens do not notify: short waits.
            s = self
                .shared
                .wake
                .wait_timeout(s, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Loads the model in the background if it is not warm (e.g. the overlay was shown).
    pub fn warm(&self) {
        self.shared.lock().warm_wanted = true;
        self.shared.wake.notify_all();
    }

    /// Releases the model's memory now (memory pressure, Eco); it reloads on demand.
    pub fn unload(&self) {
        self.shared.lock().unload_wanted = true;
        self.shared.wake.notify_all();
    }

    /// Forgets cached query vectors (privacy: clearing history).
    pub fn clear_cache(&self) {
        self.shared.lock().cache.clear();
    }

    #[must_use]
    pub fn stats(&self) -> QueryStats {
        self.shared.lock().stats
    }
}

impl Drop for QueryEmbedder {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(shared: &Shared, mut make: MakeEmbedder, indexing: Option<Control>) {
    let cfg = shared.cfg;
    let mut embedder: Option<Embedder> = None;
    let mut hold: Option<Hold> = None;
    let mut last_done = Instant::now();
    let mut ensure = |embedder: &mut Option<Embedder>| -> Result<(), String> {
        if embedder.is_none() {
            *embedder = Some(make()?);
        }
        if let Some(e) = embedder.as_ref() {
            e.warm_text().map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    loop {
        let mut s = shared.lock();
        // Sleep until work, or until the linger / idle deadline.
        loop {
            if s.shutdown || s.pending.is_some() || s.warm_wanted || s.unload_wanted {
                break;
            }
            let idle = last_done.elapsed();
            let mut timeout = Duration::from_secs(3600);
            if hold.is_some() {
                if idle >= cfg.linger {
                    hold = None;
                } else {
                    timeout = timeout.min(cfg.linger - idle);
                }
            }
            if let (Some(limit), true) = (cfg.idle_unload, s.stats.warm) {
                if idle >= limit {
                    s.unload_wanted = true;
                    break;
                }
                timeout = timeout.min(limit - idle);
            }
            s = shared
                .wake
                .wait_timeout(s, timeout)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if s.shutdown {
            for t in s.waiting.clone() {
                s.done.insert(t, Err(QueryError::Stopped));
            }
            shared.wake.notify_all();
            return;
        }
        if std::mem::take(&mut s.unload_wanted) {
            if let Some(e) = &embedder {
                let _ = e.backend().unload(Modality::Text);
            }
            s.stats.warm = false;
            continue;
        }
        let request = s.pending.take();
        let warm_only = std::mem::take(&mut s.warm_wanted) && request.is_none();
        if s.failed.is_some() {
            continue;
        }
        drop(s);

        if request.is_some() && hold.is_none() {
            hold = indexing.as_ref().map(Control::hold);
        }
        let loaded = ensure(&mut embedder);
        let mut s = shared.lock();
        if let Err(why) = loaded {
            s.failed = Some(why.clone());
            for t in s.waiting.clone() {
                s.done.insert(t, Err(QueryError::Unavailable(why.clone())));
            }
            s.pending = None;
            shared.wake.notify_all();
            hold = None;
            continue;
        }
        s.stats.warm = true;
        if warm_only {
            continue;
        }
        let Some((ticket, text)) = request else {
            continue;
        };
        if !s.waiting.contains(&ticket) {
            // Its caller stopped waiting before it started.
            continue;
        }
        drop(s);
        let started = Instant::now();
        let result = embedder
            .as_ref()
            .ok_or(QueryError::Stopped)
            .and_then(|e| e.embed_query(&text, None).map_err(QueryError::Embedding));
        let elapsed = started.elapsed();
        last_done = Instant::now();
        let mut s = shared.lock();
        s.stats.last_embed = Some(elapsed);
        let outcome: Outcome = result.map(Vector::from);
        if let Ok(v) = &outcome {
            s.stats.embedded += 1;
            if cfg.cache > 0 {
                s.cache.retain(|(k, _)| *k != text);
                s.cache.push_front((text, Arc::clone(v)));
                s.cache.truncate(cfg.cache);
            }
        }
        if s.waiting.contains(&ticket) {
            s.done.insert(ticket, outcome);
        }
        shared.wake.notify_all();
    }
}
