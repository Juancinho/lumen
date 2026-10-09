//! Content indexing in the shell (T202, ADR-029). Runs on the catalog thread — the one
//! SQLite writer (ADR-025) — after every catalog pass:
//!
//! 1. the content pass (text/code → chunks; lexical content works without a model) over
//!    locations whose content is indexed (ADR-027 `content`);
//! 2. embedding-queue slices of 30 s while chunks are pending, between which the thread
//!    checks for catalog work. Each slice asks the device policy (ADR-019) for a plan from
//!    the live power/memory/idle state: paused on low battery or memory pressure, fewer
//!    threads on battery or while the user is active. The model is unloaded when the queue
//!    drains.
//! 3. ANN maintenance for the generation being filled (T203, ADR-031): the first
//!    generation is searchable at once; its HNSW file (`<app data>/vectors/`) is rebuilt
//!    when the in-memory delta or the stale share grows; a later generation (new model or
//!    chunker) replaces the active one only once it is complete and validated, and the
//!    retired one is then deleted in small batches.
//!
//! Semantic indexing runs once the model and runtime are present (`provisioning`, T210:
//! tray download, a runtime beside the exe, or `LUMEN_EMBED_MODEL_DIR` / `LUMEN_ORT_DYLIB`). Pause/resume is a tray toggle, remembered in
//! `indexing.paused`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use lumen_catalog::IndexLocations;
use lumen_content::{
    Control, PassConfig, QueueConfig, QueueError, QueueJob, Slice, Stop, run_content_slice,
    run_image_slice, run_queue,
};
use lumen_core::CancellationToken;
use lumen_embedding::policy::{
    self, IndexingPlan, PauseReason, PolicyConfig, PowerSource, Quarantine, ResourceProfile,
    SystemState,
};
use lumen_embedding::{Embedder, EmbeddingProfile, Modality};
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_semantic::{
    IndexSettings, Maintenance, SemanticIndex, SharedIndex, build_file, cleanup_files, validate,
};
use lumen_storage::{GenerationSpec, GenerationState, Store};
use tauri::{App, AppHandle, Manager, Runtime};

use crate::{settings, tray};

pub(crate) const SETTING_PAUSED: &str = "indexing.paused";
/// `q4` (default, ADR-015), `q8`, `fp32`.
pub(crate) const ENV_VARIANT: &str = "LUMEN_EMBED_VARIANT";
/// Overrides the policy's thread count (T014 experiments).
pub(crate) const ENV_THREADS: &str = "LUMEN_EMBED_THREADS";

/// Queue slice between catalog checks.
const SLICE: Duration = Duration::from_secs(30);
/// Retry delay while the policy pauses indexing (battery, memory).
const POLICY_RETRY: Duration = Duration::from_secs(60);
/// No input for this long = the user is away (Balanced may use more threads).
const USER_IDLE: Duration = Duration::from_secs(120);
/// ANN files live next to the database.
const VECTOR_DIR: &str = "vectors";
/// Vectors probed when validating a generation before it replaces the active one.
const VALIDATION_SAMPLE: usize = 64;
/// Retired vector rows deleted per transaction.
const RETIRE_BATCH: usize = 5_000;

/// Semantic-indexing state shown in the tray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Semantic {
    CheckingGpu,
    Reading(&'static str),
    /// No model configured (T210 not built): lexical content only.
    NoModel,
    /// The model or runtime failed to load / the device failed; message for logs only.
    Failed,
    Idle,
    Running {
        threads: usize,
    },
    RunningGpu,
    RunningImages {
        gpu: bool,
    },
    PausedByUser,
    Waiting(PauseReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) coverage: lumen_storage::content::FileCoverage,
    pub(crate) image_files: u64,
    pub(crate) known: bool,
    pub(crate) vector_failures: u64,
    pub(crate) images: lumen_storage::images::ImageCounts,
    pub(crate) files: u64,
    pub(crate) chunks: u64,
    pub(crate) embedded: u64,
    pub(crate) semantic: Semantic,
}

/// What the catalog thread should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Nothing pending (or nothing possible): wait for catalog work.
    Idle,
    /// Chunks are still pending: run another slice right after checking catalog work.
    More,
    /// Paused by the resource policy: try again after this long.
    RetryIn(Duration),
}

struct Loaded {
    threads: usize,
    device: lumen_embedding_ort::Device,
    images: bool,
    image_gpu: bool,
    embedder: Embedder,
    generation: Option<i64>,
}

/// Managed state.
pub(crate) struct Indexing {
    control: Control,
    status: Mutex<Status>,
    loaded: Mutex<Option<Loaded>>,
    /// The model failed: do not retry until restart (or a settings change, later).
    failed: Mutex<bool>,
    /// The searchable index of the generation being filled (T203); the settled-query
    /// lane (T205) reads it.
    ann: SharedIndex,
}

impl Indexing {
    pub(crate) fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_semantic(&self, s: Semantic) {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .semantic = s;
    }

    pub(crate) fn paused(&self) -> bool {
        self.control.is_paused()
    }

    /// The indexing control: the query lane holds it while the user searches (ADR-030).
    pub(crate) fn control(&self) -> &Control {
        &self.control
    }

    /// The ANN index the semantic provider searches (ADR-031).
    pub(crate) fn shared_index(&self) -> SharedIndex {
        Arc::clone(&self.ann)
    }

    /// Whether semantic search has an active generation to search.
    pub(crate) fn semantic_searchable(&self) -> bool {
        self.ann
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|ix| ix.generation().state == GenerationState::Active)
    }
}

/// Model and runtime are present (installed from the tray, beside the exe, or by env).
pub(crate) fn model_configured() -> bool {
    crate::provisioning::ready()
}

/// The download finished: semantic indexing can start now.
pub(crate) fn on_model_installed<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Indexing>();
    *state.loaded.lock().unwrap_or_else(PoisonError::into_inner) = None;
    *state.failed.lock().unwrap_or_else(PoisonError::into_inner) = false;
    state.set_semantic(if state.paused() {
        Semantic::PausedByUser
    } else {
        Semantic::Idle
    });
    crate::catalog::request_work(app);
    tray::refresh_indexing(app);
    if app
        .try_state::<crate::gpu::Gpu>()
        .is_some_and(|gpu| gpu.enabled())
    {
        crate::gpu::discover(app);
    }
}

/// The model is about to be deleted: drop the indexing session (vectors stay).
pub(crate) fn on_model_removed<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Indexing>();
    *state.loaded.lock().unwrap_or_else(PoisonError::into_inner) = None;
    state.set_semantic(Semantic::NoModel);
    tray::refresh_indexing(app);
}

pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let control = Control::new();
    let paused = settings::get_raw(&app.state::<settings::Settings>(), SETTING_PAUSED)
        .is_some_and(|v| v.trim() == "true");
    if paused {
        control.pause();
    }
    let semantic = if !model_configured() {
        Semantic::NoModel
    } else if paused {
        Semantic::PausedByUser
    } else {
        Semantic::Idle
    };
    app.manage(Indexing {
        control,
        status: Mutex::new(Status {
            coverage: Default::default(),
            image_files: 0,
            known: false,
            vector_failures: 0,
            images: Default::default(),
            files: 0,
            chunks: 0,
            embedded: 0,
            semantic,
        }),
        loaded: Mutex::new(None),
        failed: Mutex::new(false),
        ann: Arc::new(RwLock::new(None)),
    });
}

/// The user may be about to search: indexing switches to one-chunk batches for a while so
/// a query never waits behind a long batch (ADR-030).
pub(crate) fn on_overlay_shown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<Indexing>() {
        state.control.mark_interactive();
        if state.semantic_searchable() {
            crate::search::warm_semantic(app);
        }
    }
}

/// Tray toggle: pauses or resumes indexing (remembered across restarts).
pub(crate) fn set_paused<R: Runtime>(app: &AppHandle<R>, paused: bool) {
    let state = app.state::<Indexing>();
    if paused {
        state.control.pause();
    } else {
        state.control.resume();
    }
    settings::set_raw(
        &app.state::<settings::Settings>(),
        SETTING_PAUSED,
        if paused { "true" } else { "false" },
    );
    if model_configured() && !*state.failed.lock().unwrap_or_else(PoisonError::into_inner) {
        state.set_semantic(if paused {
            Semantic::PausedByUser
        } else {
            Semantic::Idle
        });
    }
    crate::catalog::request_work(app);
    tray::refresh_indexing(app);
}

/// Cursors belong to this catalog round; persistent states survive restart/settings edits.
#[derive(Default)]
pub(crate) struct ContentProgress {
    text: i64,
    image: i64,
    text_done: bool,
    image_done: bool,
}

/// One bounded extraction round, followed by a turn for the persistent vector queue.
pub(crate) fn content_pass<R: Runtime>(
    app: &AppHandle<R>,
    db: &Path,
    model: &IndexLocations,
    token: &CancellationToken,
    progress: &mut ContentProgress,
) -> bool {
    let state = app.state::<Indexing>();
    if state.paused() {
        return false;
    }
    let started = Instant::now();
    let mut store = match Store::open_writer(db) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("lumen: content pass skipped: {err}");
            return false;
        }
    };
    let now = now_ms;
    refresh_content_counts(&state, &store, model);
    if !progress.image_done && !token.is_cancelled() {
        state.set_semantic(Semantic::Reading("image metadata"));
        tray::refresh_indexing(app);
        match run_image_slice(
            &mut store,
            &|path| model.indexes_content(path),
            token,
            &now,
            progress.image,
            Slice {
                max_files: 8,
                max_run: Duration::from_millis(250),
            },
        ) {
            Ok(r) => {
                progress.image = r.cursor;
                progress.image_done = r.exhausted;
                crate::catalog::notify(app, r.files > 0);
            }
            Err(err) => {
                eprintln!("lumen: image metadata pass failed: {err}");
                progress.image_done = true;
            }
        }
        refresh_content_counts(&state, &store, model);
    }
    if !progress.text_done && !token.is_cancelled() && !state.paused() {
        state.set_semantic(Semantic::Reading("text and PDFs"));
        tray::refresh_indexing(app);
        match run_content_slice(
            &mut store,
            &PassConfig {
                batch_files: 8,
                ..PassConfig::default()
            },
            &EstimateTokens,
            &|path| model.indexes_content(path),
            token,
            &now,
            &mut |_| {},
            progress.text,
            Slice {
                max_files: 8,
                max_run: Duration::from_millis(750),
            },
        ) {
            Ok(r) => {
                progress.text = r.cursor;
                progress.text_done = r.exhausted;
                crate::diag::record("content_pass_ms", started.elapsed().as_secs_f64() * 1000.0);
                crate::diag::record("content_files", count(r.files));
                crate::catalog::notify(app, r.files > 0);
            }
            Err(err) => {
                eprintln!("lumen: content pass failed: {err}");
                progress.text_done = true;
            }
        }
    }
    refresh_content_counts(&state, &store, model);
    state.set_semantic(if state.paused() {
        Semantic::PausedByUser
    } else {
        Semantic::Idle
    });
    tray::refresh_indexing(app);
    !progress.text_done || !progress.image_done
}

fn refresh_content_counts(state: &Indexing, store: &Store, model: &IndexLocations) {
    let generation = store.active_generation().ok().flatten().map(|g| g.id);
    let content = store.content_counts().ok();
    let images = store.image_counts(generation).ok();
    let queue = generation.and_then(|g| store.queue_counts(g).ok());
    let extensions: Vec<_> = lumen_extract::TEXT_EXTENSIONS
        .iter()
        .copied()
        .chain(["pdf"])
        .chain(lumen_content::IMAGE_EXTENSIONS.iter().copied())
        .collect();
    let coverage = store
        .file_coverage(&extensions, &|path| model.indexes_content(path))
        .ok();
    let image_files = store
        .file_coverage(lumen_content::IMAGE_EXTENSIONS, &|path| {
            model.indexes_content(path)
        })
        .ok();
    let mut s = state.status.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(c) = coverage {
        s.coverage = c;
        s.known = true;
    }
    if let Some(c) = image_files {
        s.image_files = c.total;
    }
    if let Some(c) = content {
        s.files = c.indexed;
    }
    if let Some(c) = images {
        s.images = c;
    }
    if let Some(c) = queue {
        s.chunks = c.chunks;
        s.embedded = c.embedded;
        s.vector_failures = c.failed;
    }
}

#[allow(clippy::cast_precision_loss)]
fn count(n: u64) -> f64 {
    n as f64
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The policy's view of this machine right now.
pub(crate) fn system_state(
    power: Option<lumen_windows::system::PowerStatus>,
    available_memory_mib: Option<u64>,
    idle: Option<Duration>,
    logical_cpus: usize,
) -> SystemState {
    let power = match power {
        Some(p) if p.on_battery == Some(true) => PowerSource::Battery {
            percent: p.battery_percent,
        },
        Some(p) if p.on_battery == Some(false) => PowerSource::Ac,
        _ => PowerSource::Unknown,
    };
    SystemState {
        power,
        profile: ResourceProfile::Balanced,
        logical_cpus,
        available_memory_mib,
        // Unknown idle time counts as active (fewer threads).
        user_active: idle.is_none_or(|d| d < USER_IDLE),
    }
}

pub(crate) fn current_system_state() -> SystemState {
    system_state(
        lumen_windows::system::power_status(),
        lumen_windows::system::available_memory_mib(),
        lumen_windows::system::input_idle(),
        std::thread::available_parallelism().map_or(1, usize::from),
    )
}

fn current_plan<R: Runtime>(app: &AppHandle<R>, space_key: &str) -> IndexingPlan {
    let state = current_system_state();
    crate::gpu::retry_if_ready(app, &state);
    let baseline = policy::plan(
        space_key,
        &[],
        &Quarantine::new(),
        &state,
        &PolicyConfig::default(),
    );
    let plan = app
        .try_state::<crate::gpu::Gpu>()
        .and_then(|gpu| gpu.plan(space_key, &state))
        .unwrap_or(baseline.indexing);
    match (plan, env_threads()) {
        (IndexingPlan::Run { device, .. }, Some(threads)) if device == "cpu" => {
            IndexingPlan::Run { device, threads }
        }
        (plan, _) => plan,
    }
}

fn env_threads() -> Option<usize> {
    std::env::var(ENV_THREADS)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
}

pub(crate) fn build_embedder(threads: usize) -> Result<Embedder, String> {
    build_device_embedder(threads, lumen_embedding_ort::Device::Cpu, false, false)
}

pub(crate) fn model_variant() -> lumen_embedding_ort::ModelVariant {
    use lumen_embedding_ort::ModelVariant;
    std::env::var(ENV_VARIANT)
        .ok()
        .and_then(|v| ModelVariant::parse(v.trim()))
        .unwrap_or(ModelVariant::Q4)
}

fn build_device_embedder(
    threads: usize,
    device: lumen_embedding_ort::Device,
    images: bool,
    image_gpu: bool,
) -> Result<Embedder, String> {
    use lumen_embedding_ort::{OrtBackend, OrtConfig, init_runtime};
    let dir = crate::provisioning::model_dir().ok_or("semantic search is not installed")?;
    let dylib = crate::provisioning::runtime_library().ok_or("no ONNX Runtime library")?;
    init_runtime(&dylib).map_err(|e| e.to_string())?;
    let mut cfg = OrtConfig::new(dir, model_variant(), device);
    cfg.threads = Some(threads);
    cfg.max_batch = QueueConfig::default().batch;
    if images {
        cfg.vision_dir = crate::provisioning::vision_dir();
        if image_gpu {
            cfg.image_device = device;
        }
    }
    let backend = OrtBackend::new(cfg).map_err(|e| e.to_string())?;
    Embedder::new(Arc::new(backend), EmbeddingProfile::DEFAULT).map_err(|e| e.to_string())
}

/// One queue slice. Returns what the thread should do next.
pub(crate) fn embed_slice<R: Runtime>(
    app: &AppHandle<R>,
    db: &Path,
    token: &CancellationToken,
) -> Next {
    let state = app.state::<Indexing>();
    if !model_configured() || *state.failed.lock().unwrap_or_else(PoisonError::into_inner) {
        return Next::Idle;
    }
    if state.control.is_paused() {
        state.set_semantic(Semantic::PausedByUser);
        unload(&state);
        return Next::Idle;
    }
    if app.state::<crate::gpu::Gpu>().status() == crate::gpu::Status::Checking {
        state.set_semantic(Semantic::CheckingGpu);
        tray::refresh_indexing(app);
        return Next::RetryIn(POLICY_RETRY);
    }
    let mut store = match Store::open_writer(db) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("lumen: embedding skipped: {err}");
            return Next::Idle;
        }
    };

    // The space key is only known once a backend exists; the policy needs it for probes
    // only; the GPU manager uses its validated probe's key before the first load.
    let space_key = state
        .loaded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|l| l.embedder.space().key())
        .unwrap_or_default();
    let (device, threads) = match current_plan(app, &space_key) {
        IndexingPlan::Paused(why) => {
            state.set_semantic(Semantic::Waiting(why));
            unload(&state);
            tray::refresh_indexing(app);
            return Next::RetryIn(POLICY_RETRY);
        }
        IndexingPlan::Run { device, threads } => (
            lumen_embedding_ort::Device::parse(&device).unwrap_or(lumen_embedding_ort::Device::Cpu),
            threads,
        ),
    };

    // Expensive media work pauses on battery (PERFORMANCE §10); text keeps its policy.
    let images = !matches!(current_system_state().power, PowerSource::Battery { .. });
    let image_gpu = images
        && device != lumen_embedding_ort::Device::Cpu
        && app.state::<crate::gpu::Gpu>().images_ready();
    let mut loaded = state.loaded.lock().unwrap_or_else(PoisonError::into_inner);
    if loaded.as_ref().is_none_or(|l| {
        l.threads != threads || l.device != device || l.images != images || l.image_gpu != image_gpu
    }) {
        *loaded = None;
        match build_device_embedder(threads, device, images, image_gpu) {
            Ok(embedder) => {
                *loaded = Some(Loaded {
                    threads,
                    device,
                    images,
                    image_gpu,
                    embedder,
                    generation: None,
                });
            }
            Err(err) => {
                if device != lumen_embedding_ort::Device::Cpu {
                    drop(loaded);
                    crate::gpu::failed(app);
                    return Next::More;
                }
                eprintln!("lumen: semantic indexing unavailable: {err}");
                *state.failed.lock().unwrap_or_else(PoisonError::into_inner) = true;
                state.set_semantic(Semantic::Failed);
                tray::refresh_indexing(app);
                return Next::Idle;
            }
        }
    }
    let Some(l) = loaded.as_mut() else {
        return Next::Idle;
    };
    let generation = match l.generation {
        Some(g) => g,
        None => {
            let key = l.embedder.space().key();
            match store.ensure_generation(
                GenerationSpec {
                    space_key: &key,
                    chunker_version: EXTRACTOR_VERSION,
                    dim: l.embedder.profile().dim,
                },
                now_ms(),
            ) {
                Ok(g) => {
                    // The first generation of a database is searchable while it fills.
                    if let Err(err) = store.promote_first(g, now_ms()) {
                        eprintln!("lumen: generation {g} not promoted: {err}");
                    }
                    l.generation = Some(g);
                    g
                }
                Err(err) => {
                    eprintln!("lumen: embedding skipped: {err}");
                    return Next::Idle;
                }
            }
        }
    };
    if store
        .queue_counts(generation)
        .is_ok_and(|c| c.pending() == 0)
    {
        drop(loaded);
        refresh_counts(&state, &store, generation);
        state.set_semantic(Semantic::Idle);
        unload(&state);
        maintain_ann(&state, &mut store, db, generation, token);
        tray::refresh_indexing(app);
        return Next::Idle;
    }

    refresh_counts(&state, &store, generation);
    state.set_semantic(if device == lumen_embedding_ort::Device::Cpu {
        Semantic::Running { threads }
    } else {
        Semantic::RunningGpu
    });
    tray::refresh_indexing(app);
    let job = QueueJob {
        embedder: &l.embedder,
        generation,
        control: &state.control,
        cancel: token,
        cfg: QueueConfig {
            max_run: SLICE,
            ..QueueConfig::default()
        },
    };
    let now = now_ms;
    let baseline = state.status();
    let mut last_published = Instant::now();
    let mut last_modality = None;
    let result = run_queue(&mut store, &job, &now, &mut |r| {
        if r.working {
            state.set_semantic(if r.modality == Some(Modality::Image) {
                Semantic::RunningImages { gpu: image_gpu }
            } else if device == lumen_embedding_ort::Device::Cpu {
                Semantic::Running { threads }
            } else {
                Semantic::RunningGpu
            });
        }
        {
            let mut s = state.status.lock().unwrap_or_else(PoisonError::into_inner);
            s.embedded = baseline.embedded + r.embedded;
            s.vector_failures = baseline.vector_failures + r.failed;
            s.images.indexed = baseline.images.indexed + r.images_embedded;
            s.images.pending = baseline.images.pending.saturating_sub(r.images_embedded);
        }
        if r.modality != last_modality || last_published.elapsed() >= Duration::from_secs(1) {
            crate::progress::publish(app);
            last_published = Instant::now();
            last_modality = r.modality;
        }
    });
    drop(loaded);
    refresh_counts(&state, &store, generation);
    if result.as_ref().is_ok_and(|r| r.stale_images > 0) {
        crate::catalog::request_work(app);
    }
    if result.as_ref().is_ok_and(|r| r.embedded > 0) {
        maintain_ann(&state, &mut store, db, generation, token);
        crate::catalog::notify(app, true);
    }
    let next = match result {
        Ok(r) => {
            if r.embedded > 0 && r.busy > Duration::ZERO {
                crate::diag::record(
                    "embed_chunks_per_s",
                    count(r.embedded) / r.busy.as_secs_f64(),
                );
            }
            match r.stop {
                Stop::TimeSlice | Stop::Cancelled => Next::More,
                Stop::Paused => {
                    state.set_semantic(Semantic::PausedByUser);
                    unload(&state);
                    Next::Idle
                }
                Stop::Drained => {
                    let battery_images = !images && state.status().images.pending > 0;
                    state.set_semantic(if battery_images {
                        Semantic::Waiting(PauseReason::OnBattery)
                    } else {
                        Semantic::Idle
                    });
                    unload(&state);
                    if battery_images {
                        Next::RetryIn(POLICY_RETRY)
                    } else {
                        Next::Idle
                    }
                }
            }
        }
        Err(QueueError::Device(err)) => {
            eprintln!("lumen: embedding device failed: {err}");
            if device != lumen_embedding_ort::Device::Cpu {
                *state.loaded.lock().unwrap_or_else(PoisonError::into_inner) = None;
                crate::gpu::failed(app);
                state.set_semantic(Semantic::Idle);
                tray::refresh_indexing(app);
                return Next::More;
            }
            *state.failed.lock().unwrap_or_else(PoisonError::into_inner) = true;
            state.set_semantic(Semantic::Failed);
            *state.loaded.lock().unwrap_or_else(PoisonError::into_inner) = None;
            Next::Idle
        }
        Err(err) => {
            eprintln!("lumen: embedding stopped: {err}");
            Next::RetryIn(POLICY_RETRY)
        }
    };
    tray::refresh_indexing(app);
    next
}

fn vector_dir(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join(VECTOR_DIR)
}

/// ANN upkeep after a queue slice (ADR-031): refresh the delta, rebuild the file when due,
/// and switch a completed, validated generation in. Failures are logged; search falls back
/// to lexical results meanwhile.
fn maintain_ann(
    state: &Indexing,
    store: &mut Store,
    db: &Path,
    generation: i64,
    token: &CancellationToken,
) {
    let started = Instant::now();
    if let Err(err) = try_maintain_ann(state, store, &vector_dir(db), generation, token) {
        eprintln!("lumen: ANN maintenance: {err}");
    }
    crate::diag::record(
        "ann_maintenance_ms",
        started.elapsed().as_secs_f64() * 1000.0,
    );
}

/// Locks are held only for in-memory steps (refresh, swap); opening, building and
/// validating happen outside them, so a rebuild never stalls the search thread.
fn try_maintain_ann(
    state: &Indexing,
    store: &mut Store,
    dir: &Path,
    generation: i64,
    token: &CancellationToken,
) -> Result<(), String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let info = |store: &Store| -> Result<_, String> {
        store
            .generations()
            .map_err(|x| e(&x))?
            .into_iter()
            .find(|g| g.id == generation)
            .ok_or_else(|| "generation vanished".to_owned())
    };
    let current = info(store)?;
    let open = |store: &Store, g| {
        SemanticIndex::open(store, dir, g, IndexSettings::default()).map_err(|x| e(&x))
    };
    let swap = |index: SemanticIndex| {
        *state.ann.write().unwrap_or_else(PoisonError::into_inner) = Some(index);
    };

    // 1. The index for this generation, up to date.
    let refreshed = {
        let mut ann = state.ann.write().unwrap_or_else(PoisonError::into_inner);
        match ann.as_mut() {
            Some(index)
                if index.generation().id == generation
                    && index.generation().state == current.state =>
            {
                index.refresh(store).map_err(|x| e(&x))?;
                true
            }
            _ => false,
        }
    };
    if !refreshed {
        swap(open(store, current.clone())?);
    }

    // 2. Rebuild the file when due.
    let due = state
        .ann
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|ix| ix.maintenance(store))
        .transpose()
        .map_err(|x| e(&x))?
        == Some(Maintenance::Rebuild);
    if due {
        let started = Instant::now();
        let record = build_file(store, dir, generation, token, now_ms()).map_err(|x| e(&x))?;
        store.set_ann_file(&record).map_err(|x| e(&x))?;
        swap(open(store, current.clone())?);
        crate::diag::record("ann_build_ms", started.elapsed().as_secs_f64() * 1000.0);
        crate::diag::record("ann_vectors", count(record.vectors));
    }

    // 3. A later generation takes over only when complete and validated.
    if current.state == GenerationState::Building {
        let verdict = {
            let ann = state.ann.read().unwrap_or_else(PoisonError::into_inner);
            match ann.as_ref() {
                Some(ix) => Some(validate(store, ix, VALIDATION_SAMPLE).map_err(|x| e(&x))?),
                None => None,
            }
        };
        if let Some(v) = verdict {
            if v.ok {
                store
                    .activate_generation(generation, now_ms())
                    .map_err(|x| e(&x))?;
                swap(open(store, info(store)?)?);
            } else if v.complete {
                eprintln!(
                    "lumen: generation {generation} not activated: self-recall {:.2}, {} of {} failed",
                    v.self_recall, v.failed, v.chunks
                );
            }
        }
    }

    // 4. Retired generations and unrecorded files go.
    while !token.is_cancelled() {
        if store
            .delete_retired_vectors(RETIRE_BATCH)
            .map_err(|x| e(&x))?
            == 0
        {
            break;
        }
    }
    cleanup_files(store, dir).map_err(|x| e(&x))?;
    Ok(())
}

fn refresh_counts(state: &Indexing, store: &Store, generation: i64) {
    if let Ok(c) = store.queue_counts(generation) {
        let mut s = state.status.lock().unwrap_or_else(PoisonError::into_inner);
        s.chunks = c.chunks;
        s.embedded = c.embedded;
        s.vector_failures = c.failed;
    }
    if let Ok(c) = store.image_counts(Some(generation)) {
        state
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .images = c;
    }
}

pub(crate) fn image_status_text(c: &lumen_storage::images::ImageCounts, gpu: bool) -> String {
    format!(
        "Images: {} visual, {} pending, {} failed, {} skipped · {}",
        c.indexed,
        c.pending,
        c.failed,
        c.skipped,
        if gpu { "GPU model + CPU vision" } else { "CPU" }
    )
}

/// Frees the model's memory (the queue drained or cannot run).
fn unload(state: &Indexing) {
    if let Some(l) = state
        .loaded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = l.embedder.backend().unload(Modality::Text);
        let _ = l.embedder.backend().unload(Modality::Image);
    }
}

/// Tray status line.
pub(crate) fn status_text(s: &Status) -> String {
    let files = match s.files {
        0 => "No file contents indexed yet".to_owned(),
        1 => "Contents of 1 file indexed".to_owned(),
        n => format!("Contents of {n} files indexed"),
    };
    let semantic = match &s.semantic {
        Semantic::CheckingGpu => "checking GPU compatibility".into(),
        Semantic::Reading(kind) => format!("reading {kind} · vectors continue between batches"),
        Semantic::RunningImages { gpu } => {
            format!("embedding images · {}", if *gpu { "GPU" } else { "CPU" })
        }
        Semantic::NoModel => "semantic search not installed".to_owned(),
        Semantic::Failed => "semantic indexing failed (see log)".to_owned(),
        Semantic::PausedByUser => "paused".to_owned(),
        Semantic::Waiting(why) => format!(
            "waiting: {}",
            match why {
                PauseReason::OnBattery => "on battery",
                PauseReason::LowBattery => "battery low",
                PauseReason::MemoryPressure => "memory low",
                PauseReason::EcoProfile => "Eco profile",
                PauseReason::UserActive => "you are using the PC",
            }
        ),
        Semantic::Idle | Semantic::Running { .. } | Semantic::RunningGpu if s.chunks == 0 => {
            "nothing to embed".to_owned()
        }
        Semantic::Idle | Semantic::Running { .. } | Semantic::RunningGpu => {
            #[allow(clippy::cast_precision_loss)]
            let pct = 100.0 * s.embedded as f64 / s.chunks as f64;
            if s.embedded >= s.chunks {
                "semantic index up to date".to_owned()
            } else {
                let progress = format!(
                    "semantic {pct:.0}% ({} of {} passages)",
                    s.embedded, s.chunks
                );
                if s.semantic == Semantic::RunningGpu {
                    format!("{progress} · GPU text")
                } else {
                    progress
                }
            }
        }
    };
    format!("{files} · {semantic}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_windows::system::PowerStatus;

    #[test]
    fn system_state_maps_power_and_idle() {
        let s = system_state(
            Some(PowerStatus {
                on_battery: Some(true),
                battery_percent: Some(55),
            }),
            Some(4096),
            Some(Duration::from_secs(600)),
            12,
        );
        assert_eq!(s.power, PowerSource::Battery { percent: Some(55) });
        assert!(!s.user_active);
        let s = system_state(None, None, None, 12);
        assert_eq!(s.power, PowerSource::Unknown);
        assert!(s.user_active, "unknown idle counts as active");
        // The policy then gives a quarter of the logical CPUs (Balanced, active user).
        let plan = policy::plan("k", &[], &Quarantine::new(), &s, &PolicyConfig::default());
        assert_eq!(
            plan.indexing,
            IndexingPlan::Run {
                device: "cpu".into(),
                threads: 3
            }
        );
    }

    #[test]
    fn status_lines() {
        let mut s = Status {
            coverage: Default::default(),
            image_files: 0,
            known: false,
            vector_failures: 0,
            images: Default::default(),
            files: 0,
            chunks: 0,
            embedded: 0,
            semantic: Semantic::NoModel,
        };
        assert_eq!(
            status_text(&s),
            "No file contents indexed yet · semantic search not installed"
        );
        s.files = 12;
        s.chunks = 200;
        s.embedded = 50;
        s.semantic = Semantic::Running { threads: 3 };
        assert_eq!(
            status_text(&s),
            "Contents of 12 files indexed · semantic 25% (50 of 200 passages)"
        );
        s.embedded = 200;
        s.semantic = Semantic::Idle;
        assert!(status_text(&s).ends_with("semantic index up to date"));
        s.semantic = Semantic::Waiting(PauseReason::LowBattery);
        assert!(status_text(&s).ends_with("waiting: battery low"));
        s.semantic = Semantic::Reading("text and PDFs");
        assert!(status_text(&s).contains("reading text and PDFs"));
        assert!(!status_text(&s).contains("nothing to embed"));
    }
}
