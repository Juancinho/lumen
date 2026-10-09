//! Lexical content provider (`lumen.content`, T205): full-text search over extracted
//! chunks (FTS5, ADR-017), one result per file, best passage as the subtitle.
//!
//! Runs on the **settled** query only (`typing == false`): with real hits FTS costs
//! 13/68 ms p50/p95 at 100k chunks on 2 vCPUs (T016), too slow for every keystroke. All
//! content words must match (function words dropped, `FtsQuery::content`); when that finds
//! few files and the query has three or more content words, passages with at least two
//! of them fill up (`FtsQuery::two_of`). Single-word partial matches are not returned: in
//! the evaluation they outranked the semantic lane's right answer (ADR-032).

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use lumen_core::{
    CancellationToken, Confidence, LatencyClass, MatchKind, Provider, ProviderError, ProviderId,
    ProviderQuery, QueryFilters, ResultItem, Score, SearchQuery,
};
use lumen_storage::{
    ChunkHit, FtsQuery, HIGHLIGHT_END, HIGHLIGHT_START, SearchBudget, StorageError, Store,
};

use crate::provider::to_result;

pub const CONTENT_PROVIDER_ID: ProviderId = ProviderId::from_static("lumen.content");

/// Whole-provider budget (settled lane; the UI already shows name results).
const BUDGET: Duration = Duration::from_millis(150);
/// Chunks fetched per file returned (several passages of one file are common).
const CHUNKS_PER_RESULT: usize = 4;

pub struct ContentProvider {
    id: ProviderId,
    store: Mutex<Store>,
}

impl ContentProvider {
    /// `store` should be a reader ([`Store::open_reader`]) dedicated to this provider.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self {
            id: CONTENT_PROVIDER_ID,
            store: Mutex::new(store),
        }
    }
}

/// The snippet as plain text (match markers removed, whitespace collapsed).
fn plain(snippet: &str) -> String {
    snippet
        .chars()
        .filter(|&c| c != HIGHLIGHT_START && c != HIGHLIGHT_END)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Provider for ContentProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn latency_class(&self) -> LatencyClass {
        LatencyClass::Fast
    }

    fn search(
        &self,
        query: &ProviderQuery<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<ResultItem>, ProviderError> {
        if query.typing || query.limit == 0 {
            return Ok(Vec::new());
        }
        let syntax = SearchQuery::parse(query.text);
        if !syntax.valid || !syntax.filters.allows_file_content() {
            return Ok(Vec::new());
        }
        // The last word may be unfinished even when the user paused: prefix it.
        let Some(all) = FtsQuery::content(&syntax.text, true) else {
            return Ok(Vec::new());
        };
        let store = self
            .store
            .lock()
            .map_err(|_| ProviderError::Unavailable("content lock poisoned".into()))?;
        let budget = SearchBudget::within(BUDGET).with_cancel(cancel.clone());
        let want = query.limit * CHUNKS_PER_RESULT;
        let mut hits = run(&store, &all, want, &syntax.filters, &budget, cancel)?;
        let mut files: HashSet<i64> = hits.iter().map(|h| h.item_id).collect();
        if files.len() < query.limit
            && syntax.phrases.is_empty()
            && let Some(any) = FtsQuery::two_of(&syntax.text, true)
        {
            let seen: HashSet<i64> = hits.iter().map(|h| h.chunk_id).collect();
            for h in run(&store, &any, want, &syntax.filters, &budget, cancel)? {
                if !seen.contains(&h.chunk_id) {
                    files.insert(h.item_id);
                    hits.push(h);
                }
            }
        }
        // One result per file, in first-hit order; bm25 is negative, lower = better.
        let best = hits.first().map_or(-1.0, |h| h.rank.min(-1e-9));
        let refs: std::collections::HashMap<_, _> = store
            .chunk_refs(&hits.iter().map(|h| h.chunk_id).collect::<Vec<_>>(), 4000)
            .map_err(unavailable)?
            .into_iter()
            .map(|r| (r.chunk_id, r))
            .collect();
        let mut out = Vec::new();
        let mut done = HashSet::new();
        for hit in &hits {
            if out.len() >= query.limit {
                break;
            }
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            if !done.insert(hit.item_id) {
                continue;
            }
            let Some(item) = store
                .catalog_item_filtered(hit.item_id, &syntax.filters)
                .map_err(unavailable)?
            else {
                continue;
            };
            #[allow(clippy::cast_possible_truncation)]
            let relative = (hit.rank / best).clamp(0.0, 1.0) as f32;
            let score = Score::new(
                Confidence::saturating(0.4 + 0.5 * relative),
                MatchKind::FullText,
            );
            if let Some(mut result) = to_result(&item, score) {
                result.provider = CONTENT_PROVIDER_ID;
                result.subtitle = Some(plain(&hit.snippet));
                if let Some(reference) = refs.get(&hit.chunk_id) {
                    crate::code::enrich(&mut result, reference, item.extension.as_deref());
                }
                out.push(result);
            }
        }
        Ok(out)
    }
}

/// One budgeted FTS query; an exhausted budget returns what earlier steps found (none
/// here), a cancellation cancels.
fn run(
    store: &Store,
    q: &FtsQuery,
    limit: usize,
    filters: &QueryFilters,
    budget: &SearchBudget,
    cancel: &CancellationToken,
) -> Result<Vec<ChunkHit>, ProviderError> {
    match store.search_chunks_filtered(q, limit, filters, budget) {
        Ok(hits) => Ok(hits),
        Err(StorageError::Interrupted) if cancel.is_cancelled() => Err(ProviderError::Cancelled),
        Err(StorageError::Interrupted) => Ok(Vec::new()),
        Err(e) => Err(unavailable(e)),
    }
}

fn unavailable(e: StorageError) -> ProviderError {
    ProviderError::Unavailable(e.to_string())
}

#[cfg(test)]
mod tests {
    use lumen_core::QueryId;
    use lumen_storage::{NewChunk, NewItem};

    use super::*;

    fn query(text: &str, typing: bool) -> ProviderQuery<'_> {
        ProviderQuery {
            id: QueryId::new(1).unwrap(),
            text,
            typing,
            limit: 10,
        }
    }

    #[test]
    fn finds_files_by_their_words_once_settled() {
        let dir =
            std::env::temp_dir().join(format!("lumen-catalog-content-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("lumen.db");
        let mut w = Store::open_writer(&db).unwrap();
        let notes = w
            .insert_item(&NewItem::file("/docs/reunion.md", "reunion.md"))
            .unwrap();
        let code = w
            .insert_item(&NewItem::file("/src/client.py", "client.py"))
            .unwrap();
        let chunk = |item_id, ordinal, text| NewChunk {
            item_id,
            ordinal,
            chunk_kind: "text",
            text,
            symbol_name: None,
            page_number: None,
            start_offset: None,
            end_offset: None,
        };
        w.insert_chunks(&[
            chunk(
                notes,
                0,
                "Notas de la reunión con el cliente sobre el contrato.",
            ),
            chunk(
                notes,
                1,
                "El cliente pide revisar el contrato antes del viernes.",
            ),
            chunk(
                code,
                0,
                "def retry_request(url): retry failed http requests with backoff",
            ),
        ])
        .unwrap();
        let p = ContentProvider::new(Store::open_reader(&db).unwrap());
        let cancel = CancellationToken::new();

        assert!(
            p.search(&query("contrato", true), &cancel)
                .unwrap()
                .is_empty()
        );
        let r = p.search(&query("contrato", false), &cancel).unwrap();
        assert_eq!(r.len(), 1, "two passages, one file");
        assert_eq!(r[0].title, "reunion.md");
        assert_eq!(r[0].provider, CONTENT_PROVIDER_ID);
        assert_eq!(r[0].score.match_kind, MatchKind::FullText);
        assert!(r[0].subtitle.as_deref().unwrap().contains("contrato"));
        assert!(!r[0].subtitle.as_deref().unwrap().contains(HIGHLIGHT_START));

        // Prefix of the last word; function words do not have to occur.
        let title = |q: &str| {
            p.search(&query(q, false), &cancel)
                .unwrap()
                .first()
                .map(|r| r.title.clone())
        };
        assert_eq!(title("retr").as_deref(), Some("client.py"));
        assert_eq!(
            title("el contrato del cliente").as_deref(),
            Some("reunion.md")
        );
        // Two of three content words is enough; one of two is not.
        let n = |q: &str| p.search(&query(q, false), &cancel).unwrap().len();
        assert_eq!(n("contrato cliente backoff"), 1);
        assert_eq!(n("contrato backoff"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn code_language_and_symbol_actions_work_without_a_model() {
        use lumen_core::{Payload, ResultKind, builtin, validate_result};
        let dir = std::env::temp_dir().join(format!("lumen-code-content-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("lumen.db");
        let mut w = Store::open_writer(&db).unwrap();
        for (path, name, language) in [
            ("/repo/retry.ts", "retry.ts", "typescript"),
            ("/repo/retry.py", "retry.py", "python"),
        ] {
            let id = w.insert_item(&NewItem::file(path, name)).unwrap();
            w.insert_chunks(&[NewChunk {
                item_id: id,
                ordinal: 0,
                chunk_kind: "code",
                text: "def retry_request(): exponential backoff for http requests",
                symbol_name: Some("retry_request"),
                page_number: None,
                start_offset: Some(10),
                end_offset: Some(80),
            }])
            .unwrap();
            w.set_code_context(id, path, language, Some("/repo"))
                .unwrap();
        }
        let p = ContentProvider::new(Store::open_reader(&db).unwrap());
        let cancel = CancellationToken::new();
        let results = p
            .search(&query("exponential backoff python", false), &cancel)
            .unwrap();
        assert_eq!(
            results[0].title, "retry.py",
            "language in metadata must beat identical body in another language"
        );
        let single = p.search(&query("backoff python", false), &cancel).unwrap();
        assert_eq!(single.len(), 1);
        for r in &results {
            assert_eq!(r.kind, ResultKind::Code);
            assert!(validate_result(r, &builtin::DESCRIPTORS).is_empty());
            assert!(r.offers(&builtin::COPY_SYMBOL));
            assert!(r.offers(&builtin::REVEAL_REPOSITORY));
            assert_eq!(r.primary_action, builtin::OPEN);
            let Payload::Code(code) = &r.payload else {
                panic!("missing typed code target")
            };
            assert_eq!(code.symbol.as_deref(), Some("retry_request"));
            assert_eq!(code.start_offset, Some(10));
            assert_eq!(
                code.repository.as_deref(),
                Some(std::path::Path::new("/repo"))
            );
        }
        cancel.cancel();
        assert!(matches!(
            p.search(&query("backoff python", false), &cancel),
            Err(ProviderError::Cancelled)
        ));
        drop(p);
        drop(w);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
