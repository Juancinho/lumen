//! Root search wiring (T107): the core `SearchService` (latest query wins) with the catalog
//! provider, streamed to the UI as `lumen:results` events.
//!
//! The UI numbers its queries (`search(queryId, text)`); the service drops older ids and
//! cancels superseded work, and the UI also ignores updates that are not for its latest id.

use std::sync::Arc;

use lumen_catalog::CatalogProvider;
use lumen_core::QueryId;
use lumen_search::{Coordinator, Request, SearchService};
use lumen_storage::Store;
use tauri::{App, AppHandle, Emitter, Manager, Runtime};

use crate::dto::ResultsDto;
use crate::{overlay, settings};

/// Event with a [`ResultsDto`]. Mirrored in `src/ipc/events.ts`.
pub(crate) const EVENT_RESULTS: &str = "lumen:results";

/// Results per update: 8 visible rows, the rest scrolls.
pub(crate) const RESULT_LIMIT: usize = 30;

/// Development diagnostics (T110): provider, match kind and confidence on every row, and
/// per-query timings. Never on unless `LUMEN_DIAGNOSTICS=1`.
pub(crate) const ENV_DIAGNOSTICS: &str = "LUMEN_DIAGNOSTICS";

pub(crate) fn diagnostics_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var(ENV_DIAGNOSTICS).is_ok_and(|v| v == "1"))
}

/// Managed state; `None` when the search thread could not start.
pub(crate) struct Search(pub(crate) Option<SearchService>);

pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let mut coordinator = Coordinator::new(RESULT_LIMIT);
    match settings::db_path(app).map(|p| Store::open_reader(&p)) {
        Some(Ok(store)) => coordinator.register(Arc::new(CatalogProvider::new(store))),
        Some(Err(err)) => eprintln!("lumen: catalog search unavailable: {err}"),
        None => eprintln!("lumen: no app-data directory; catalog search unavailable"),
    }
    let handle = app.handle().clone();
    let service = SearchService::start(coordinator, move |update| {
        deliver(&handle, &update);
    })
    .map_err(|err| eprintln!("lumen: search thread failed to start: {err}"))
    .ok();
    app.manage(Search(service));
}

fn deliver<R: Runtime>(app: &AppHandle<R>, update: &lumen_search::Update) {
    crate::diag::record(
        if update.done {
            "search_done_ms"
        } else {
            "search_partial_ms"
        },
        update.elapsed.as_secs_f64() * 1000.0,
    );
    for provider in &update.failed {
        eprintln!("lumen: provider {} failed", provider.as_str());
    }
    let payload = ResultsDto::new(update, diagnostics_enabled());
    if let Err(err) = app.emit_to(overlay::WINDOW_LABEL, EVENT_RESULTS, payload) {
        eprintln!("lumen: emit {EVENT_RESULTS} failed: {err}");
    }
}

/// Queues a root search; returns `false` when the id is invalid or not the newest.
pub(crate) fn submit<R: Runtime>(app: &AppHandle<R>, query_id: u64, text: String) -> bool {
    let Some(id) = QueryId::new(query_id) else {
        return false;
    };
    let state = app.state::<Search>();
    let Some(service) = state.0.as_ref() else {
        return false;
    };
    service.submit(Request {
        id,
        text,
        typing: true,
    })
}
