//! Instant name provider over the catalog (`lumen.catalog`).
//!
//! T101 scope: exact and prefix matches on the folded name, apps first. Token-prefix,
//! fuzzy and path matching, plus real ranking signals, are T102.

use std::sync::Mutex;
use std::time::Duration;

use lumen_core::builtin::{COPY_PATH, LAUNCH, OPEN, REVEAL};
use lumen_core::{
    CancellationToken, Capability, CapabilitySet, Confidence, IconRef, LatencyClass, MatchKind,
    Payload, Provider, ProviderError, ProviderId, ProviderQuery, ResultId, ResultItem, ResultKind,
    Score,
};
use lumen_storage::{CatalogItem, ItemKind, NameHit, SearchBudget, Source, StorageError, Store};

use crate::path::decode;
use crate::text::fold;

pub const PROVIDER_ID: ProviderId = ProviderId::from_static("lumen.catalog");

/// Per-call safety net on top of the coordinator's cancellation (instant class).
const BUDGET: Duration = Duration::from_millis(25);

/// Candidates fetched per result returned (room for apps-first ordering).
const OVERFETCH: usize = 4;

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

fn confidence(hit: &NameHit, key_chars: usize) -> f32 {
    let app = hit.item.source == Source::Apps;
    if hit.exact {
        return if app { 1.0 } else { 0.95 };
    }
    #[allow(clippy::cast_precision_loss)]
    let coverage = key_chars as f32 / hit.item.name.chars().count().max(key_chars).max(1) as f32;
    0.4 + 0.5 * coverage + if app { 0.05 } else { 0.0 }
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
        let key = fold(query.text);
        if key.is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }
        let budget = SearchBudget::within(BUDGET).with_cancel(cancel.clone());
        let hits = {
            let store = self
                .store
                .lock()
                .map_err(|_| ProviderError::Unavailable("catalog lock poisoned".into()))?;
            store
                .search_names(&key, query.limit * OVERFETCH, &budget)
                .map_err(|e| match e {
                    StorageError::Interrupted => ProviderError::Cancelled,
                    other => ProviderError::Unavailable(other.to_string()),
                })?
        };
        let key_chars = key.chars().count();
        let mut scored: Vec<(f32, usize, &NameHit)> = hits
            .iter()
            .map(|h| (confidence(h, key_chars), h.item.name.chars().count(), h))
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        Ok(scored
            .into_iter()
            .take(query.limit)
            .filter_map(|(c, _, h)| {
                let kind = if h.exact {
                    MatchKind::Exact
                } else {
                    MatchKind::Prefix
                };
                to_result(&h.item, Score::new(Confidence::saturating(c), kind))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use lumen_core::{QueryId, builtin, validate_result};
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
