//! The background embedding queue (T202, ADR-029).
//!
//! - **Persistent:** pending work is "chunks without a result in the target generation"
//!   (`lumen_storage::content`), so a restart resumes where it stopped; memory holds one
//!   batch at a time — that is the backpressure bound.
//! - **Controllable:** [`Control`] pauses/resumes, caps the CPU share with a duty cycle, and
//!   lets interactive work preempt indexing ([`Control::hold`]: the query lane holds the
//!   queue while it embeds; the queue waits at the next batch boundary,
//!   docs/SEARCH_AND_INDEXING.md §17).
//! - **Time-sliced:** [`run_queue`] returns after `max_run` so the single indexing thread can
//!   interleave catalog syncs and content passes (one SQLite writer, ADR-025).
//! - **Failure handling:** a device failure aborts the run (the device policy, ADR-019,
//!   decides what next); a batch that fails otherwise is retried item by item, and items
//!   that still fail are recorded as failed for this generation instead of blocking the
//!   queue.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use lumen_core::CancellationToken;
use lumen_embedding::policy::is_device_failure;
use lumen_embedding::{Embedder, EmbeddingError, EmbeddingTask, TextInput};
use lumen_storage::{PendingChunk, StorageError, Store, VectorWrite};

/// Shared control surface (clone freely: shell, tray, query lane, worker).
#[derive(Debug, Clone, Default)]
pub struct Control {
    inner: Arc<(Mutex<State>, Condvar)>,
}

#[derive(Debug)]
struct State {
    paused: bool,
    holds: usize,
    /// Fraction of wall time the queue may spend embedding, `(0, 1]`.
    duty: f64,
    /// Bumped on every change, so sleepers wake and re-check.
    epoch: u64,
    /// Last sign of interactive use (a hold taken or released, the overlay shown).
    last_interactive: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            last_interactive: None,
            paused: false,
            holds: 0,
            duty: 1.0,
            epoch: 0,
        }
    }
}

/// While alive, the queue does not start a new batch. Dropping it releases the hold.
#[derive(Debug)]
pub struct Hold {
    control: Control,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.control.update(|s| {
            s.holds = s.holds.saturating_sub(1);
            s.last_interactive = Some(Instant::now());
        });
    }
}

impl Control {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, f: impl FnOnce(&mut State)) {
        let mut s = self.lock();
        f(&mut s);
        s.epoch += 1;
        drop(s);
        self.inner.1.notify_all();
    }

    pub fn pause(&self) {
        self.update(|s| s.paused = true);
    }

    pub fn resume(&self) {
        self.update(|s| s.paused = false);
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.lock().paused
    }

    /// Caps the share of wall time spent embedding (1.0 = no cap). With an embedding
    /// session of `t` intra-op threads on `n` logical CPUs, the machine share is about
    /// `share * t / n`. Clamped to `[0.05, 1]`.
    pub fn set_duty(&self, share: f64) {
        let share = if share.is_finite() { share } else { 1.0 };
        self.update(|s| s.duty = share.clamp(0.05, 1.0));
    }

    #[must_use]
    pub fn duty(&self) -> f64 {
        self.lock().duty
    }

    /// Preempts indexing until the returned guard is dropped (interactive embedding).
    #[must_use]
    pub fn hold(&self) -> Hold {
        self.update(|s| {
            s.holds += 1;
            s.last_interactive = Some(Instant::now());
        });
        Hold {
            control: self.clone(),
        }
    }

    /// The user may be about to search (overlay shown): indexing switches to single-chunk
    /// batches for a while, so a query never waits behind a long batch.
    pub fn mark_interactive(&self) {
        self.lock().last_interactive = Some(Instant::now());
    }

    /// Held now, or interactive use within `window`.
    #[must_use]
    pub fn interactive_within(&self, window: Duration) -> bool {
        let s = self.lock();
        s.holds > 0 || s.last_interactive.is_some_and(|t| t.elapsed() < window)
    }

    /// Wakes every waiter (e.g. after cancelling their token).
    pub fn notify(&self) {
        self.update(|_| {});
    }

    /// Blocks while paused (or held) until resumed, cancelled or `timeout` elapsed; returns
    /// whether the queue may run.
    pub fn wait_until_runnable(&self, cancel: &CancellationToken, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = self.lock();
        loop {
            if cancel.is_cancelled() {
                return false;
            }
            if !s.paused && s.holds == 0 {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            // Short slices: cancellation tokens do not notify the condvar.
            let wait = (deadline - now).min(Duration::from_millis(100));
            s = self
                .inner
                .1
                .wait_timeout(s, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Sleeps `d` unless the control changes or `cancel` fires first.
    fn sleep(&self, d: Duration, cancel: &CancellationToken) {
        let deadline = Instant::now() + d;
        let mut s = self.lock();
        let epoch = s.epoch;
        while !cancel.is_cancelled() && s.epoch == epoch {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            let wait = (deadline - now).min(Duration::from_millis(100));
            s = self
                .inner
                .1
                .wait_timeout(s, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// How long after interactive use indexing keeps single-chunk batches.
const INTERACTIVE_WINDOW: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub struct QueueConfig {
    /// Chunks per embedding call and per write transaction.
    pub batch: usize,
    /// Return after this long so the owner can run other indexing work.
    pub max_run: Duration,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            // CPU throughput is flat in batch size (T014); 8 keeps preemption latency at
            // one short batch and write transactions small.
            batch: 8,
            max_run: Duration::from_secs(30),
        }
    }
}

/// Why [`run_queue`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// No pending chunk left in the generation.
    Drained,
    Paused,
    Cancelled,
    /// `max_run` elapsed with work left.
    TimeSlice,
}

/// Counts and timings only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueueReport {
    /// Modality about to run / last completed batch. Notifications precede native inference.
    pub modality: Option<lumen_embedding::Modality>,
    pub working: bool,
    pub images_embedded: u64,
    pub embedded: u64,
    pub failed: u64,
    /// Source changed during inference; metadata must run again before retrying.
    pub stale_images: u64,
    pub batches: u64,
    /// Time inside the embedder.
    pub busy: Duration,
    /// Time spent waiting for interactive holds and duty-cycle sleeps.
    pub yielded: Duration,
    pub elapsed: Duration,
    pub stop: Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueueError {
    Storage(String),
    /// The device or runtime failed: hand it to the device policy (ADR-019).
    Device(EmbeddingError),
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "storage: {e}"),
            Self::Device(e) => write!(f, "embedding device: {e}"),
        }
    }
}

impl std::error::Error for QueueError {}

impl From<StorageError> for QueueError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}

/// What one [`run_queue`] call works on.
#[derive(Debug, Clone, Copy)]
pub struct QueueJob<'a> {
    pub embedder: &'a Embedder,
    /// Created by the caller with `Store::ensure_generation` for `embedder.space()`.
    pub generation: i64,
    pub control: &'a Control,
    pub cancel: &'a CancellationToken,
    pub cfg: QueueConfig,
}

/// Embeds pending chunks of `job.generation` until drained, paused, cancelled or
/// `cfg.max_run` elapsed. `progress` runs before inference and after each written batch.
///
/// # Errors
/// [`QueueError::Device`] on a device/runtime failure (nothing of that batch is written);
/// storage failures.
pub fn run_queue(
    store: &mut Store,
    job: &QueueJob<'_>,
    now_ms: &dyn Fn() -> i64,
    progress: &mut dyn FnMut(&QueueReport),
) -> Result<QueueReport, QueueError> {
    let QueueJob {
        embedder,
        generation,
        control,
        cancel,
        cfg,
    } = *job;
    let started = Instant::now();
    let mut report = QueueReport {
        modality: None,
        working: false,
        images_embedded: 0,
        embedded: 0,
        failed: 0,
        stale_images: 0,
        batches: 0,
        busy: Duration::ZERO,
        yielded: Duration::ZERO,
        elapsed: Duration::ZERO,
        stop: Stop::Drained,
    };
    let mut text_cursor = 0;
    let mut image_cursor = 0;
    let mut rescanned = false;
    let mut text_batches = 0;
    let visual = embedder
        .backend()
        .capabilities()
        .model
        .modalities
        .contains(lumen_embedding::Modality::Image);
    let stop = loop {
        if cancel.is_cancelled() {
            break Stop::Cancelled;
        }
        if control.is_paused() {
            break Stop::Paused;
        }
        if started.elapsed() >= cfg.max_run {
            break Stop::TimeSlice;
        }
        // Interactive holds: wait at the batch boundary (bounded, then re-check the rest).
        let waited = Instant::now();
        if !control.wait_until_runnable(cancel, Duration::from_millis(500)) {
            report.yielded += waited.elapsed();
            continue;
        }
        report.yielded += waited.elapsed();

        // Near interactive use, one chunk per call: a query that arrives mid-batch then
        // waits for at most one chunk (T204: p95 70 vs 184 ms with 8-chunk batches).
        let size = if control.interactive_within(INTERACTIVE_WINDOW) {
            1
        } else {
            cfg.batch.max(1)
        };
        // Text keeps first turn, but a large text backlog cannot starve images forever.
        // Independent cursors preserve work when modality IDs are interleaved.
        let mut batch = if visual && text_batches >= 8 {
            store.pending_image_chunks(generation, image_cursor, 1)?
        } else {
            Vec::new()
        };
        if batch.is_empty() {
            batch = store.pending_chunks_with_images(generation, text_cursor, size, false)?;
        }
        if batch.is_empty() && visual {
            batch = store.pending_image_chunks(generation, image_cursor, 1)?;
        }
        let Some(last) = batch.last() else {
            // A deleted top chunk id can be reused below the cursor: one rescan from the
            // start before declaring the queue empty.
            if (text_cursor > 0 || image_cursor > 0) && !rescanned {
                text_cursor = 0;
                image_cursor = 0;
                rescanned = true;
                continue;
            }
            break Stop::Drained;
        };
        if last.kind == "image" {
            image_cursor = last.chunk_id;
            text_batches = 0;
        } else {
            text_cursor = last.chunk_id;
            text_batches += 1;
        }

        report.modality = Some(if last.kind == "image" {
            lumen_embedding::Modality::Image
        } else {
            lumen_embedding::Modality::Text
        });
        report.working = true;
        progress(&report);
        let t = Instant::now();
        let results = embed_batch(embedder, &batch, cancel);
        let spent = t.elapsed();
        report.busy += spent;
        let results = match results {
            Ok(r) => r,
            Err(EmbeddingError::Cancelled) => break Stop::Cancelled,
            Err(e) => return Err(QueueError::Device(e)),
        };
        for (chunk, result) in batch.iter().zip(&results) {
            if *result == Err("image:changed") {
                store.invalidate_image(chunk.item_id)?;
                report.stale_images += 1;
            }
        }
        let writes: Vec<VectorWrite<'_>> = batch
            .iter()
            .zip(&results)
            .filter(|(_, r)| **r != Err("image:changed"))
            .map(|(c, r)| VectorWrite {
                chunk_id: c.chunk_id,
                result: r.as_deref().map_err(|code| *code),
            })
            .collect();
        store.write_vectors(generation, &writes, now_ms())?;
        report.batches += 1;
        for r in &results {
            if r.is_ok() {
                report.embedded += 1;
                if report.modality == Some(lumen_embedding::Modality::Image) {
                    report.images_embedded += 1;
                }
            } else if *r != Err("image:changed") {
                report.failed += 1;
            }
        }
        report.elapsed = started.elapsed();
        report.working = false;
        progress(&report);

        // Duty cycle: busy / (busy + idle) = duty.
        let duty = control.duty();
        if duty < 1.0 {
            let idle = spent.mul_f64((1.0 - duty) / duty);
            let t = Instant::now();
            control.sleep(idle, cancel);
            report.yielded += t.elapsed();
        }
    };
    report.stop = stop;
    report.working = false;
    report.elapsed = started.elapsed();
    Ok(report)
}

type ItemResult = Result<Vec<f32>, &'static str>;

/// One result per chunk. `Err` only for device failures and cancellation.
fn embed_batch(
    embedder: &Embedder,
    batch: &[PendingChunk],
    cancel: &CancellationToken,
) -> Result<Vec<ItemResult>, EmbeddingError> {
    if batch.len() == 1 && batch[0].kind == "image" {
        let chunk = &batch[0];
        let path = lumen_catalog::path::decode(&chunk.path, chunk.raw_path.as_deref());
        let image = match lumen_image::decode(&path, chunk.image_digest.as_deref(), &|| {
            cancel.is_cancelled()
        }) {
            Ok(image) => image,
            Err(lumen_image::Error::Cancelled) => return Err(EmbeddingError::Cancelled),
            Err(error) => return Ok(vec![Err(error.code())]),
        };
        let input = lumen_embedding::ImageInput {
            width: image.metadata.width,
            height: image.metadata.height,
            rgb: &image.rgb,
        };
        let vector = match embedder.embed_images(&[input], Some(cancel)) {
            Ok(vector) => vector.into_flat(),
            Err(error) if error == EmbeddingError::Cancelled || is_device_failure(&error) => {
                return Err(error);
            }
            Err(error) => return Ok(vec![Err(error_code(&error))]),
        };
        // A write during inference cannot publish an embedding for stale source pixels.
        match lumen_image::inspect(&path, &|| cancel.is_cancelled()) {
            Ok(metadata) if metadata.digest == image.metadata.digest => {
                return Ok(vec![Ok(vector)]);
            }
            Err(lumen_image::Error::Cancelled) => return Err(EmbeddingError::Cancelled),
            _ => return Ok(vec![Err("image:changed")]),
        }
    }
    let mut results: Vec<Option<ItemResult>> = batch
        .iter()
        .map(|c| c.text.trim().is_empty().then_some(Err("empty")))
        .collect();
    let todo: Vec<usize> = (0..batch.len()).filter(|&i| results[i].is_none()).collect();
    let inputs: Vec<TextInput<'_>> = todo
        .iter()
        .map(|&i| TextInput::with_title(&batch[i].text, &batch[i].title))
        .collect();
    if !inputs.is_empty() {
        match embedder.embed(EmbeddingTask::SearchDocument, &inputs, Some(cancel)) {
            Ok(vectors) => {
                for (&i, v) in todo.iter().zip(vectors.iter()) {
                    results[i] = Some(Ok(v.to_vec()));
                }
            }
            Err(e) if e == EmbeddingError::Cancelled || is_device_failure(&e) => return Err(e),
            // Something about one input: isolate it.
            Err(_) => {
                for (&i, input) in todo.iter().zip(&inputs) {
                    results[i] = Some(
                        match embedder.embed(
                            EmbeddingTask::SearchDocument,
                            std::slice::from_ref(input),
                            Some(cancel),
                        ) {
                            Ok(v) => Ok(v.into_flat()),
                            Err(e) if e == EmbeddingError::Cancelled || is_device_failure(&e) => {
                                return Err(e);
                            }
                            Err(e) => Err(error_code(&e)),
                        },
                    );
                }
            }
        }
    }
    Ok(results
        .into_iter()
        .map(|r| r.unwrap_or(Err("missing")))
        .collect())
}

fn error_code(e: &EmbeddingError) -> &'static str {
    match e {
        EmbeddingError::EmptyInput { .. } => "empty",
        EmbeddingError::Unsupported(_) => "unsupported",
        EmbeddingError::InvalidProfile(_) => "profile",
        _ => "embed",
    }
}
