//! Semantic provider (`lumen.semantic`, T205): the settled query embedded by the warm
//! [`QueryEmbedder`] (ADR-030), searched in the active ANN generation (ADR-031), one
//! result per file with the start of the best passage as the subtitle.
//!
//! Latency class `Semantic`: the coordinator calls it only for settled queries. It
//! answers nothing (rather than failing) while there is no active generation, the
//! generation belongs to another vector space (model change in progress), or the query is
//! too short to mean anything.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use lumen_catalog::provider::to_result;
use lumen_core::{
    CancellationToken, Confidence, LatencyClass, MatchKind, Provider, ProviderError, ProviderId,
    ProviderQuery, ResultItem, Score, SearchQuery,
};
use lumen_storage::{ChunkRef, GenerationState, SearchBudget, Store};

use crate::index::SemanticIndex;
use crate::query::{QueryEmbedder, QueryError};

pub const SEMANTIC_PROVIDER_ID: ProviderId = ProviderId::from_static("lumen.semantic");

/// The searchable generation, shared between the indexing thread (writes) and search
/// (reads).
pub type SharedIndex = Arc<RwLock<Option<SemanticIndex>>>;

/// Characters of the passage shown as the subtitle.
const EXCERPT_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticConfig {
    /// Queries shorter than this (in characters, trimmed) are not embedded.
    pub min_query_chars: usize,
    /// Chunks fetched per file returned.
    pub chunks_per_result: usize,
    /// Hits more than this below the best hit's similarity are dropped: every query has
    /// nearest neighbours, and a tail of weak ones only adds noise to fusion.
    pub relative_floor: f32,
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            min_query_chars: 3,
            chunks_per_result: 3,
            relative_floor: 0.15,
        }
    }
}

pub struct SemanticProvider {
    id: ProviderId,
    embedder: Arc<QueryEmbedder>,
    index: SharedIndex,
    store: Mutex<Store>,
    config: SemanticConfig,
}

impl SemanticProvider {
    /// `store` should be a reader ([`Store::open_reader`]) dedicated to this provider.
    #[must_use]
    pub fn new(
        embedder: Arc<QueryEmbedder>,
        index: SharedIndex,
        store: Store,
        config: SemanticConfig,
    ) -> Self {
        Self {
            id: SEMANTIC_PROVIDER_ID,
            embedder,
            index,
            store: Mutex::new(store),
            config,
        }
    }
}

/// Excerpt as one line.
fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() >= EXCERPT_CHARS {
        format!("{line}…")
    } else {
        line
    }
}

impl Provider for SemanticProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn latency_class(&self) -> LatencyClass {
        LatencyClass::Semantic
    }

    fn search(
        &self,
        query: &ProviderQuery<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<ResultItem>, ProviderError> {
        let syntax = SearchQuery::parse(query.text);
        if !syntax.valid || !syntax.phrases.is_empty() || !syntax.filters.allows_file_content() {
            return Ok(Vec::new());
        }
        let text = syntax.text.trim();
        if query.typing || query.limit == 0 || text.chars().count() < self.config.min_query_chars {
            return Ok(Vec::new());
        }
        // Nothing to search: do not load the model for nothing.
        let searchable = self
            .index
            .read()
            .map_err(|_| ProviderError::Unavailable("semantic index lock poisoned".into()))?
            .as_ref()
            .is_some_and(|ix| ix.generation().state == GenerationState::Active);
        if !searchable {
            return Ok(Vec::new());
        }
        let vector = match self.embedder.embed(text, cancel) {
            Ok(v) => v,
            Err(QueryError::Cancelled | QueryError::Superseded | QueryError::Stopped) => {
                return Err(ProviderError::Cancelled);
            }
            Err(e) => return Err(ProviderError::Unavailable(e.to_string())),
        };
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let store = self
            .store
            .lock()
            .map_err(|_| ProviderError::Unavailable("semantic store lock poisoned".into()))?;
        let guard = self
            .index
            .read()
            .map_err(|_| ProviderError::Unavailable("semantic index lock poisoned".into()))?;
        let Some(index) = guard.as_ref() else {
            return Ok(Vec::new());
        };
        let generation = index.generation();
        if generation.state != GenerationState::Active
            || self
                .embedder
                .space_key()
                .is_some_and(|k| k != generation.space_key)
        {
            return Ok(Vec::new());
        }
        let want = query.limit * self.config.chunks_per_result;
        let mut fetch = want;
        let budget =
            SearchBudget::within(std::time::Duration::from_millis(100)).with_cancel(cancel.clone());
        let mut partial = Vec::new();
        let hits = loop {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            if !syntax.filters.0.is_empty()
                && budget
                    .deadline
                    .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                break partial;
            }
            let mut hits = index
                .search(&store, &vector, fetch)
                .map_err(|e| ProviderError::Unavailable(e.to_string()))?;
            if syntax.filters.0.is_empty() {
                break hits;
            }
            let exhausted = hits.len() < fetch;
            let ids: Vec<_> = hits.iter().map(|h| h.chunk_id).collect();
            let allowed = match store.matching_chunk_ids(&ids, &syntax.filters, &budget) {
                Ok(ids) => ids,
                Err(lumen_storage::StorageError::Interrupted) if cancel.is_cancelled() => {
                    return Err(ProviderError::Cancelled);
                }
                Err(lumen_storage::StorageError::Interrupted) => break partial,
                Err(e) => return Err(ProviderError::Unavailable(e.to_string())),
            };
            hits.retain(|h| allowed.contains(&h.chunk_id));
            // Bounded ANN overfetch prevents a few excluded nearest neighbours from
            // hiding eligible files. Very narrow filters can return a partial list.
            if hits.len() >= want || exhausted || fetch >= 1024 {
                break hits;
            }
            partial = hits;
            fetch = fetch.saturating_mul(4).min(1024);
        };
        drop(guard);
        let Some(best) = hits.first().map(|h| h.similarity) else {
            return Ok(Vec::new());
        };
        let kept: Vec<_> = hits
            .iter()
            .filter(|h| h.similarity >= best - self.config.relative_floor)
            .collect();
        let ids: Vec<i64> = kept.iter().map(|h| h.chunk_id).collect();
        let refs: HashMap<i64, ChunkRef> = store
            .chunk_refs(&ids, 4000)
            .map_err(|e| ProviderError::Unavailable(e.to_string()))?
            .into_iter()
            .map(|r| (r.chunk_id, r))
            .collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for hit in kept {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            if out.len() >= query.limit {
                break;
            }
            // Chunks deleted since the search have no ref.
            let Some(r) = refs.get(&hit.chunk_id) else {
                continue;
            };
            if !seen.insert(r.item_id) {
                continue;
            }
            let Some(item) = store
                .catalog_item_filtered(r.item_id, &syntax.filters)
                .map_err(|e| ProviderError::Unavailable(e.to_string()))?
            else {
                continue;
            };
            let score = Score::new(Confidence::saturating(hit.similarity), MatchKind::Semantic);
            if let Some(mut result) = to_result(&item, score) {
                result.provider = SEMANTIC_PROVIDER_ID;
                let excerpt: String = r.excerpt.chars().take(EXCERPT_CHARS).collect();
                if r.kind != "image" {
                    result.subtitle = Some(one_line(&excerpt));
                }
                lumen_catalog::code::enrich(&mut result, r, item.extension.as_deref());
                lumen_catalog::pdf::enrich(&mut result, r, item.extension.as_deref());
                out.push(result);
            }
        }
        Ok(out)
    }
}
