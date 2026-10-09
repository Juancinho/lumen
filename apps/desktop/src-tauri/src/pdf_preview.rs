//! One lazy preview worker: one running job + one latest pending job, never a FIFO.
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

use base64::Engine;
use futures_channel::oneshot;
use lumen_windows::pdf::{PreviewError, Renderer};
use tauri::{AppHandle, Manager, Runtime};

use crate::dto::PdfPreviewDto;

struct Job {
    id: u64,
    query: u64,
    path: std::path::PathBuf,
    page: u32,
    cancel: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<PdfPreviewDto, String>>,
}

#[derive(Default)]
struct State {
    latest: u64,
    pending: Option<Job>,
    running: Option<(u64, Arc<AtomicBool>)>,
    clear: bool,
    visible: bool,
}

#[derive(Clone)]
pub(crate) struct PdfPreviews(Arc<(Mutex<State>, Condvar)>);

impl PdfPreviews {
    pub(crate) fn start() -> Result<Self, std::io::Error> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("lumen-pdf-preview".into())
            .spawn(move || {
                // OS objects never leave this MTA thread; initialized on its first PDF.
                let mut renderer = Renderer::default();
                let mut rendered_query = None;
                loop {
                    let (lock, wake) = &*worker;
                    let Ok(mut state) = lock.lock() else { break };
                    while state.pending.is_none() && !state.clear {
                        let Ok(next) = wake.wait(state) else { return };
                        state = next;
                    }
                    let clear = std::mem::take(&mut state.clear);
                    let job = state.pending.take();
                    if let Some(job) = &job {
                        state.running = Some((job.id, job.cancel.clone()));
                    }
                    drop(state);
                    // Native object release can be expensive: never hold the mutex the
                    // UI's show/hide callbacks use while clearing renderer memory.
                    if clear {
                        renderer.clear();
                        rendered_query = None;
                    }
                    let Some(job) = job else { continue };
                    // A catalog commit starts a new query, including known same-metadata
                    // writes. Keep cached rasters within that query's page navigation.
                    if rendered_query != Some(job.query) {
                        renderer.clear();
                        rendered_query = Some(job.query);
                    }
                    let result = renderer
                        .render(&job.path, job.page, &|| job.cancel.load(Ordering::Acquire));
                    let result = match result {
                        Ok(page) => Ok(PdfPreviewDto {
                            page_number: page.page_number,
                            page_count: Some(page.page_count),
                            width: Some(page.width),
                            height: Some(page.height),
                            image: Some(format!(
                                "data:image/png;base64,{}",
                                base64::engine::general_purpose::STANDARD.encode(&page.png)
                            )),
                            unavailable: None,
                        }),
                        Err(PreviewError::Cancelled) => Err("preview superseded".into()),
                        Err(error) => Ok(PdfPreviewDto {
                            page_number: job.page,
                            page_count: None,
                            width: None,
                            height: None,
                            image: None,
                            unavailable: Some(error.message()),
                        }),
                    };
                    // No stale image delivery even if OS completion raced with cancellation.
                    if !job.cancel.load(Ordering::Acquire) {
                        let _ = job.reply.send(result);
                    }
                    if let Ok(mut state) = lock.lock() {
                        state.running = None;
                    }
                }
            })?;
        Ok(Self(shared))
    }

    pub(crate) fn submit(
        &self,
        id: u64,
        query: u64,
        path: std::path::PathBuf,
        page: u32,
    ) -> Result<oneshot::Receiver<Result<PdfPreviewDto, String>>, String> {
        let (lock, wake) = &*self.0;
        let mut state = lock.lock().map_err(|_| "preview unavailable")?;
        if !state.visible {
            return Err("overlay hidden".into());
        }
        if id == 0 || id <= state.latest {
            return Err("preview superseded".into());
        }
        state.latest = id;
        cancel_jobs(&mut state);
        let (reply, answer) = oneshot::channel();
        state.pending = Some(Job {
            id,
            query,
            path,
            page,
            reply,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        wake.notify_one();
        Ok(answer)
    }

    pub(crate) fn cancel(&self, id: Option<u64>) {
        let (lock, wake) = &*self.0;
        let Ok(mut state) = lock.lock() else { return };
        if let Some(id) = id {
            if id < state.latest {
                return;
            }
            // Cancellation may arrive before its async render command.
            state.latest = id;
        }
        cancel_jobs(&mut state);
        if id.is_none() {
            state.visible = false;
            state.clear = true;
            wake.notify_one();
        }
    }
}

fn cancel_jobs(state: &mut State) {
    if let Some((_, flag)) = &state.running {
        flag.store(true, Ordering::Release);
    }
    if let Some(job) = state.pending.take() {
        job.cancel.store(true, Ordering::Release);
    }
}

pub(crate) fn install<R: Runtime>(app: &AppHandle<R>) {
    match PdfPreviews::start() {
        Ok(state) => {
            app.manage(state);
        }
        Err(error) => eprintln!("lumen: PDF preview worker unavailable: {error}"),
    }
}

pub(crate) fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<PdfPreviews>() {
        state.cancel(None);
    }
}

pub(crate) fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(service) = app.try_state::<PdfPreviews>()
        && let Ok(mut state) = service.0.0.lock()
    {
        state.visible = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_submit_and_late_cleanup_do_not_reorder_requests() {
        // No worker needed: exercise the same bounded submission/cancellation state.
        let service = PdfPreviews(Arc::new((
            Mutex::new(State {
                visible: true,
                ..State::default()
            }),
            Condvar::new(),
        )));
        service.cancel(Some(1));
        assert!(service.submit(1, 7, "missing.pdf".into(), 1).is_err());
        let answer = service.submit(2, 7, "missing.pdf".into(), 2).unwrap();
        service.cancel(Some(1));
        assert_eq!(service.0.0.lock().unwrap().pending.as_ref().unwrap().id, 2);
        let newer = service.submit(3, 7, "missing.pdf".into(), 3).unwrap();
        assert_eq!(service.0.0.lock().unwrap().pending.as_ref().unwrap().id, 3);
        drop((answer, newer));
        service.cancel(None);
        let state = service.0.0.lock().unwrap();
        assert!(state.pending.is_none() && state.clear && !state.visible);
        drop(state);
        assert!(service.submit(4, 7, "missing.pdf".into(), 1).is_err());
    }
}
