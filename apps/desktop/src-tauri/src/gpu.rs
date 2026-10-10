//! Persisted opt-in GPU indexing, discovery and isolated probes (T212).

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use lumen_embedding::policy::{IndexingPlan, Quarantine, SystemState, accelerated_indexing_plan};
use lumen_windows::gpu::DedicatedGpu;
use tauri::{App, AppHandle, Manager, Runtime};

use crate::gpu_probe::{Report, Request};
use crate::{indexing, provisioning, settings, tray};

const SETTING: &str = "indexing.gpu.enabled";
const CACHE: &str = "indexing.gpu.probe";
const QUARANTINE: &str = "indexing.gpu.quarantine";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    Discovering,
    NoDevice,
    NeedsRuntime,
    NeedsRestart,
    Available(String),
    Deferred,
    Checking,
    Ready(String),
    Fallback,
}

pub(crate) struct Gpu {
    enabled: AtomicBool,
    probing: AtomicBool,
    recheck: AtomicBool,
    persisting: Mutex<()>,
    status: Mutex<Status>,
    report: Mutex<Option<Report>>,
}

impl Gpu {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub(crate) fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    fn set(&self, status: Status) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }

    pub(crate) fn images_ready(&self) -> bool {
        self.enabled()
            && self
                .report
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .and_then(|r| r.images.as_ref())
                .is_some_and(|m| m.accepted())
    }
    pub(crate) fn plan(&self, space: &str, state: &SystemState) -> Option<IndexingPlan> {
        if !self.enabled() {
            return None;
        }
        let report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        let report = report.as_ref()?;
        let space = if space.is_empty() {
            report.space.as_str()
        } else {
            space
        };
        Some(accelerated_indexing_plan(
            space,
            &report.probes(),
            &Quarantine::new(),
            state,
        ))
    }
}

pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let enabled =
        settings::get_raw(&app.state::<settings::Settings>(), SETTING).is_some_and(|s| s == "true");
    app.manage(Gpu {
        enabled: AtomicBool::new(enabled),
        probing: AtomicBool::new(false),
        recheck: AtomicBool::new(false),
        persisting: Mutex::new(()),
        status: Mutex::new(Status::Discovering),
        report: Mutex::new(None),
    });
}

pub(crate) fn text(status: &Status) -> String {
    match status {
        Status::Discovering => "Checking for a dedicated GPU…".into(),
        Status::NoDevice => "No compatible dedicated GPU; queries and indexing use CPU".into(),
        Status::NeedsRuntime => "GPU acceleration needs its optional runtime".into(),
        Status::NeedsRestart => "GPU acceleration installed; restart Lumen to use it".into(),
        Status::Available(name) => format!("{name} available; queries use CPU"),
        Status::Deferred => "GPU check waiting for AC power and indexing to resume".into(),
        Status::Checking => "Checking GPU compatibility… Queries use CPU".into(),
        Status::Ready(name) => format!("{name} ready for indexing; queries use CPU"),
        Status::Fallback => "GPU unavailable or failed its check; using CPU".into(),
    }
}

pub(crate) fn choose<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if enabled
        && matches!(
            app.state::<Gpu>().status(),
            Status::NeedsRuntime | Status::NeedsRestart
        )
    {
        provisioning::ask_gpu_download(app);
        tray::refresh_indexing(app);
        return;
    }
    remember(app, enabled);
    if enabled {
        discover(app);
    } else {
        crate::catalog::request_work(app);
    }
    tray::refresh_indexing(app);
}

/// Saves the user's accepted GPU-download preference; activation follows a restart.
pub(crate) fn remember<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    app.state::<Gpu>().enabled.store(enabled, Ordering::Release);
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("lumen-gpu-setting".into())
        .spawn(move || {
            let state = handle.state::<Gpu>();
            let _guard = state
                .persisting
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            settings::set_raw(
                &handle.state::<settings::Settings>(),
                SETTING,
                if state.enabled() { "true" } else { "false" },
            );
        });
}

pub(crate) fn discover<R: Runtime>(app: &AppHandle<R>) {
    if app.state::<Gpu>().probing.swap(true, Ordering::AcqRel) {
        app.state::<Gpu>().recheck.store(true, Ordering::Release);
        return;
    }
    let handle = app.clone();
    if std::thread::Builder::new()
        .name("lumen-gpu-probe".into())
        .spawn(move || {
            let result = discover_inner(&handle);
            if result.is_err() {
                handle.state::<Gpu>().set(Status::Fallback);
            }
            handle
                .state::<Gpu>()
                .probing
                .store(false, Ordering::Release);
            if handle.state::<Gpu>().recheck.swap(false, Ordering::AcqRel) {
                discover(&handle);
            }
            crate::catalog::request_work(&handle);
            tray::refresh_indexing(&handle);
        })
        .is_err()
    {
        app.state::<Gpu>().probing.store(false, Ordering::Release);
        app.state::<Gpu>().set(Status::Fallback);
    }
}

fn discover_inner<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<Gpu>();
    let Some(adapter) = lumen_windows::gpu::dedicated_gpus()?.into_iter().next() else {
        state.set(Status::NoDevice);
        return Ok(());
    };
    let Some(runtime) = provisioning::runtime_library() else {
        state.set(if provisioning::gpu_runtime_installed() {
            Status::NeedsRestart
        } else {
            Status::NeedsRuntime
        });
        return Ok(());
    };
    lumen_embedding_ort::init_runtime(&runtime).map_err(|e| e.to_string())?;
    if !lumen_embedding_ort::directml_available().map_err(|e| e.to_string())? {
        state.set(if provisioning::gpu_runtime_installed() {
            Status::NeedsRestart
        } else {
            Status::NeedsRuntime
        });
        return Ok(());
    }
    if !state.enabled() {
        state.set(Status::Available(adapter.name));
        return Ok(());
    }
    let Some(model) = provisioning::model_dir() else {
        state.set(Status::Available(adapter.name));
        return Ok(());
    };
    if !probe_resources_ready(&indexing::current_system_state())
        || app.state::<indexing::Indexing>().paused()
    {
        state.set(Status::Deferred);
        return Ok(());
    }
    state.set(Status::Checking);
    *state.report.lock().unwrap_or_else(PoisonError::into_inner) = None;
    tray::refresh_indexing(app);
    let variant = indexing::model_variant();
    let key = cache_key(&adapter, &runtime, &model, variant.file_stem())?;
    if settings::get_raw(&app.state::<settings::Settings>(), QUARANTINE)
        .and_then(|s| serde_json::from_str::<String>(&s).ok())
        .is_some_and(|s| s == key)
    {
        state.set(Status::Fallback);
        return Ok(());
    }
    let cached = settings::get_raw(&app.state::<settings::Settings>(), CACHE)
        .and_then(|s| serde_json::from_str::<Report>(&s).ok())
        .filter(|r| r.valid_for(&key, adapter.adapter));
    let report = match cached {
        Some(report) => report,
        None => {
            let request = Request {
                vision: provisioning::vision_dir(),
                key: key.clone(),
                model,
                runtime,
                variant: variant.name().into(),
                adapter: adapter.adapter,
                name: adapter.name,
                total_mib: adapter.memory_mib,
                threads: (std::thread::available_parallelism().map_or(2, usize::from) / 2).max(1),
            };
            // Probe inference competes only with queries; persistent background work resumes
            // immediately when this guard drops, including timeout/cancellation/error.
            let _hold = app.state::<indexing::Indexing>().control().hold();
            match run_child(app, &request) {
                Ok(report) => report,
                Err(error) => {
                    if state.enabled() {
                        settings::set_raw(
                            &app.state::<settings::Settings>(),
                            QUARANTINE,
                            &serde_json::to_string(&key).map_err(|e| e.to_string())?,
                        );
                    }
                    return Err(error);
                }
            }
        }
    };
    let test_state = SystemState {
        power: lumen_embedding::policy::PowerSource::Ac,
        profile: lumen_embedding::policy::ResourceProfile::Balanced,
        logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
        available_memory_mib: None,
        user_active: true,
    };
    let chosen = accelerated_indexing_plan(
        &report.space,
        &report.probes(),
        &Quarantine::new(),
        &test_state,
    );
    if !matches!(chosen, IndexingPlan::Run { ref device, .. } if device != "cpu") {
        state.set(Status::Fallback);
        return Ok(());
    }
    settings::set_raw(
        &app.state::<settings::Settings>(),
        CACHE,
        &serde_json::to_string(&report).map_err(|e| e.to_string())?,
    );
    state.set(Status::Ready(report.name.clone()));
    *state.report.lock().unwrap_or_else(PoisonError::into_inner) = Some(report);
    Ok(())
}

fn cache_key(
    adapter: &DedicatedGpu,
    runtime: &Path,
    model: &Path,
    stem: &str,
) -> Result<String, String> {
    let hash = |path: &Path| lumen_provision::sha256_file(path).map_err(|e| e.to_string());
    let mut parts = vec![
        "gpu-probe-v3-warm-image-backbone".to_owned(),
        adapter.identity.clone(),
        hash(runtime)?,
        hash(&model.join("tokenizer.json"))?,
        hash(&model.join("onnx").join(format!("{stem}.onnx")))?,
    ];
    if let Some(dir) = runtime.parent() {
        for name in ["DirectML.dll", "onnxruntime_providers_shared.dll"] {
            let file = dir.join(name);
            if file.is_file() {
                parts.push(hash(&file)?);
            }
        }
    }
    let weights = model.join("onnx").join(format!("{stem}.onnx_data"));
    if weights.is_file() {
        parts.push(hash(&weights)?);
    }
    if let Some(vision) = provisioning::vision_dir() {
        parts.push(hash(&vision.join("onnx/vision_encoder_q4.onnx"))?);
        parts.push(hash(&vision.join("onnx/vision_encoder_q4.onnx_data"))?);
    }
    Ok(parts.join("/"))
}

fn run_child<R: Runtime>(app: &AppHandle<R>, request: &Request) -> Result<Report, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("gpu-probe");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = dir.join(format!("{}-request.json", std::process::id()));
    let output = dir.join(format!("{}-report.json", std::process::id()));
    let _ = std::fs::remove_file(&output);
    std::fs::write(
        &input,
        serde_json::to_vec(request).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--gpu-probe")
        .arg(&input)
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let result = (|| {
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let started = Instant::now();
        // Image validation adds twelve native visual calls, including shape warmup.
        // Keep a finite child lifetime without rejecting the text route halfway through.
        let timeout = Duration::from_secs(if request.vision.is_some() { 240 } else { 120 });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < timeout && app.state::<Gpu>().enabled() => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("GPU check cancelled, failed or timed out".into());
                }
            }
        };
        if !status.success() {
            return Err("GPU check failed".into());
        }
        if std::fs::metadata(&output).map_err(|e| e.to_string())?.len() > 65_536 {
            return Err("GPU report too large".into());
        }
        let bytes = std::fs::read(&output).map_err(|e| e.to_string())?;
        let report: Report = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if !report.valid_for(&request.key, request.adapter) {
            return Err("GPU check returned incompatible data".into());
        }
        Ok(report)
    })();
    let _ = std::fs::remove_file(input);
    let _ = std::fs::remove_file(output);
    result
}

fn probe_resources_ready(state: &SystemState) -> bool {
    state.power == lumen_embedding::policy::PowerSource::Ac
        && state.available_memory_mib.is_none_or(|mib| {
            mib >= lumen_embedding::policy::PolicyConfig::default().min_available_memory_mib
        })
}

/// Reuses existing queue slices/policy retries; no additional hidden timer.
pub(crate) fn retry_if_ready<R: Runtime>(app: &AppHandle<R>, state: &SystemState) {
    let gpu = app.state::<Gpu>();
    if gpu.enabled() && gpu.status() == Status::Deferred && probe_resources_ready(state) {
        discover(app);
    }
}

pub(crate) fn failed<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Gpu>();
    if let Some(report) = state
        .report
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        settings::set_raw(
            &app.state::<settings::Settings>(),
            QUARANTINE,
            &serde_json::to_string(&report.key).unwrap_or_default(),
        );
    }
    state.set(Status::Fallback);
    tray::refresh_indexing(app);
}

pub(crate) fn runtime_installed<R: Runtime>(app: &AppHandle<R>) {
    app.state::<Gpu>().set(Status::NeedsRestart);
    tray::refresh_indexing(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_embedding::policy::{PowerSource, ResourceProfile};

    #[test]
    fn native_probe_defers_on_battery_unknown_power_and_memory_pressure() {
        let mut state = SystemState {
            power: PowerSource::Ac,
            profile: ResourceProfile::Balanced,
            logical_cpus: 4,
            available_memory_mib: Some(2000),
            user_active: true,
        };
        assert!(probe_resources_ready(&state));
        for power in [
            PowerSource::Unknown,
            PowerSource::Battery { percent: Some(90) },
        ] {
            state.power = power;
            assert!(!probe_resources_ready(&state));
        }
        state.power = PowerSource::Ac;
        state.available_memory_mib = Some(500);
        assert!(!probe_resources_ready(&state));
    }
}
