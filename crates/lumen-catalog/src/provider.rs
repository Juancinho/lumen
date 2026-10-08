//! Instant name provider over the catalog (`lumen.catalog`).
//!
//! Candidates come from three bounded index lookups (see `gather`); [`crate::rank`] decides
//! the order. T101 built the provider, T102 the matching and ranking.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lumen_core::builtin::{COPY_PATH, LAUNCH, OPEN, REVEAL};
use lumen_core::{
    CancellationToken, Capability, CapabilitySet, Confidence, IconRef, LatencyClass, Payload,
    Provider, ProviderError, ProviderId, ProviderQuery, ResultId, ResultItem, ResultKind, Score,
};
use lumen_storage::{CatalogItem, ItemKind, SearchBudget, StorageError, Store};

use crate::path::decode;
use crate::rank::{ParsedQuery, Scored, score, typo_budget};

pub const PROVIDER_ID: ProviderId = ProviderId::from_static("lumen.catalog");

/// Per-call safety net on top of the coordinator's cancellation (instant class).
const BUDGET: Duration = Duration::from_millis(25);

/// Name-prefix candidates fetched per result returned.
const OVERFETCH: usize = 4;
/// Token-prefix (FTS) candidates, best bm25 first.
const TOKEN_CANDIDATES: usize = 300;
/// Typo candidates (same first two characters), in key order.
const FUZZY_CANDIDATES: usize = 1_000;
/// Time slices of the best-effort stages (see `gather`).
const TOKEN_SLICE: Duration = Duration::from_millis(8);
const FUZZY_SLICE: Duration = Duration::from_millis(4);

pub struct CatalogProvider {
    id: ProviderId,
    store: Mutex<Store>,
}

impl CatalogProvider {
    /// `store` should be a reader ([`Store::open_reader`]) dedicated to this provider.
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self {
            id: PROVIDER_ID,
            store: Mutex::new(store),
        }
    }
}

/// Builds the universal result for one catalog item.
#[must_use]
pub fn to_result(item: &CatalogItem, score: Score) -> Option<ResultItem> {
    let id = ResultId::from_parts("item", &item.id.to_string()).ok()?;
    let local = CapabilitySet::of(&[Capability::LocalPath, Capability::Pinnable]);
    let parent = |p: &str| {
        std::path::Path::new(p)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .filter(|d| !d.is_empty())
    };
    let result = match item.kind {
        ItemKind::Application => {
            let target = item
                .launch_target
                .clone()
                .unwrap_or_else(|| item.path.clone());
            let shortcut = !target.starts_with("shell:");
            ResultItem {
                id,
                provider: PROVIDER_ID,
                kind: ResultKind::Application,
                title: item.name.clone(),
                subtitle: None,
                detail: Some("Application".into()),
                icon: IconRef::Native,
                score,
                capabilities: if shortcut {
                    local.with(Capability::Launchable)
                } else {
                    CapabilitySet::of(&[Capability::Launchable, Capability::Pinnable])
                },
                primary_action: LAUNCH,
                secondary_actions: if shortcut {
                    vec![REVEAL, COPY_PATH]
                } else {
                    Vec::new()
                },
                payload: if shortcut {
                    Payload::Path(decode(&item.path, item.raw_path.as_deref()))
                } else {
                    Payload::ProviderKey(target.into())
                },
            }
        }
        ItemKind::File | ItemKind::Folder => ResultItem {
            id,
            provider: PROVIDER_ID,
            kind: if item.kind == ItemKind::Folder {
                ResultKind::Folder
            } else {
                ResultKind::File
            },
            title: item.name.clone(),
            subtitle: None,
            detail: parent(&item.path),
            icon: match &item.extension {
                Some(ext) if item.kind == ItemKind::File => {
                    IconRef::FileExtension(ext.as_str().into())
                }
                _ => IconRef::KindDefault,
            },
            score,
            capabilities: local,
            primary_action: OPEN,
            secondary_actions: vec![REVEAL, COPY_PATH],
            payload: Payload::Path(decode(&item.path, item.raw_path.as_deref())),
        },
    };
    Some(result)
}

impl Provider for CatalogProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn latency_class(&self) -> LatencyClass {
        LatencyClass::Instant
    }

    fn search(
        &self,
        query: &ProviderQuery<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<ResultItem>, ProviderError> {
        let Some(q) = ParsedQuery::parse(query.text) else {
            return Ok(Vec::new());
        };
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let budget = SearchBudget::within(BUDGET).with_cancel(cancel.clone());
        let candidates = {
            let store = self
                .store
                .lock()
                .map_err(|_| ProviderError::Unavailable("catalog lock poisoned".into()))?;
            gather(&store, &q, query.limit, &budget)?
        };
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let now = now_ms();
        let mut scored: Vec<(Scored, &CatalogItem)> = candidates
            .values()
            .filter_map(|item| score(&q, item, now).map(|s| (s, item)))
            .collect();
        scored.sort_by(|a, b| {
            b.0.rank
                .total_cmp(&a.0.rank)
                .then_with(|| a.1.name.chars().count().cmp(&b.1.name.chars().count()))
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        Ok(scored
            .into_iter()
            .take(query.limit)
            .filter_map(|(s, item)| {
                to_result(item, Score::new(Confidence::saturating(s.base), s.kind))
            })
            .collect())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Candidate items for `q`, deduplicated by id:
/// 1. exact + prefix on the whole name key (index range, always);
/// 2. token-prefix over names and parent folders (FTS5, tokens of 3+ characters; skipped if
///    the budget runs out — step 1 already has the strongest matches);
/// 3. typo candidates sharing the first two characters, only while results are scarce.
fn gather(
    store: &Store,
    q: &ParsedQuery,
    limit: usize,
    budget: &SearchBudget,
) -> Result<HashMap<i64, CatalogItem>, ProviderError> {
    let interrupted = |e: StorageError| match e {
        StorageError::Interrupted => ProviderError::Cancelled,
        other => ProviderError::Unavailable(other.to_string()),
    };
    let mut out: HashMap<i64, CatalogItem> = HashMap::new();
    for hit in store
        .search_names(&q.key, limit * OVERFETCH, budget)
        .map_err(interrupted)?
    {
        out.insert(hit.item.id, hit.item);
    }
    // Stages 2 and 3 are best-effort: each gets its own short slice of time so a pathological
    // corpus (100k `lib*` files) degrades to stage-1 results instead of a slow keystroke.
    let slice = |limit: Duration| {
        let deadline = Instant::now() + limit;
        SearchBudget {
            deadline: Some(budget.deadline.map_or(deadline, |d| d.min(deadline))),
            cancel: budget.cancel.clone(),
        }
    };
    if let Some(matcher) = q.fts_matcher() {
        match store.search_name_tokens(&matcher, TOKEN_CANDIDATES, &slice(TOKEN_SLICE)) {
            Ok(items) => {
                for item in items {
                    out.entry(item.id).or_insert(item);
                }
            }
            Err(StorageError::Interrupted) if !budget.is_cancelled() => {}
            Err(e) => return Err(interrupted(e)),
        }
    }
    if out.len() < limit && typo_budget(q.chars()) > 0 {
        let lo: String = q.key.chars().take(2).collect();
        let hi = format!("{lo}\u{10FFFF}");
        match store.name_key_range(&lo, &hi, FUZZY_CANDIDATES, &slice(FUZZY_SLICE)) {
            Ok(items) => {
                for item in items {
                    out.entry(item.id).or_insert(item);
                }
            }
            Err(StorageError::Interrupted) if !budget.is_cancelled() => {}
            Err(e) => return Err(interrupted(e)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use lumen_core::{MatchKind, QueryId, builtin, validate_result};
    use lumen_indexer::{Exclusions, ScanOptions};

    use super::*;
    use crate::apps::{AppSource, DiscoveredApp, write_apps};
    use crate::files::sync_files;

    struct Tmp(PathBuf);

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn setup(tag: &str) -> (Tmp, CatalogProvider) {
        let t = Tmp(std::env::temp_dir().join(format!(
            "lumen-catalog-provider-{tag}-{}",
            std::process::id()
        )));
        let _ = fs::remove_dir_all(&t.0);
        let root = t.0.join("root");
        fs::create_dir_all(root.join("Spotify exports")).unwrap();
        fs::write(root.join("Spotify exports/playlist.csv"), b"x").unwrap();
        fs::write(root.join("Presupuesto Reunión.xlsx"), b"x").unwrap();
        let db = t.0.join("c.db");
        let mut writer = Store::open_writer(&db).unwrap();
        sync_files(
            &mut writer,
            &ScanOptions {
                roots: vec![root],
                exclusions: Exclusions::default(),
                identity: false,
            },
            None,
        )
        .unwrap();
        let app = |name: &str, uri: &str| DiscoveredApp {
            name: name.into(),
            location: uri.into(),
            launch_target: uri.into(),
            source: AppSource::AppsFolder,
        };
        write_apps(
            &mut writer,
            &[
                app(
                    "Spotify",
                    "shell:AppsFolder\\SpotifyAB.SpotifyMusic!Spotify",
                ),
                app(
                    "Calculator",
                    "shell:AppsFolder\\Microsoft.WindowsCalculator!App",
                ),
            ],
        )
        .unwrap();
        drop(writer);
        let provider = CatalogProvider::new(Store::open_reader(&db).unwrap());
        (t, provider)
    }

    fn query(text: &str) -> ProviderQuery<'_> {
        ProviderQuery {
            id: QueryId::new(1).unwrap(),
            text,
            typing: true,
            limit: 10,
        }
    }

    #[test]
    fn apps_rank_first_and_every_result_is_valid() {
        let (_t, p) = setup("rank");
        let results = p.search(&query("spot"), &CancellationToken::new()).unwrap();
        assert_eq!(results[0].title, "Spotify");
        assert_eq!(results[0].kind, ResultKind::Application);
        assert_eq!(results[0].primary_action, LAUNCH);
        assert!(matches!(results[0].payload, Payload::ProviderKey(_)));
        assert!(results.iter().any(|r| r.kind == ResultKind::Folder));
        for r in &results {
            assert!(
                validate_result(r, &builtin::DESCRIPTORS).is_empty(),
                "{r:?}"
            );
        }
        let exact = p
            .search(&query("Spotify"), &CancellationToken::new())
            .unwrap();
        assert_eq!(exact[0].score.match_kind, MatchKind::Exact);
    }

    #[test]
    fn accents_and_case_are_ignored_and_files_open() {
        let (_t, p) = setup("fold");
        let results = p
            .search(&query("PRESUPUESTO reu"), &CancellationToken::new())
            .unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.title, "Presupuesto Reunión.xlsx");
        assert_eq!(r.primary_action, OPEN);
        assert_eq!(r.icon, IconRef::FileExtension("xlsx".into()));
        assert!(matches!(&r.payload, Payload::Path(p) if p.ends_with("Presupuesto Reunión.xlsx")));
        assert!(r.detail.is_some());
        assert_eq!(r.id.as_str().split(':').next(), Some("item"));
    }

    #[test]
    fn empty_query_and_cancellation() {
        let (_t, p) = setup("cancel");
        assert!(
            p.search(&query("   "), &CancellationToken::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(p.id(), &PROVIDER_ID);
        assert_eq!(p.latency_class(), LatencyClass::Instant);
    }
}
