//! T304: cached opt-in/coverage, one native engine on the existing catalog writer.
use lumen_content::{
    Slice,
    ocr::{OcrError, OcrText, Recognizer, run_ocr_slice},
};
use lumen_core::CancellationToken;
use lumen_storage::{Store, ocr::Counts};
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::{App, AppHandle, Manager, Runtime};

const SETTING: &str = "indexing.ocr.enabled";
pub(crate) struct ImageOcr {
    enabled: AtomicBool,
    cleanup: AtomicBool,
    language: Mutex<Option<Result<String, lumen_windows::ocr::Error>>>,
    counts: Mutex<Counts>,
}
impl ImageOcr {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub(crate) fn label(&self) -> String {
        if !self.enabled() && self.cleanup.load(Ordering::Acquire) {
            return "Removing indexed image text…".into();
        }
        let language = self.language.lock().unwrap_or_else(PoisonError::into_inner);
        match language.as_ref() {
            None if !self.enabled() => {
                "Image text (OCR) off · uses installed Windows language".into()
            }
            None => coverage_text(
                &self.counts.lock().unwrap_or_else(PoisonError::into_inner),
                None,
            ),
            Some(Err(lumen_windows::ocr::Error::Language)) => {
                "OCR unavailable: no installed profile language".into()
            }
            Some(Err(_)) => "Windows OCR unavailable on this installation".into(),
            Some(Ok(language)) if !self.enabled() => {
                if self.cleanup.load(Ordering::Acquire) {
                    "Removing indexed image text…".into()
                } else {
                    format!("Image text (OCR) off · {language} available")
                }
            }
            Some(Ok(language)) => {
                let c = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
                coverage_text(&c, Some(language))
            }
        }
    }
}
fn coverage_text(c: &Counts, language: Option<&str>) -> String {
    let done = c.indexed + c.empty + c.skipped + c.failed;
    let total = done + c.pending;
    if total == 0 {
        return "Image text (OCR) on · waiting for prepared images".into();
    }
    let percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
    let language = language.map_or_else(String::new, |l| format!(" · {l}"));
    format!(
        "OCR · {done}/{total} checked ({percent}% of prepared) · {} text, {} empty, {} pending, {} skipped, {} failed{language}",
        c.indexed, c.empty, c.pending, c.skipped, c.failed
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_coverage_is_honest_before_native_language_or_engine_creation() {
        assert!(coverage_text(&Counts::default(), None).contains("waiting for prepared images"));
        let c = Counts {
            indexed: 2,
            empty: 1,
            pending: 7,
            ..Default::default()
        };
        assert!(coverage_text(&c, None).contains("3/10 checked (30% of prepared)"));
        assert!(coverage_text(&c, Some("es-ES")).ends_with(" · es-ES"));
        let c = Counts {
            indexed: 10,
            ..Default::default()
        };
        assert!(coverage_text(&c, None).contains("10/10 checked (100% of prepared)"));
    }
    #[test]
    fn preview_requires_the_path_of_the_remembered_result() {
        let path = std::path::Path::new("C:/synthetic/remembered.png");
        let record = lumen_storage::ocr::Preview {
            path: lumen_catalog::path::encode(path).text,
            state: "indexed".into(),
            language: Some("es-ES".into()),
            reason: None,
            text: "ERROR 42".into(),
        };
        assert!(record_for_path(Some(record.clone()), Some(path)).is_some());
        assert!(record_for_path(Some(record.clone()), None).is_none());
        assert!(
            record_for_path(
                Some(record),
                Some(std::path::Path::new("C:/synthetic/reused.png"))
            )
            .is_none()
        );
    }
}
pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let enabled = crate::settings::get_raw(&app.state::<crate::settings::Settings>(), SETTING)
        .is_some_and(|v| v.trim() == "true");
    app.manage(ImageOcr {
        enabled: AtomicBool::new(enabled),
        cleanup: AtomicBool::new(!enabled),
        language: Mutex::new(None),
        counts: Mutex::new(Counts::default()),
    });
}
pub(crate) fn choose<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    let state = app.state::<ImageOcr>();
    crate::settings::set_raw(
        &app.state::<crate::settings::Settings>(),
        SETTING,
        if enabled { "true" } else { "false" },
    );
    state.enabled.store(enabled, Ordering::Release);
    state.cleanup.store(!enabled, Ordering::Release);
    if enabled {
        *state
            .language
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
    }
    crate::catalog::request_work(app);
    crate::tray::refresh_indexing(app);
}
pub(crate) struct Native(pub(crate) lumen_windows::ocr::Engine);
impl Recognizer for Native {
    fn recognize(
        &mut self,
        width: u32,
        height: u32,
        rgb: &[u8],
        stop: &dyn Fn() -> bool,
    ) -> Result<OcrText, OcrError> {
        self.0
            .recognize(width, height, rgb, stop)
            .map(|t| OcrText {
                text: t.text,
                language: t.language,
            })
            .map_err(|error| match error {
                lumen_windows::ocr::Error::Cancelled => OcrError::Cancelled,
                // Availability was checked before this pass; recognition failure is bounded.
                lumen_windows::ocr::Error::Unavailable | lumen_windows::ocr::Error::Language => {
                    OcrError::Failed("ocr:recognition")
                }
                error => OcrError::Failed(error.code()),
            })
    }
}
#[derive(Default)]
pub(crate) struct Progress {
    cursor: i64,
    done: bool,
    native: Option<Native>,
    pub(crate) deferred: bool,
}
impl Progress {
    pub(crate) fn metadata_finished(&mut self) {
        self.cursor = 0;
        self.done = false;
    }
}

/// Clean disabled OCR even while indexing is paused; bounded, crash-resumable writer pages.
pub(crate) fn cleanup<R: Runtime>(app: &AppHandle<R>, store: &mut Store) -> bool {
    let state = app.state::<ImageOcr>();
    if state.enabled() || !state.cleanup.load(Ordering::Acquire) {
        return false;
    }
    match store.clear_ocr_page() {
        Ok(0) => {
            state.cleanup.store(false, Ordering::Release);
            false
        }
        Ok(_) => {
            crate::catalog::notify(app, true);
            true
        }
        Err(err) => {
            eprintln!("lumen: OCR cleanup deferred: {err}");
            false
        }
    }
}
/// Returns whether another OCR slice remains. Deferred work uses the existing policy retry.
pub(crate) fn run<R: Runtime>(
    app: &AppHandle<R>,
    store: &mut Store,
    model: &lumen_catalog::IndexLocations,
    token: &CancellationToken,
    progress: &mut Progress,
) -> bool {
    let state = app.state::<ImageOcr>();
    progress.deferred = false;
    if !state.enabled() {
        progress.native = None;
        return false;
    }
    if progress.done || token.is_cancelled() {
        progress.native = None;
        return false;
    }
    if let Ok(counts) = store.ocr_counts(&|path| model.indexes_content(path)) {
        if counts.pending == 0 && counts.failed == 0 {
            progress.done = true;
            progress.native = None;
        }
        *state.counts.lock().unwrap_or_else(PoisonError::into_inner) = counts;
    }
    if progress.done {
        return false;
    }
    let indexing = app.state::<crate::indexing::Indexing>();
    let system = crate::indexing::current_system_state();
    if indexing.paused()
        || indexing
            .control()
            .interactive_within(Duration::from_secs(10))
        || matches!(
            system.power,
            lumen_embedding::policy::PowerSource::Battery { .. }
        )
        || system.available_memory_mib.is_some_and(|m| {
            m < lumen_embedding::policy::PolicyConfig::default().min_available_memory_mib
        })
    {
        progress.native = None;
        progress.deferred = true;
        return true;
    }
    // Capability discovery does not read images and never runs on the UI thread.
    if state
        .language
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_none()
    {
        let native = progress
            .native
            .get_or_insert_with(|| Native(Default::default()));
        *state
            .language
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(native.0.language());
    }
    if !state.enabled() || progress.done || token.is_cancelled() {
        return false;
    }
    if let Ok(counts) = store.ocr_counts(&|path| model.indexes_content(path)) {
        *state.counts.lock().unwrap_or_else(PoisonError::into_inner) = counts;
    }
    if !matches!(
        state
            .language
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref(),
        Some(Ok(_))
    ) {
        progress.done = true;
        return false;
    }
    indexing.set_semantic(crate::indexing::Semantic::Reading("image text"));
    crate::tray::refresh_indexing(app);
    let stop = || {
        !state.enabled()
            || indexing.paused()
            || indexing
                .control()
                .interactive_within(Duration::from_secs(10))
    };
    let native = progress
        .native
        .get_or_insert_with(|| Native(Default::default()));
    match run_ocr_slice(
        store,
        &|path| model.indexes_content(path),
        token,
        &stop,
        native,
        progress.cursor,
        Slice {
            max_files: 4,
            max_run: Duration::from_secs(2),
        },
    ) {
        Ok(report) => {
            progress.cursor = report.cursor;
            progress.done = report.exhausted;
            progress.deferred = report.cancelled && !token.is_cancelled();
            crate::diag::record("image_ocr_ms", report.elapsed.as_secs_f64() * 1000.0);
            crate::catalog::notify(app, report.files > 0);
        }
        Err(err) => {
            eprintln!("lumen: OCR pass deferred: {err}");
            progress.done = true;
        }
    }
    if let Ok(counts) = store.ocr_counts(&|path| model.indexes_content(path)) {
        *state.counts.lock().unwrap_or_else(PoisonError::into_inner) = counts;
    }
    if progress.done {
        progress.native = None;
    }
    !progress.done
}

pub(crate) fn enrich<R: Runtime>(
    app: &AppHandle<R>,
    item: &lumen_core::ResultItem,
    dto: &mut crate::dto::PreviewDto,
) {
    if item.kind != lumen_core::ResultKind::Image {
        return;
    }
    let state = app.state::<ImageOcr>();
    let admitted = item.payload.local_path().is_some_and(|path| {
        app.state::<crate::catalog::Catalog>()
            .locations()
            .indexes_content(&lumen_catalog::path::encode(path).text)
    });
    let enabled = state.enabled() && admitted;
    let unavailable = matches!(
        state
            .language
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref(),
        Some(Err(_))
    );
    let record = if enabled {
        item.id
            .as_str()
            .strip_prefix("item:")
            .and_then(|id| id.parse::<i64>().ok())
            .and_then(|id| {
                app.path()
                    .app_data_dir()
                    .ok()
                    .and_then(|dir| Store::open_reader(&dir.join(crate::settings::DB_FILE)).ok())
                    .and_then(|store| store.image_ocr_preview(id).ok().flatten())
            })
    } else {
        None
    };
    let record = record_for_path(record, item.payload.local_path());
    crate::preview::image_ocr(dto, enabled, unavailable, record.as_ref());
}

fn record_for_path(
    record: Option<lumen_storage::ocr::Preview>,
    path: Option<&std::path::Path>,
) -> Option<lumen_storage::ocr::Preview> {
    record.filter(|record| {
        path.is_some_and(|path| lumen_catalog::path::encode(path).text == record.path)
    })
}
