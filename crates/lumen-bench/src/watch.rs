//! Native watcher evidence, synthetic files only, never a user's catalog (T207).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use lumen_catalog::{sync_changes, sync_files};
use lumen_content::{PassConfig, run_content_pass};
use lumen_core::CancellationToken;
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_indexer::{
    ScanOptions,
    watch::{NativeWatch, Notification, Pending},
};
use lumen_storage::{GenerationSpec, Store, VectorWrite};
use serde::Serialize;

use crate::cpu::{CpuUse, cpu_time};
use crate::machine::MachineInfo;
use crate::stats::Summary;

#[derive(Serialize)]
pub(crate) struct Report {
    machine: MachineInfo,
    inventory_items: u64,
    operations: usize,
    mutation_to_lexical: Summary,
    reconciliation: Summary,
    inventories_during_mutations: usize,
    emitted_entries_during_mutations: u64,
    rename_vectors_preserved: bool,
    idle_wall_s: f64,
    idle_cpu: Option<CpuUse>,
    idle_notifications: bool,
    embedding: &'static str,
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn content(store: &mut Store) -> Result<(), String> {
    run_content_pass(
        store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &CancellationToken::new(),
        &|| 1,
        &mut |_| {},
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn seed_vectors(store: &mut Store, generation: i64) -> Result<(), String> {
    let pending = store
        .pending_chunks(generation, 0, 100)
        .map_err(|e| e.to_string())?;
    let vectors: Vec<_> = pending
        .iter()
        .map(|p| VectorWrite {
            chunk_id: p.chunk_id,
            result: Ok(&[0.0, 1.0]),
        })
        .collect();
    store
        .write_vectors(generation, &vectors, 0)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

type Wake = Arc<(Mutex<Pending>, Condvar)>;

fn next_batch(wake: &Wake, deadline: Instant) -> Result<lumen_indexer::watch::Batch, String> {
    let (lock, signal) = &**wake;
    let mut pending = lock.lock().map_err(|_| "watch lock poisoned")?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err("native notification deadline exceeded".into());
        }
        let delay = pending
            .delay(now)
            .unwrap_or(deadline - now)
            .min(deadline - now);
        if delay.is_zero() {
            return Ok(pending.take());
        }
        pending = signal
            .wait_timeout(pending, delay)
            .map_err(|_| "watch lock poisoned")?
            .0;
    }
}

pub(crate) fn run() -> Result<Report, String> {
    let temp = Temp(std::env::temp_dir().join(format!(
            "lumen-watch-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        )));
    let root = temp.0.join("files");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for i in 0..10_000 {
        fs::write(root.join(format!("inventory-{i}.bin")), []).map_err(|e| e.to_string())?;
    }
    let opts = ScanOptions {
        roots: vec![root.clone()],
        identity: true,
        ..ScanOptions::default()
    };
    let mut store = Store::open_writer(&temp.0.join("test.db")).map_err(|e| e.to_string())?;
    sync_files(&mut store, &opts, None).map_err(|e| e.to_string())?;
    let inventory_items = store
        .count_items(lumen_storage::Source::Files)
        .map_err(|e| e.to_string())?;
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: "watch-probe-state-only",
                chunker_version: EXTRACTOR_VERSION,
                dim: 2,
            },
            0,
        )
        .map_err(|e| e.to_string())?;
    let wake: Wake = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
    let callback = Arc::clone(&wake);
    let watched_root = root.clone();
    let mut watch = NativeWatch::new(move |mut event: Notification| {
        if let Ok(e) = &mut event {
            e.paths.retain(|p| p.starts_with(&watched_root));
            if e.paths.is_empty() && !e.need_rescan() {
                return;
            }
        }
        let (lock, signal) = &*callback;
        if let Ok(mut pending) = lock.lock()
            && pending.push(event, Instant::now())
        {
            signal.notify_all();
        }
    })
    .map_err(|e| e.to_string())?;
    if !watch.set_roots(std::slice::from_ref(&root)).is_empty() {
        return Err("native root registration failed".into());
    }
    let mut latency = Vec::new();
    let mut reconciliation = Vec::new();
    let mut emitted = 0;
    let mut full = 0;
    let path = root.join("focus.txt");
    let renamed = root.join("renamed.txt");
    let mut rename_vectors_preserved = true;
    for i in 0..20 {
        let before = store
            .vectors(generation, 0, 100)
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        match i % 4 {
            0 => fs::write(&path, "oldprobeword"),
            1 => fs::write(&path, "newprobeword"),
            2 => fs::rename(&path, &renamed),
            _ => fs::remove_file(&renamed),
        }
        .map_err(|e| e.to_string())?;
        loop {
            let batch = next_batch(&wake, start + Duration::from_secs(12))?;
            let sync_start = Instant::now();
            if batch.rescan {
                full += 1;
                sync_files(&mut store, &opts, None).map_err(|e| e.to_string())?;
            } else {
                let r = sync_changes(&mut store, &opts, &batch.changes, None)
                    .map_err(|e| e.to_string())?;
                emitted += r.scan.emitted();
                if !r.scan.is_complete() {
                    return Err("incremental inventory incomplete".into());
                }
            }
            content(&mut store)?;
            reconciliation.push(sync_start.elapsed().as_secs_f64() * 1000.0);
            let exists = |p: &Path| {
                store
                    .item_id_by_path(p.to_str().unwrap_or(""))
                    .map(|id| id.is_some())
                    .map_err(|e| e.to_string())
            };
            let ready = match i % 4 {
                0 | 1 => {
                    let word = if i % 4 == 0 {
                        "oldprobeword"
                    } else {
                        "newprobeword"
                    };
                    let q = lumen_storage::FtsQuery::from_user(word, false)
                        .ok_or("invalid probe query")?;
                    exists(&path)?
                        && !store
                            .search_chunks(&q, 1, &lumen_storage::SearchBudget::unbounded())
                            .map_err(|e| e.to_string())?
                            .is_empty()
                }
                2 => exists(&renamed)? && !exists(&path)?,
                _ => !exists(&renamed)?,
            };
            if ready {
                break;
            }
        }
        latency.push(start.elapsed().as_secs_f64() * 1000.0);
        if i % 4 == 2 {
            rename_vectors_preserved &= store
                .vectors(generation, 0, 100)
                .map_err(|e| e.to_string())?
                == before;
        }
        seed_vectors(&mut store, generation)?;
    }
    // Drain any trailing root-mtime hint, then measure a parked native watcher.
    std::thread::sleep(Duration::from_millis(400));
    wake.0.lock().map_err(|_| "watch lock poisoned")?.take();
    let cpu_start = cpu_time(None);
    let idle = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    let idle_wall_s = idle.elapsed().as_secs_f64();
    let idle_cpu = CpuUse::between(cpu_start, cpu_time(None), idle_wall_s);
    let idle_notifications = wake
        .0
        .lock()
        .map_err(|_| "watch lock poisoned")?
        .delay(Instant::now())
        .is_some();
    drop(watch);
    if !rename_vectors_preserved {
        return Err("native rename replaced unchanged vectors".into());
    }
    Ok(Report {
        machine: MachineInfo::collect(),
        inventory_items,
        operations: latency.len(),
        mutation_to_lexical: Summary::of(&latency).ok_or("empty latency sample")?,
        reconciliation: Summary::of(&reconciliation).ok_or("empty reconciliation sample")?,
        inventories_during_mutations: full,
        emitted_entries_during_mutations: emitted,
        rename_vectors_preserved,
        idle_wall_s,
        idle_cpu,
        idle_notifications,
        embedding: "two-dimensional synthetic vectors; preservation/queue state, no model inference",
    })
}
