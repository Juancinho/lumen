//! Name/path relevance over the committed synthetic set (docs/TESTING.md §2):
//! `fixtures/search/catalog-relevance.json`. Reports MRR@10 and Recall@1/@3.

#![allow(clippy::cast_precision_loss, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use lumen_catalog::CatalogProvider;
use lumen_catalog::apps::{AppSource, DiscoveredApp, write_apps};
use lumen_catalog::sync_files;
use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId, ResultKind};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::Store;

fn fixture() -> serde_like::Fixture {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/search/catalog-relevance.json");
    serde_like::parse(&fs::read_to_string(path).unwrap())
}

/// Minimal JSON reading without a serde dependency in this crate.
mod serde_like {
    pub(crate) struct Fixture {
        pub(crate) apps: Vec<String>,
        pub(crate) files: Vec<String>,
        pub(crate) queries: Vec<(String, String)>,
    }

    fn strings_in(section: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = section;
        while let Some(start) = rest.find('"') {
            let after = &rest[start + 1..];
            let end = after.find('"').unwrap();
            out.push(after[..end].to_owned());
            rest = &after[end + 1..];
        }
        out
    }

    fn section<'a>(json: &'a str, key: &str) -> &'a str {
        let start = json.find(&format!("\"{key}\"")).unwrap();
        let open = start + json[start..].find('[').unwrap();
        let close = open + json[open..].find(']').unwrap();
        &json[open + 1..close]
    }

    pub(crate) fn parse(json: &str) -> Fixture {
        let queries = strings_in(section(json, "queries"));
        let pairs = queries
            .chunks(4) // "q", value, "expect", value
            .map(|c| (c[1].clone(), c[3].clone()))
            .collect();
        Fixture {
            apps: strings_in(section(json, "apps")),
            files: strings_in(section(json, "files")),
            queries: pairs,
        }
    }
}

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn catalog_relevance_set() {
    let fx = fixture();
    let t = Tmp(std::env::temp_dir().join(format!("catalog-relevance-{}", std::process::id())));
    let _ = fs::remove_dir_all(&t.0);
    let root = t.0.join("root");
    for f in &fx.files {
        let p = root.join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"x").unwrap();
    }
    let db = t.0.join("c.db");
    let mut store = Store::open_writer(&db).unwrap();
    sync_files(
        &mut store,
        &ScanOptions {
            roots: vec![root.clone()],
            exclusions: Exclusions::default(),
            identity: false,
        },
        None,
    )
    .unwrap();
    let apps: Vec<DiscoveredApp> = fx
        .apps
        .iter()
        .map(|name| DiscoveredApp {
            name: name.clone(),
            location: format!("shell:AppsFolder\\{name}"),
            launch_target: format!("shell:AppsFolder\\{name}"),
            source: AppSource::AppsFolder,
        })
        .collect();
    write_apps(&mut store, &apps).unwrap();
    drop(store);
    let provider = CatalogProvider::new(Store::open_reader(&db).unwrap());

    let mut rr_sum = 0.0;
    let (mut at1, mut at3) = (0, 0);
    let mut misses = Vec::new();
    for (i, (q, expect)) in fx.queries.iter().enumerate() {
        let results = provider
            .search(
                &ProviderQuery {
                    id: QueryId::new(i as u64 + 1).unwrap(),
                    text: q,
                    typing: true,
                    limit: 10,
                },
                &CancellationToken::new(),
            )
            .unwrap();
        let rank = results
            .iter()
            .position(|r| match expect.strip_prefix("app:") {
                Some(app) => r.kind == ResultKind::Application && r.title == *app,
                None => r.payload.local_path() == Some(root.join(expect).as_path()),
            });
        match rank {
            Some(r) => {
                rr_sum += 1.0 / (r as f64 + 1.0);
                at1 += usize::from(r == 0);
                at3 += usize::from(r < 3);
                if r > 0 {
                    misses.push(format!(
                        "{q:?}: expected at #{}, got {:?}",
                        r + 1,
                        results[0].title
                    ));
                }
            }
            None => misses.push(format!(
                "{q:?}: not in top 10 (top: {:?})",
                results.first().map(|r| &r.title)
            )),
        }
    }
    let n = fx.queries.len() as f64;
    let mrr = rr_sum / n;
    eprintln!(
        "relevance: {} queries, MRR@10 {mrr:.3}, R@1 {:.3}, R@3 {:.3}",
        fx.queries.len(),
        at1 as f64 / n,
        at3 as f64 / n
    );
    for m in &misses {
        eprintln!("  {m}");
    }
    assert!(fx.queries.len() >= 40);
    assert!(
        mrr >= 0.95,
        "MRR@10 {mrr:.3} below 0.95:\n{}",
        misses.join("\n")
    );
    assert!(
        at3 as f64 / n >= 0.97,
        "R@3 too low:\n{}",
        misses.join("\n")
    );
}
