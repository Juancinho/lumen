//! `lumen-bench catalog` (T101): inventory → SQLite throughput, app discovery, and instant
//! name-lookup latency through the real `CatalogProvider`.
//!
//! Privacy: the JSON report holds counts and timings only. Queries are prefixes of names
//! sampled from the catalog itself and are never written out. `--show QUERY` prints the
//! top results for one query to stderr, for the person running it.

use std::path::PathBuf;
use std::time::Instant;

use lumen_catalog::apps::{AppSourceUsed, start_menu_dirs};
use lumen_catalog::{CatalogProvider, sync_apps, sync_files};
use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::Store;
use serde::Serialize;

use crate::machine::{MachineInfo, MemorySnapshot, memory};
use crate::stats::Summary;

#[derive(Debug, Clone, Default)]
pub(crate) struct CatalogOptions {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) apps: bool,
    pub(crate) sample: usize,
    pub(crate) work_dir: Option<PathBuf>,
    pub(crate) show: Vec<String>,
    pub(crate) label: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SyncTiming {
    seconds: f64,
    entries_per_s: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct CatalogReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    roots: usize,
    entries: u64,
    complete: bool,
    blocking_issues: u64,
    first_sync: SyncTiming,
    resync: SyncTiming,
    inserted: u64,
    resync_updated: u64,
    resync_removed: u64,
    apps_discovered: u64,
    apps_source: Option<&'static str>,
    apps_folder_error: bool,
    apps_seconds: f64,
    db_mib: f64,
    queries: usize,
    /// One provider call per keystroke prefix of each sampled name.
    keystroke: Summary,
    mean_results: f64,
    /// Share of sampled names whose full-name query returns that exact item in the top 10.
    full_name_found_in_top10: f64,
    memory_after: Option<MemorySnapshot>,
}

fn secs(t: Instant) -> f64 {
    t.elapsed().as_secs_f64()
}

/// # Errors
/// Storage failures or no roots/apps requested.
pub(crate) fn run(opts: &CatalogOptions) -> Result<CatalogReport, String> {
    if opts.roots.is_empty() && !opts.apps {
        return Err("catalog needs --root DIR and/or --apps".into());
    }
    let work = opts.work_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("lumen-bench-catalog-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let db = work.join("catalog.db");
    let mut writer = Store::open_writer(&db).map_err(|e| e.to_string())?;
    let scan = ScanOptions {
        roots: opts.roots.clone(),
        exclusions: Exclusions::default(),
        identity: true,
    };

    let (entries, complete, blocking, first, inserted, resync, updated, removed) =
        if opts.roots.is_empty() {
            let zero = || SyncTiming {
                seconds: 0.0,
                entries_per_s: 0.0,
            };
            (0, true, 0, zero(), 0, zero(), 0, 0)
        } else {
            let t = Instant::now();
            let r1 = sync_files(&mut writer, &scan, None).map_err(|e| e.to_string())?;
            let s1 = secs(t);
            let t = Instant::now();
            let r2 = sync_files(&mut writer, &scan, None).map_err(|e| e.to_string())?;
            let s2 = secs(t);
            let n = r1.scan.emitted();
            #[allow(clippy::cast_precision_loss)]
            let rate = |s: f64| n as f64 / s.max(1e-9);
            (
                n,
                r1.scan.is_complete(),
                r1.scan.blocking_issues().count() as u64,
                SyncTiming {
                    seconds: s1,
                    entries_per_s: rate(s1),
                },
                r1.written.inserted,
                SyncTiming {
                    seconds: s2,
                    entries_per_s: rate(s2),
                },
                r2.written.updated,
                r2.removed,
            )
        };

    let (apps_discovered, apps_source, apps_error, apps_seconds) = if opts.apps {
        let t = Instant::now();
        let r = sync_apps(&mut writer, &start_menu_dirs()).map_err(|e| e.to_string())?;
        if let Some(err) = &r.apps_folder_error {
            eprintln!("  apps: AppsFolder not used ({err}); Start-menu shortcuts instead");
        }
        (
            r.discovered,
            r.source.map(|s| match s {
                AppSourceUsed::AppsFolder => "apps-folder",
                AppSourceUsed::StartMenuShortcuts => "start-menu-shortcuts",
            }),
            r.apps_folder_error.is_some(),
            secs(t),
        )
    } else {
        (0, None, false, 0.0)
    };
    writer.checkpoint().map_err(|e| e.to_string())?;
    #[allow(clippy::cast_precision_loss)]
    let db_mib = std::fs::metadata(&db).map_or(0, |m| m.len()) as f64 / (1024.0 * 1024.0);

    // Sample names (never written to the report).
    let sample: Vec<(i64, String)> = {
        let mut stmt = writer
            .connection()
            .prepare("SELECT id, display_name FROM items ORDER BY random() LIMIT ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([i64::try_from(opts.sample).unwrap_or(300)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.filter_map(Result::ok).collect()
    };
    drop(writer);

    let provider = CatalogProvider::new(Store::open_reader(&db).map_err(|e| e.to_string())?);
    let cancel = CancellationToken::new();
    let mut seq = 0_u64;
    let mut ask = |text: &str| {
        seq += 1;
        let q = ProviderQuery {
            id: QueryId::new(seq).unwrap_or(QueryId::MAX),
            text,
            typing: true,
            limit: 10,
        };
        provider.search(&q, &cancel)
    };

    for q in &opts.show {
        match ask(q) {
            Ok(results) => {
                eprintln!("  results for {q:?}:");
                for r in results {
                    eprintln!(
                        "    {:?} {:<40} {} {}",
                        r.kind,
                        r.title,
                        r.score.confidence,
                        r.detail.unwrap_or_default()
                    );
                }
            }
            Err(e) => eprintln!("  {q:?}: {e}"),
        }
    }

    let mut latencies = Vec::new();
    let mut results_total = 0usize;
    let mut found = 0usize;
    for (id, name) in &sample {
        let chars: Vec<char> = name.chars().collect();
        for end in 1..=chars.len().min(8) {
            let prefix: String = chars[..end].iter().collect();
            let t = Instant::now();
            let n = ask(&prefix).map(|r| r.len()).unwrap_or(0);
            latencies.push(t.elapsed().as_secs_f64() * 1000.0);
            results_total += n;
        }
        let wanted = format!("item:{id}");
        if ask(name).is_ok_and(|r| r.iter().any(|x| x.id.as_str() == wanted)) {
            found += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&work);

    #[allow(clippy::cast_precision_loss)]
    Ok(CatalogReport {
        schema_version: 1,
        kind: "catalog",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        roots: opts.roots.len(),
        entries,
        complete,
        blocking_issues: blocking,
        first_sync: first,
        resync,
        inserted,
        resync_updated: updated,
        resync_removed: removed,
        apps_discovered,
        apps_source,
        apps_folder_error: apps_error,
        apps_seconds,
        db_mib,
        queries: latencies.len(),
        keystroke: Summary::of(&latencies).ok_or("no names to query (empty catalog)")?,
        mean_results: results_total as f64 / latencies.len().max(1) as f64,
        full_name_found_in_top10: found as f64 / sample.len().max(1) as f64,
        memory_after: memory(),
    })
}

pub(crate) fn summarize(r: &CatalogReport) -> String {
    format!(
        "catalog: {} roots, {} entries (complete={}, {} blocking issues){}\n  \
         first sync {:.2} s ({:.0} entries/s, {} inserted) | resync {:.2} s ({:.0} entries/s, \
         {} updated, {} removed)\n  \
         apps: {} discovered via {} in {:.2} s\n  \
         db {:.1} MiB | keystroke lookup (n={}): p50 {:.3} p95 {:.3} max {:.3} ms, {:.1} results avg\n  \
         full name found in top 10: {:.1}%{}\n",
        r.roots,
        r.entries,
        r.complete,
        r.blocking_issues,
        r.label
            .as_deref()
            .map(|l| format!(" [{l}]"))
            .unwrap_or_default(),
        r.first_sync.seconds,
        r.first_sync.entries_per_s,
        r.inserted,
        r.resync.seconds,
        r.resync.entries_per_s,
        r.resync_updated,
        r.resync_removed,
        r.apps_discovered,
        r.apps_source.unwrap_or("-"),
        r.apps_seconds,
        r.db_mib,
        r.keystroke.n,
        r.keystroke.p50_ms,
        r.keystroke.p95_ms,
        r.keystroke.max_ms,
        r.mean_results,
        r.full_name_found_in_top10 * 100.0,
        if r.machine.build_profile == "release" {
            ""
        } else {
            "\n  WARNING: debug build - not acceptance evidence"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_catalog_run_counts_without_names() {
        let dir =
            std::env::temp_dir().join(format!("lumen-bench-catalog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root/sub")).unwrap();
        for i in 0..30 {
            std::fs::write(dir.join(format!("root/sub/secret-doc-{i}.txt")), b"x").unwrap();
        }
        let r = run(&CatalogOptions {
            roots: vec![dir.join("root")],
            sample: 20,
            work_dir: Some(dir.join("work")),
            ..CatalogOptions::default()
        })
        .unwrap();
        assert_eq!(r.entries, 32);
        assert_eq!(r.resync_updated, 32);
        assert!(r.full_name_found_in_top10 > 0.99);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("secret-doc"));
        assert!(summarize(&r).contains("keystroke lookup"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
