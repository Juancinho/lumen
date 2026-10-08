//! Root search wiring (T107, T205): the core `SearchService` (latest query wins, settled
//! re-run) with three fused lanes — names (`lumen.catalog`, every keystroke), file contents
//! (`lumen.content`, settled) and meaning (`lumen.semantic`, settled, when a model is
//! configured) — streamed to the UI as `lumen:results` events.
//!
//! The UI numbers its queries (`search(queryId, text)`); the service drops older ids and
//! cancels superseded work, and the UI also ignores updates that are not for its latest id.

use std::sync::Arc;

use lumen_catalog::{CatalogProvider, ContentProvider};
use lumen_core::QueryId;
use lumen_search::{Coordinator, Request, SearchService};
use lumen_semantic::{QueryConfig, QueryEmbedder, SemanticConfig, SemanticProvider};
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

/// Fusion weights per lane (ADR-032; meaning doubled by ADR-033 on `fixtures/eval-hard`).
pub(crate) const WEIGHT_NAME: f32 = 1.0;
pub(crate) const WEIGHT_CONTENT: f32 = 1.0;
pub(crate) const WEIGHT_SEMANTIC: f32 = 2.0;

/// The query model is unloaded after this long without searches (memory, §15).
const QUERY_IDLE_UNLOAD: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Managed state; `None` when the search thread could not start.
pub(crate) struct Search(pub(crate) Option<SearchService>);

/// The query-lane embedder, when a model is configured.
pub(crate) struct QueryLane(pub(crate) Option<Arc<QueryEmbedder>>);

/// Loads the query model in the background (overlay shown and semantic search possible).
pub(crate) fn warm_semantic<R: Runtime>(app: &AppHandle<R>) {
    if let Some(QueryLane(Some(q))) = app.try_state::<QueryLane>().as_deref() {
        q.warm();
    }
}

fn query_threads() -> usize {
    (std::thread::available_parallelism().map_or(2, usize::from) / 2).clamp(1, 4)
}

pub(crate) fn install<R: Runtime>(app: &App<R>) {
    let mut coordinator = Coordinator::new(RESULT_LIMIT);
    let reader = || settings::db_path(app).map(|p| Store::open_reader(&p));
    match reader() {
        Some(Ok(store)) => {
            coordinator.register_weighted(Arc::new(CatalogProvider::new(store)), WEIGHT_NAME);
        }
        Some(Err(err)) => eprintln!("lumen: catalog search unavailable: {err}"),
        None => eprintln!("lumen: no app-data directory; catalog search unavailable"),
    }
    if let Some(Ok(store)) = reader() {
        coordinator.register_weighted(Arc::new(ContentProvider::new(store)), WEIGHT_CONTENT);
    }
    let mut lane = None;
    if crate::indexing::model_configured()
        && let Some(indexing) = app.try_state::<crate::indexing::Indexing>()
        && let Some(Ok(store)) = reader()
    {
        let threads = query_threads();
        match QueryEmbedder::start(
            Box::new(move || crate::indexing::build_embedder(threads)),
            Some(indexing.control().clone()),
            QueryConfig {
                idle_unload: Some(QUERY_IDLE_UNLOAD),
                ..QueryConfig::default()
            },
        ) {
            Ok(q) => {
                let q = Arc::new(q);
                coordinator.register_weighted(
                    Arc::new(SemanticProvider::new(
                        Arc::clone(&q),
                        indexing.shared_index(),
                        store,
                        SemanticConfig::default(),
                    )),
                    WEIGHT_SEMANTIC,
                );
                lane = Some(q);
            }
            Err(err) => eprintln!("lumen: semantic search unavailable: {err}"),
        }
    }
    app.manage(QueryLane(lane));
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
