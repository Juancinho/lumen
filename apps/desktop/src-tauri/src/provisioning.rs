//! Semantic search setup (T210, ADR-034): where the model and the inference runtime come
//! from, and the user-initiated, consented download that installs them.
//!
//! Resolution order, for each of the two:
//! 1. the environment (`LUMEN_EMBED_MODEL_DIR`, `LUMEN_ORT_DYLIB`) — development;
//! 2. the runtime next to `lumen.exe` (how a packaged build ships it, ADR-015);
//! 3. what this module installed under the app-data folder (`models/…`, `runtime/…`).
//!
//! Nothing is downloaded until the user picks *Download…* in the tray and confirms a dialog
//! that names the size, the hosts and the licenses. The download uses the system `curl`
//! (no TLS stack in Lumen), resumes after a cancel or a crash and verifies every file's
//! SHA-256 before use.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use lumen_core::CancellationToken;
use lumen_provision::{
    Component, CurlFetch, EMBEDDING_MODEL, INFERENCE_RUNTIME, InstallError, Progress, State,
    install, remove,
};
use tauri::{App, AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::{indexing, tray};

pub(crate) const ENV_MODEL_DIR: &str = "LUMEN_EMBED_MODEL_DIR";
pub(crate) const ENV_ORT_DYLIB: &str = "LUMEN_ORT_DYLIB";

/// The app-data folder (set once at start-up).
static ROOT: OnceLock<PathBuf> = OnceLock::new();

#[cfg(windows)]
const RUNTIME_FILE: &str = "onnxruntime.dll";
#[cfg(not(windows))]
const RUNTIME_FILE: &str = "libonnxruntime.so";

/// What the tray shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Setup {
    Ready,
    /// Not installed; bytes still to download (model + runtime if needed).
    Missing {
        download: u64,
    },
    Downloading {
        done: u64,
        total: u64,
    },
    /// Last attempt failed (message for the tray; the partial download is kept).
    Failed(String),
    /// No runtime can be installed here (non-Windows without `LUMEN_ORT_DYLIB`).
    Unsupported,
}

pub(crate) struct Provisioning {
    setup: Mutex<Setup>,
    running: Mutex<Option<CancellationToken>>,
}

fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

fn installed(c: &Component) -> Option<PathBuf> {
    match lumen_provision::state(root()?, c) {
        State::Installed { dir } => Some(dir),
        _ => None,
    }
}

/// The model folder (`tokenizer.json`, `onnx/model_q4.onnx`), if any.
pub(crate) fn model_dir() -> Option<PathBuf> {
    std::env::var_os(ENV_MODEL_DIR)
        .map(PathBuf::from)
        .or_else(|| installed(&EMBEDDING_MODEL))
}

/// The ONNX Runtime library, if any.
pub(crate) fn runtime_library() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_ORT_DYLIB) {
        return Some(PathBuf::from(p));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(RUNTIME_FILE)))
        .filter(|p| p.is_file());
    beside.or_else(|| installed(&INFERENCE_RUNTIME).map(|d| d.join(RUNTIME_FILE)))
}

/// The model in use is the one this module installed (not a development path).
pub(crate) fn model_removable() -> bool {
    std::env::var_os(ENV_MODEL_DIR).is_none() && installed(&EMBEDDING_MODEL).is_some()
}

/// Both pieces are present: semantic indexing and search can run.
pub(crate) fn ready() -> bool {
    model_dir().is_some() && runtime_library().is_some()
}

/// What a download would still have to fetch: (components, bytes).
fn missing() -> Option<(Vec<Component>, u64)> {
    let mut parts = Vec::new();
    if model_dir().is_none() {
        parts.push(EMBEDDING_MODEL);
    }
    if runtime_library().is_none() {
        if !INFERENCE_RUNTIME.platform_ok {
            return None;
        }
        parts.push(INFERENCE_RUNTIME);
    }
    let bytes = parts.iter().map(Component::download_bytes).sum();
    Some((parts, bytes))
}

fn current_setup() -> Setup {
    if ready() {
        return Setup::Ready;
    }
    match missing() {
        Some((_, download)) => Setup::Missing { download },
        None => Setup::Unsupported,
    }
}

pub(crate) fn install_state<R: Runtime>(app: &App<R>) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = ROOT.set(dir);
    }
    app.manage(Provisioning {
        setup: Mutex::new(current_setup()),
        running: Mutex::new(None),
    });
}

pub(crate) fn setup<R: Runtime>(app: &AppHandle<R>) -> Setup {
    app.try_state::<Provisioning>()
        .map_or(Setup::Unsupported, |p| {
            p.setup
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        })
}

fn set<R: Runtime>(app: &AppHandle<R>, s: Setup) {
    if let Some(p) = app.try_state::<Provisioning>() {
        *p.setup.lock().unwrap_or_else(PoisonError::into_inner) = s;
    }
    tray::refresh_semantic(app);
}

fn mb(bytes: u64) -> u64 {
    bytes.div_ceil(1_000_000)
}

/// Tray status line.
pub(crate) fn setup_text(s: &Setup) -> String {
    match s {
        Setup::Ready => "Semantic search installed".to_owned(),
        Setup::Missing { download } => {
            format!(
                "Semantic search not installed ({} MB download)",
                mb(*download)
            )
        }
        Setup::Downloading { done, total } => {
            let pct = (done * 100).checked_div(*total).unwrap_or(0);
            format!("Downloading… {pct}% of {} MB", mb(*total))
        }
        Setup::Failed(why) => format!("Download stopped: {why}"),
        Setup::Unsupported => "Semantic search needs a runtime on this system".to_owned(),
    }
}

/// The consent text: what, how big, from where, under which license, and what is not sent.
pub(crate) fn consent_text(parts: &[Component], bytes: u64) -> String {
    let mut s = format!(
        "Lumen will download {} MB to enable search by meaning:\n\n",
        mb(bytes)
    );
    for c in parts {
        s.push_str(&format!(
            "• {} — {} MB from {}, license {}\n",
            c.title,
            mb(c.download_bytes()),
            c.host,
            c.license
        ));
    }
    s.push_str(
        "\nOnly these fixed files are requested; nothing about your files or searches is \
         sent. Each file is checked against its published SHA-256 before use. You can \
         remove it again from this menu.",
    );
    s
}

/// Tray → Download…: asks, then installs in the background.
pub(crate) fn ask_download<R: Runtime>(app: &AppHandle<R>) {
    if app.try_state::<Provisioning>().is_some_and(|p| {
        p.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }) {
        return;
    }
    let Some((parts, bytes)) = missing() else {
        set(app, Setup::Unsupported);
        return;
    };
    if parts.is_empty() {
        set(app, Setup::Ready);
        return;
    }
    let handle = app.clone();
    app.dialog()
        .message(consent_text(&parts, bytes))
        .title("Download semantic search?")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Download".into(),
            "Not now".into(),
        ))
        .show(move |ok| {
            if ok {
                start(&handle, parts);
            }
        });
}

fn start<R: Runtime>(app: &AppHandle<R>, parts: Vec<Component>) {
    let Some(root) = root().map(Path::to_owned) else {
        set(app, Setup::Failed("no app-data folder".into()));
        return;
    };
    let token = CancellationToken::new();
    if let Some(p) = app.try_state::<Provisioning>() {
        *p.running.lock().unwrap_or_else(PoisonError::into_inner) = Some(token.clone());
    }
    let handle = app.clone();
    let total: u64 = parts.iter().map(Component::download_bytes).sum();
    let spawned = std::thread::Builder::new()
        .name("lumen-provision".into())
        .spawn(move || {
            let started = Instant::now();
            let fetch = CurlFetch::new();
            let mut before = 0;
            let mut result = Ok(());
            let mut last_pct = u64::MAX;
            for c in &parts {
                let base = before;
                let r = install(&root, c, &fetch, &token, &mut |p: Progress| {
                    let done = base + p.done;
                    let pct = (done * 100).checked_div(total).unwrap_or(100);
                    if pct != last_pct {
                        last_pct = pct;
                        set(&handle, Setup::Downloading { done, total });
                    }
                });
                before += c.download_bytes();
                if let Err(e) = r {
                    result = Err(e);
                    break;
                }
            }
            if let Some(p) = handle.try_state::<Provisioning>() {
                *p.running.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
            match result {
                Ok(()) => {
                    crate::diag::record("provision_s", started.elapsed().as_secs_f64());
                    set(&handle, current_setup());
                    indexing::on_model_installed(&handle);
                    crate::search::on_model_installed(&handle);
                }
                Err(InstallError::Cancelled) => set(&handle, current_setup()),
                Err(e) => {
                    eprintln!("lumen: semantic search download failed: {e}");
                    set(&handle, Setup::Failed(e.to_string()));
                }
            }
        });
    if spawned.is_err() {
        set(app, Setup::Failed("could not start the download".into()));
    }
}

/// Tray → Cancel download.
pub(crate) fn cancel<R: Runtime>(app: &AppHandle<R>) {
    if let Some(p) = app.try_state::<Provisioning>()
        && let Some(t) = p
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
    {
        t.cancel();
    }
}

/// Tray → Remove…: asks, unloads the model and deletes the installed model (the runtime
/// stays: it is small and, once loaded, Windows keeps the DLL locked until Lumen exits).
pub(crate) fn ask_remove<R: Runtime>(app: &AppHandle<R>) {
    if installed(&EMBEDDING_MODEL).is_none() {
        return;
    }
    let handle = app.clone();
    app.dialog()
        .message(format!(
            "Remove the downloaded model ({} MB)? Search by meaning stops until it is \
             downloaded again; names and file contents keep working. Already computed \
             vectors stay, so a later download does not re-index.",
            mb(EMBEDDING_MODEL.installed_bytes())
        ))
        .title("Remove semantic search?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Remove".into(),
            "Keep".into(),
        ))
        .show(move |ok| {
            if !ok {
                return;
            }
            indexing::on_model_removed(&handle);
            crate::search::on_model_removed(&handle);
            // The query lane releases its session on its own thread; Windows refuses to
            // delete mapped files until it has.
            if let Some(root) = root() {
                let mut result = remove(root, &EMBEDDING_MODEL);
                for _ in 0..20 {
                    if result.is_ok() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    result = remove(root, &EMBEDDING_MODEL);
                }
                if let Err(e) = result {
                    eprintln!("lumen: removing the model failed: {e}");
                }
            }
            set(&handle, current_setup());
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_name_size_hosts_and_licenses() {
        let t = consent_text(&[EMBEDDING_MODEL, INFERENCE_RUNTIME], 221_030_242);
        assert!(t.starts_with("Lumen will download 222 MB"));
        assert!(t.contains("huggingface.co, license Apache-2.0"));
        assert!(t.contains("files.pythonhosted.org, license MIT"));
        assert!(t.contains("nothing about your files or searches is sent"));
        assert_eq!(
            setup_text(&Setup::Downloading {
                done: 50,
                total: 200
            }),
            "Downloading… 25% of 1 MB"
        );
        assert_eq!(
            setup_text(&Setup::Missing {
                download: 206_718_772
            }),
            "Semantic search not installed (207 MB download)"
        );
    }
}
