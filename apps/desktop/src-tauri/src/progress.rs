//! Cached indexing snapshot. No source paths, filesystem work, hidden polling or timers.
use crate::indexing::{Indexing, Semantic};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    pub known: bool,
    pub phase: String,
    pub device: &'static str,
    pub files_total: u64,
    pub files_read: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub passages_total: u64,
    pub passages_ready: u64,
    pub passages_failed: u64,
    pub images_total: u64,
    pub images_ready: u64,
    pub images_pending: u64,
}

pub(crate) fn snapshot(state: &Indexing) -> Progress {
    let s = state.status();
    let (phase, device) = match &s.semantic {
        Semantic::CheckingGpu => ("Checking GPU compatibility".into(), ""),
        Semantic::Reading("image metadata") => ("Preparing image files".into(), "CPU"),
        Semantic::Reading("image text") => ("Reading text in images".into(), "CPU"),
        Semantic::Reading(_) => ("Reading text and PDFs".into(), "CPU"),
        Semantic::RunningGpu => ("Embedding text".into(), "GPU"),
        Semantic::Running { .. } => ("Embedding text".into(), "CPU"),
        Semantic::RunningImages { gpu } => (
            if *gpu {
                "Embedding images · CPU vision + GPU model".into()
            } else {
                "Embedding images".into()
            },
            if *gpu { "GPU + CPU" } else { "CPU" },
        ),
        Semantic::PausedByUser => ("Indexing paused".into(), ""),
        Semantic::Waiting(why) => (
            format!(
                "Waiting: {}",
                match why {
                    lumen_embedding::policy::PauseReason::OnBattery => "on battery",
                    lumen_embedding::policy::PauseReason::LowBattery => "battery low",
                    lumen_embedding::policy::PauseReason::MemoryPressure => "memory low",
                    lumen_embedding::policy::PauseReason::EcoProfile => "Eco profile",
                    lumen_embedding::policy::PauseReason::UserActive => "PC in use",
                }
            ),
            "",
        ),
        Semantic::NoModel => (
            "Content search available · semantic model not installed".into(),
            "",
        ),
        Semantic::Failed => (
            "Embedding failed · file names and contents remain available".into(),
            "",
        ),
        Semantic::Idle if !s.known => ("Scanning indexed locations".into(), "CPU"),
        Semantic::Idle if s.embedded < s.chunks || s.coverage.read < s.coverage.total => {
            ("Indexing queued".into(), "")
        }
        Semantic::Idle => ("Index up to date".into(), ""),
    };
    Progress {
        known: s.known,
        phase,
        device,
        files_total: s.coverage.total,
        files_read: s.coverage.read,
        files_skipped: s.coverage.skipped,
        files_failed: s.coverage.failed,
        passages_total: s.chunks,
        passages_ready: s.embedded,
        passages_failed: s.vector_failures,
        images_total: s.image_files,
        images_ready: s.images.indexed,
        images_pending: s.images.pending,
    }
}

pub(crate) fn publish<R: Runtime>(app: &AppHandle<R>) {
    if crate::overlay::is_shown()
        && let Some(state) = app.try_state::<Indexing>()
    {
        let _ = app.emit_to(
            crate::overlay::WINDOW_LABEL,
            "lumen:indexing-progress",
            snapshot(&state),
        );
    }
}

#[tauri::command]
pub(crate) async fn indexing_progress(
    state: tauri::State<'_, Indexing>,
) -> Result<Progress, String> {
    Ok(snapshot(&state))
}
