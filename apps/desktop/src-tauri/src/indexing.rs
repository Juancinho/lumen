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
//!
//! The model is not provisioned by the app yet (T210): semantic indexing runs only when
//! `LUMEN_EMBED_MODEL_DIR` (an `onnx-community/embeddinggemma-2-ONNX` copy) and
//! `LUMEN_ORT_DYLIB` (onnxruntime.dll) are set. Pause/resume is a tray toggle, remembered in
//! `indexing.paused`.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use lumen_catalog::IndexLocations;
use lumen_content::{
    Control, PassConfig, QueueConfig, QueueError, QueueJob, Stop, run_content_pass, run_queue,
};
use lumen_core::CancellationToken;
use lumen_embedding::policy::{
    self, IndexingPlan, PauseReason, PolicyConfig, PowerSource, Quarantine, ResourceProfile,
    SystemState,
};
use lumen_embedding::{Embedder, EmbeddingProfile, Modality};
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_storage::{GenerationSpec, Store};
use tauri::{App, AppHandle, Manager, Runtime};

use crate::{settings, tray};

pub(crate) const SETTING_PAUSED: &str = "indexing.paused";
pub(crate) const ENV_MODEL_DIR: &str = "LUMEN_EMBED_MODEL_DIR";
pub(crate) const ENV_ORT_DYLIB: &str = "LUMEN_ORT_DYLIB";
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

/// Semantic-indexing state shown in the tray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Semantic {
    /// No model configured (T210 not built): lexical content only.
    NoModel,
    /// The model or runtime failed to load / the device failed; message for logs only.
    Failed,
    Idle,
    Running {
        threads: usize,
    },
    PausedByUser,
    Waiting(PauseReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
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
}

fn model_configured() -> bool {
    std::env::var_os(ENV_MODEL_DIR).is_some() && std::env::var_os(ENV_ORT_DYLIB).is_some()
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
            files: 0,
            chunks: 0,
            embedded: 0,
            semantic,
        }),
        loaded: Mutex::new(None),
        failed: Mutex::new(false),
    });
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

/// The content pass over content-indexed locations. Cheap when nothing changed.
pub(crate) fn content_pass<R: Runtime>(
    app: &AppHandle<R>,
    db: &Path,
    model: &IndexLocations,
    token: &CancellationToken,
) {
    let started = Instant::now();
    let mut store = match Store::open_writer(db) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("lumen: content pass skipped: {err}");
            return;
        }
    };
    let now = now_ms;
    match run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|path| model.indexes_content(path),
        token,
        &now,
        &mut |_| {},
    ) {
        Ok(r) => {
            crate::diag::record("content_pass_ms", started.elapsed().as_secs_f64() * 1000.0);
            crate::diag::record("content_files", count(r.files));
        }
        Err(err) => eprintln!("lumen: content pass failed: {err}"),
    }
    if let Ok(c) = store.content_counts() {
        let state = app.state::<Indexing>();
        let mut s = state.status.lock().unwrap_or_else(PoisonError::into_inner);
        s.files = c.indexed;
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

fn current_plan(space_key: &str) -> IndexingPlan {
    let state = system_state(
        lumen_windows::system::power_status(),
        lumen_windows::system::available_memory_mib(),
        lumen_windows::system::input_idle(),
        std::thread::available_parallelism().map_or(1, usize::from),
    );
    let plan = policy::plan(
        space_key,
        &[],
        &Quarantine::new(),
        &state,
        &PolicyConfig::default(),
    );
    match (plan.indexing, env_threads()) {
        (IndexingPlan::Run { device, .. }, Some(threads)) => IndexingPlan::Run { device, threads },
        (plan, _) => plan,
    }
}

fn env_threads() -> Option<usize> {
    std::env::var(ENV_THREADS)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
}

fn build_embedder(threads: usize) -> Result<Embedder, String> {
    use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig, init_runtime};
    let dir = std::env::var_os(ENV_MODEL_DIR).ok_or("model directory not set")?;
    let dylib = std::env::var_os(ENV_ORT_DYLIB).ok_or("runtime library not set")?;
    init_runtime(Path::new(&dylib)).map_err(|e| e.to_string())?;
    let variant = std::env::var(ENV_VARIANT)
        .ok()
        .and_then(|v| ModelVariant::parse(v.trim()))
        .unwrap_or(ModelVariant::Q4);
    let mut cfg = OrtConfig::new(dir, variant, Device::Cpu);
    cfg.threads = Some(threads);
    cfg.max_batch = QueueConfig::default().batch;
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
    let mut store = match Store::open_writer(db) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("lumen: embedding skipped: {err}");
            return Next::Idle;
        }
    };

    // The space key is only known once a backend exists; the policy needs it for probes
    // only (none yet), so any key is fine before the first load.
    let space_key = state
        .loaded
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|l| l.embedder.space().key())
        .unwrap_or_default();
    let threads = match current_plan(&space_key) {
        IndexingPlan::Paused(why) => {
            state.set_semantic(Semantic::Waiting(why));
            unload(&state);
            tray::refresh_indexing(app);
            return Next::RetryIn(POLICY_RETRY);
        }
        IndexingPlan::Run { threads, .. } => threads,
    };

    let mut loaded = state.loaded.lock().unwrap_or_else(PoisonError::into_inner);
    if loaded.as_ref().is_none_or(|l| l.threads != threads) {
        match build_embedder(threads) {
            Ok(embedder) => {
                *loaded = Some(Loaded {
                    threads,
                    embedder,
                    generation: None,
                });
            }
            Err(err) => {
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
        tray::refresh_indexing(app);
        return Next::Idle;
    }

    state.set_semantic(Semantic::Running { threads });
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
    let result = run_queue(&mut store, &job, &now, &mut |_| {});
    drop(loaded);
    refresh_counts(&state, &store, generation);
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
                    state.set_semantic(Semantic::Idle);
                    unload(&state);
                    Next::Idle
                }
            }
        }
        Err(QueueError::Device(err)) => {
            eprintln!("lumen: embedding device failed: {err}");
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

fn refresh_counts(state: &Indexing, store: &Store, generation: i64) {
    if let Ok(c) = store.queue_counts(generation) {
        let mut s = state.status.lock().unwrap_or_else(PoisonError::into_inner);
        s.chunks = c.chunks;
        s.embedded = c.embedded;
    }
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
        Semantic::Idle | Semantic::Running { .. } if s.chunks == 0 => "nothing to embed".to_owned(),
        Semantic::Idle | Semantic::Running { .. } => {
            #[allow(clippy::cast_precision_loss)]
            let pct = 100.0 * s.embedded as f64 / s.chunks as f64;
            if s.embedded >= s.chunks {
                "semantic index up to date".to_owned()
            } else {
                format!(
                    "semantic {pct:.0}% ({} of {} passages)",
                    s.embedded, s.chunks
                )
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
    }
}
