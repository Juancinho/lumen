use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use lumen_catalog::sync_files;
use lumen_core::CancellationToken;
use lumen_embedding::{
    Capabilities, Embedder, EmbeddingBackend, EmbeddingError, EmbeddingProfile, MockBackend,
    MockLatency, Modality, PromptFormat,
};
use lumen_extract::{EXTRACTOR_VERSION, EstimateTokens};
use lumen_indexer::{Exclusions, ScanOptions};
use lumen_storage::{GenerationSpec, Store};

use super::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lumen-content-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("files")).unwrap();
        Self(dir)
    }
    fn files(&self) -> PathBuf {
        self.0.join("files")
    }
    fn store(&self) -> Store {
        Store::open_writer(&self.0.join("lumen.db")).unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sync(store: &mut Store, root: &Path) {
    let opts = ScanOptions {
        roots: vec![root.to_path_buf()],
        exclusions: Exclusions::default(),
        identity: false,
    };
    sync_files(store, &opts, None).unwrap();
}

fn pass(store: &mut Store, cfg: &PassConfig) -> PassReport {
    run_content_pass(
        store,
        cfg,
        &EstimateTokens,
        &|_| true,
        &CancellationToken::new(),
        &|| 1,
        &mut |_| {},
    )
    .unwrap()
}

fn seeded(t: &Temp) -> Store {
    let dir = t.files();
    let prose = "La reunión con el cliente fue el martes. ".repeat(60);
    std::fs::write(dir.join("notas.md"), format!("# Reunión\n\n{prose}\n")).unwrap();
    std::fs::write(
        dir.join("http.py"),
        "def fetch(url):\n    return get(url)\n\n\ndef retry(f):\n    return f()\n",
    )
    .unwrap();
    std::fs::write(dir.join("empty.txt"), "").unwrap();
    std::fs::write(dir.join("blob.txt"), [0u8, 159, 146, 150]).unwrap();
    std::fs::write(dir.join("big.log"), "x".repeat(5000)).unwrap();
    std::fs::write(dir.join("photo.jpg"), [0xFF, 0xD8, 0xFF]).unwrap();
    let mut store = t.store();
    sync(&mut store, &dir);
    store
}

#[test]
fn content_pass_indexes_skips_and_is_incremental() {
    let t = Temp::new("pass");
    let mut store = seeded(&t);
    let cfg = PassConfig {
        max_bytes: 4096,
        batch_files: 2,
        ..PassConfig::default()
    };
    let r = pass(&mut store, &cfg);
    // md, py, empty txt indexed; binary txt and big log skipped; jpg never considered.
    assert_eq!((r.files, r.indexed, r.skipped, r.failed), (5, 3, 2, 0));
    assert!(r.chunks >= 4, "{r:?}");
    let fts = store
        .search_chunks(
            &lumen_storage::FtsQuery::from_user("reunion cliente", false).unwrap(),
            5,
            &lumen_storage::SearchBudget::unbounded(),
        )
        .unwrap();
    assert!(!fts.is_empty());
    // Offsets and symbols are stored.
    let (sym, start, end): (Option<String>, i64, i64) = store
        .connection()
        .query_row(
            "SELECT symbol_name, start_offset, end_offset FROM chunks
             WHERE chunk_kind = 'code' ORDER BY ordinal LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((sym.as_deref(), start), (Some("fetch"), 0));
    assert!(end > start);

    // Nothing changed: nothing to do.
    assert_eq!(pass(&mut store, &cfg).files, 0);
    // A changed file is re-read (after the catalog saw the change).
    std::fs::write(t.files().join("http.py"), "def other():\n    pass\n").unwrap();
    sync(&mut store, &t.files());
    let r = pass(&mut store, &cfg);
    assert_eq!((r.files, r.indexed), (1, 1));
    let symbols: Vec<String> = store
        .connection()
        .prepare("SELECT symbol_name FROM chunks WHERE chunk_kind = 'code'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(symbols, ["other"]);
    // A larger limit and a newer extractor would re-read; version is recorded.
    let v: i64 = store
        .connection()
        .query_row("SELECT max(extractor_version) FROM items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, i64::from(EXTRACTOR_VERSION));
}

#[test]
fn code_metadata_backfill_preserves_embedded_chunks() {
    let t = Temp::new("code-backfill");
    std::fs::create_dir_all(t.files().join(".git")).unwrap();
    let mut store = seeded(&t);
    let cfg = PassConfig::default();
    pass(&mut store, &cfg);
    let id = store
        .item_id_by_path(&t.files().join("http.py").display().to_string())
        .unwrap()
        .unwrap();
    let chunk_id: i64 = store
        .connection()
        .query_row(
            "SELECT id FROM chunks WHERE item_id = ?1 ORDER BY ordinal LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: "test",
                chunker_version: EXTRACTOR_VERSION,
                dim: 2,
            },
            0,
        )
        .unwrap();
    store
        .write_vectors(
            generation,
            &[lumen_storage::VectorWrite {
                chunk_id,
                result: Ok(&[0.0, 1.0]),
            }],
            0,
        )
        .unwrap();
    // Simulate an upgraded database with existing vectors but no discovered metadata.
    store.connection().execute("UPDATE items SET code_language = NULL, repository_path = NULL, code_context_path = NULL WHERE id = ?1", [id]).unwrap();
    assert_eq!(
        pass(&mut store, &cfg).files,
        0,
        "metadata backfill never re-extracts content"
    );
    let reference = store.chunk_refs(&[chunk_id], 4000).unwrap().remove(0);
    assert_eq!(reference.language.as_deref(), Some("python"));
    assert_eq!(reference.repository, Some(t.files().display().to_string()));
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 1);
    assert_eq!(store.vectors(generation, 0, 10).unwrap()[0].0, chunk_id);
}

#[test]
fn content_pass_respects_scope() {
    let t = Temp::new("scope");
    let mut store = seeded(&t);
    let r = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|path: &str| path.ends_with(".py"),
        &CancellationToken::new(),
        &|| 1,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!((r.files, r.indexed), (1, 1));
    // Out-of-scope files stay unprocessed (picked up once their location gets content).
    assert_eq!(pass(&mut store, &PassConfig::default()).files, 4);
}

#[test]
fn content_pass_stops_on_cancel() {
    let t = Temp::new("cancel");
    let mut store = seeded(&t);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| true,
        &cancel,
        &|| 1,
        &mut |_| {},
    )
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.indexed, 0);
}

fn hints(paths: &[PathBuf], content: bool) -> Vec<lumen_indexer::watch::Change> {
    paths
        .iter()
        .map(|p| lumen_indexer::watch::Change {
            path: p.clone(),
            recursive: true,
            content,
            renamed_from: false,
        })
        .collect()
}

fn incremental_opts(root: &Path) -> ScanOptions {
    ScanOptions {
        roots: vec![root.to_path_buf()],
        identity: true,
        exclusions: Exclusions::default(),
    }
}

fn vectors_for_all_chunks(store: &mut Store) -> i64 {
    let generation = store
        .ensure_generation(
            GenerationSpec {
                space_key: "incremental-test",
                chunker_version: EXTRACTOR_VERSION,
                dim: 2,
            },
            0,
        )
        .unwrap();
    let pending = store.pending_chunks(generation, 0, 100).unwrap();
    let vectors: Vec<_> = pending
        .iter()
        .map(|p| lumen_storage::VectorWrite {
            chunk_id: p.chunk_id,
            result: Ok(&[0.0, 1.0]),
        })
        .collect();
    store.write_vectors(generation, &vectors, 0).unwrap();
    generation
}

#[test]
fn incremental_folder_moves_preserve_vectors_and_unrelated_roots() {
    let t = Temp::new("incremental-move");
    let old = t.files().join("repository");
    std::fs::create_dir_all(old.join(".git")).unwrap();
    std::fs::write(
        old.join("parser.rs"),
        "fn parse_input() { println!(\"originaltoken\"); }\n",
    )
    .unwrap();
    let other = t.0.join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("keep.txt"), "unrelatedtoken").unwrap();
    let mut opts = incremental_opts(&t.files());
    opts.roots.push(other.clone());
    let mut store = t.store();
    sync_files(&mut store, &opts, None).unwrap();
    pass(&mut store, &PassConfig::default());
    let old_id = store
        .item_id_by_path(&old.join("parser.rs").display().to_string())
        .unwrap()
        .unwrap();
    let generation = vectors_for_all_chunks(&mut store);
    let before = store.vectors(generation, 0, 100).unwrap();
    let new = t.files().join("Repositorio nuevo");
    std::fs::rename(&old, &new).unwrap();
    let report =
        lumen_catalog::sync_changes(&mut store, &opts, &hints(&[old, new.clone()], false), None)
            .unwrap();
    assert!(report.written.moved >= 2);
    assert_eq!(report.removed, 0);
    assert_eq!(
        store
            .item_id_by_path(&new.join("parser.rs").display().to_string())
            .unwrap(),
        Some(old_id)
    );
    assert!(
        store
            .item_id_by_path(&other.join("keep.txt").display().to_string())
            .unwrap()
            .is_some()
    );
    assert_eq!(pass(&mut store, &PassConfig::default()).files, 0);
    assert_eq!(store.vectors(generation, 0, 100).unwrap(), before);
    let reference = store
        .chunk_refs(
            &[before
                .iter()
                .find(|(id, _)| store.chunk_refs(&[*id], 100).unwrap()[0].item_id == old_id)
                .unwrap()
                .0],
            100,
        )
        .unwrap();
    assert_eq!(
        reference[0].repository.as_deref(),
        Some(new.to_str().unwrap())
    );
}

#[test]
fn notified_same_size_same_mtime_write_removes_stale_content_before_extraction() {
    let t = Temp::new("incremental-write");
    let path = t.files().join("note.txt");
    std::fs::write(&path, "oldsecretword").unwrap();
    let opts = incremental_opts(&t.files());
    let mut store = t.store();
    sync_files(&mut store, &opts, None).unwrap();
    pass(&mut store, &PassConfig::default());
    let generation = vectors_for_all_chunks(&mut store);
    let old_chunk = store.vectors(generation, 0, 10).unwrap()[0].0;
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, "newsecretword").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(mtime))
        .unwrap();
    let report = lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(std::slice::from_ref(&path), true),
        None,
    )
    .unwrap();
    assert_eq!(report.written.updated, 1);
    assert!(store.chunk_refs(&[old_chunk], 100).unwrap().is_empty());
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
    let fts = |word| {
        store
            .search_chunks(
                &lumen_storage::FtsQuery::from_user(word, false).unwrap(),
                5,
                &lumen_storage::SearchBudget::unbounded(),
            )
            .unwrap()
    };
    assert!(fts("oldsecretword").is_empty());
    assert_eq!(fts("newsecretword").len(), 1);
    assert_eq!(store.queue_counts(generation).unwrap().pending(), 1);
    std::fs::remove_file(&path).unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[path], false), None).unwrap();
    assert_eq!(store.queue_counts(generation).unwrap().chunks, 0);
}

#[test]
fn atomic_replacement_invalidates_identity_even_when_metadata_matches() {
    let t = Temp::new("incremental-replace");
    let path = t.files().join("note.txt");
    std::fs::write(&path, "oldsecretword").unwrap();
    let opts = incremental_opts(&t.files());
    let mut store = t.store();
    sync_files(&mut store, &opts, None).unwrap();
    pass(&mut store, &PassConfig::default());
    let generation = vectors_for_all_chunks(&mut store);
    let old_id = store.item_id_by_path(path.to_str().unwrap()).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let temp = t.files().join("save.tmp");
    std::fs::write(&temp, "newsecretword").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&temp)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(mtime))
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&temp, &path).unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(std::slice::from_ref(&path), false),
        None,
    )
    .unwrap();
    assert_eq!(
        store.item_id_by_path(path.to_str().unwrap()).unwrap(),
        old_id
    );
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
}

#[test]
fn ambiguous_rename_write_checks_content_and_respects_names_only_scope() {
    let t = Temp::new("ambiguous-rename");
    let old = t.files().join("old.txt");
    let new = t.files().join("new.txt");
    let third = t.files().join("third.txt");
    std::fs::write(&old, "oldsecretword").unwrap();
    let opts = incremental_opts(&t.files());
    let mut store = t.store();
    sync_files(&mut store, &opts, None).unwrap();
    pass(&mut store, &PassConfig::default());
    let generation = vectors_for_all_chunks(&mut store);
    let before = store.vectors(generation, 0, 10).unwrap();
    std::fs::rename(&old, &new).unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[old, new.clone()], true), None)
        .unwrap();
    assert_eq!(store.vectors(generation, 0, 10).unwrap(), before);
    assert_eq!(pass(&mut store, &PassConfig::default()).files, 0);
    let mtime = std::fs::metadata(&new).unwrap().modified().unwrap();
    std::fs::rename(&new, &third).unwrap();
    std::fs::write(&third, "newsecretword").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&third)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(mtime))
        .unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(&[new.clone(), third.clone()], true),
        None,
    )
    .unwrap();
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
    vectors_for_all_chunks(&mut store);
    std::fs::rename(&third, &new).unwrap();
    // Existing chunks must not authorize a body read after content consent is disabled.
    lumen_catalog::sync_changes_with_content_scope(
        &mut store,
        &opts,
        &hints(&[third, new], true),
        None,
        &|_| false,
    )
    .unwrap();
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 0);
}

#[test]
fn incremental_hardlinks_do_not_steal_a_surviving_path() {
    let t = Temp::new("incremental-hardlink");
    let old = t.files().join("old.txt");
    let new = t.files().join("new.txt");
    std::fs::write(&old, "preservedword").unwrap();
    let opts = incremental_opts(&t.files());
    let mut store = t.store();
    sync_files(&mut store, &opts, None).unwrap();
    pass(&mut store, &PassConfig::default());
    let generation = vectors_for_all_chunks(&mut store);
    let old_id = store
        .item_id_by_path(old.to_str().unwrap())
        .unwrap()
        .unwrap();
    std::fs::hard_link(&old, &new).unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(std::slice::from_ref(&new), false),
        None,
    )
    .unwrap();
    assert_eq!(
        store.item_id_by_path(old.to_str().unwrap()).unwrap(),
        Some(old_id)
    );
    assert_ne!(
        store.item_id_by_path(new.to_str().unwrap()).unwrap(),
        Some(old_id)
    );
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 1);
    pass(&mut store, &PassConfig::default());
    vectors_for_all_chunks(&mut store);
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 2);
    std::fs::write(&old, "updatedword").unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[old], true), None).unwrap();
    assert_eq!(store.queue_counts(generation).unwrap().embedded, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 2);
}

fn embedder(backend: Arc<dyn EmbeddingBackend>) -> Embedder {
    Embedder::new(backend, EmbeddingProfile::DEFAULT).unwrap()
}

fn generation(store: &Store, e: &Embedder) -> i64 {
    store
        .ensure_generation(
            GenerationSpec {
                space_key: &e.space().key(),
                chunker_version: EXTRACTOR_VERSION,
                dim: e.profile().dim,
            },
            1,
        )
        .unwrap()
}

fn queue(
    store: &mut Store,
    e: &Embedder,
    g: i64,
    control: &Control,
    cfg: &QueueConfig,
) -> Result<QueueReport, QueueError> {
    run_queue(
        store,
        &QueueJob {
            embedder: e,
            generation: g,
            control,
            cancel: &CancellationToken::new(),
            cfg: *cfg,
        },
        &|| 2,
        &mut |_| {},
    )
}

#[test]
fn queue_drains_persists_and_resumes() {
    let t = Temp::new("queue");
    let mut store = seeded(&t);
    pass(&mut store, &PassConfig::default());
    let e = embedder(Arc::new(MockBackend::new()));
    let g = generation(&store, &e);
    let total = store.queue_counts(g).unwrap().chunks;
    assert!(total >= 4);

    // Time slice of zero: returns immediately with everything still pending.
    let cfg = QueueConfig {
        batch: 3,
        max_run: Duration::ZERO,
    };
    let r = queue(&mut store, &e, g, &Control::new(), &cfg).unwrap();
    assert_eq!((r.stop, r.embedded), (Stop::TimeSlice, 0));

    let cfg = QueueConfig {
        batch: 3,
        ..QueueConfig::default()
    };
    let r = queue(&mut store, &e, g, &Control::new(), &cfg).unwrap();
    assert_eq!(r.stop, Stop::Drained);
    assert_eq!(r.embedded, total);
    assert_eq!(r.batches, total.div_ceil(3));
    let counts = store.queue_counts(g).unwrap();
    assert_eq!((counts.embedded, counts.pending()), (total, 0));
    // Stored vectors are what the embedder produced for "title | text" (document prompt).
    let (chunk_id, stored) = store.vectors(g, 0, 1).unwrap().remove(0);
    let (text, title): (String, String) = store
        .connection()
        .query_row(
            "SELECT c.text, i.display_name FROM chunks c JOIN items i ON i.id = c.item_id
             WHERE c.id = ?1",
            [chunk_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let direct = e
        .embed(
            lumen_embedding::EmbeddingTask::SearchDocument,
            &[lumen_embedding::TextInput::with_title(&text, &title)],
            None,
        )
        .unwrap()
        .into_flat();
    let cos: f32 = stored.iter().zip(&direct).map(|(a, b)| a * b).sum();
    assert!(cos > 0.999, "{cos}");

    // Reopen (restart): nothing left; a new file adds exactly its chunks.
    drop(store);
    let mut store = t.store();
    let r = queue(&mut store, &e, g, &Control::new(), &cfg).unwrap();
    assert_eq!((r.stop, r.embedded), (Stop::Drained, 0));
    std::fs::write(
        t.files().join("new.md"),
        "Contenido nuevo del proyecto lumen.",
    )
    .unwrap();
    sync(&mut store, &t.files());
    pass(&mut store, &PassConfig::default());
    let r = queue(&mut store, &e, g, &Control::new(), &cfg).unwrap();
    assert_eq!((r.stop, r.embedded), (Stop::Drained, 1));
}

#[test]
fn pause_cancel_and_hold() {
    let t = Temp::new("control");
    let mut store = seeded(&t);
    pass(&mut store, &PassConfig::default());
    let e = embedder(Arc::new(MockBackend::new()));
    let g = generation(&store, &e);
    let cfg = QueueConfig::default();

    let control = Control::new();
    control.pause();
    let r = queue(&mut store, &e, g, &control, &cfg).unwrap();
    assert_eq!((r.stop, r.embedded), (Stop::Paused, 0));
    assert!(!control.wait_until_runnable(&CancellationToken::new(), Duration::from_millis(20)));
    control.resume();

    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = run_queue(
        &mut store,
        &QueueJob {
            embedder: &e,
            generation: g,
            control: &control,
            cancel: &cancel,
            cfg,
        },
        &|| 1,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.stop, Stop::Cancelled);

    // An interactive hold delays the queue until released.
    let hold = control.hold();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        drop(hold);
    });
    let r = queue(&mut store, &e, g, &control, &cfg).unwrap();
    releaser.join().unwrap();
    assert_eq!(r.stop, Stop::Drained);
    assert!(r.yielded >= Duration::from_millis(100), "{r:?}");
    // Right after interactive use the queue embeds one chunk per batch.
    assert!(control.interactive_within(Duration::from_secs(10)));
    assert_eq!(r.batches, r.embedded + r.failed, "{r:?}");
    let fresh = Control::new();
    assert!(!fresh.interactive_within(Duration::from_secs(10)));
    fresh.mark_interactive();
    assert!(fresh.interactive_within(Duration::from_secs(10)));
}

#[test]
fn duty_cycle_caps_the_busy_share() {
    let t = Temp::new("duty");
    let mut store = seeded(&t);
    pass(&mut store, &PassConfig::default());
    let e = embedder(Arc::new(MockBackend::with_latency(MockLatency {
        per_call: Duration::from_millis(15),
        ..MockLatency::default()
    })));
    let g = generation(&store, &e);
    let control = Control::new();
    control.set_duty(0.5);
    let cfg = QueueConfig {
        batch: 1,
        ..QueueConfig::default()
    };
    let r = queue(&mut store, &e, g, &control, &cfg).unwrap();
    assert_eq!(r.stop, Stop::Drained);
    let share = r.busy.as_secs_f64() / r.elapsed.as_secs_f64();
    assert!(
        (0.3..0.62).contains(&share),
        "busy share {share:.2} ({r:?})"
    );
    control.set_duty(f64::NAN);
    assert!((control.duty() - 1.0).abs() < f64::EPSILON);
}

/// Mock wrapper: inputs containing `poison` fail with a non-device error; after `lose`
/// calls the device is lost.
struct Flaky {
    inner: MockBackend,
    calls: AtomicUsize,
    lose_after: usize,
}

impl EmbeddingBackend for Flaky {
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }
    fn warm(&self, m: Modality) -> Result<(), EmbeddingError> {
        self.inner.warm(m)
    }
    fn unload(&self, m: Modality) -> Result<(), EmbeddingError> {
        self.inner.unload(m)
    }
    fn is_warm(&self, m: Modality) -> bool {
        self.inner.is_warm(m)
    }
    fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= self.lose_after {
            return Err(EmbeddingError::Backend("device removed".into()));
        }
        if inputs.iter().any(|i| i.contains("poison")) {
            return Err(EmbeddingError::Unsupported(Modality::Text));
        }
        self.inner.embed_text(inputs)
    }
}

#[test]
fn bad_inputs_are_isolated_and_device_failures_abort() {
    let t = Temp::new("flaky");
    let dir = t.files();
    std::fs::write(dir.join("a.txt"), "alpha text").unwrap();
    std::fs::write(dir.join("b.txt"), "poison text").unwrap();
    std::fs::write(dir.join("c.txt"), "gamma text").unwrap();
    let mut store = t.store();
    sync(&mut store, &dir);
    pass(&mut store, &PassConfig::default());

    let flaky = Arc::new(Flaky {
        inner: MockBackend::new(),
        calls: AtomicUsize::new(0),
        lose_after: usize::MAX,
    });
    let e = embedder(flaky);
    let g = generation(&store, &e);
    let r = queue(&mut store, &e, g, &Control::new(), &QueueConfig::default()).unwrap();
    assert_eq!((r.embedded, r.failed, r.stop), (2, 1, Stop::Drained));
    let code: String = store
        .connection()
        .query_row(
            "SELECT error_code FROM chunk_vectors WHERE vector IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(code, "unsupported");

    // A lost device writes nothing and reports a device error.
    let lost = Arc::new(Flaky {
        inner: MockBackend::new(),
        calls: AtomicUsize::new(0),
        lose_after: 0,
    });
    let e2 = Embedder::new(
        lost,
        EmbeddingProfile {
            dim: 128,
            prompts: PromptFormat::RAW,
        },
    )
    .unwrap();
    let g2 = generation(&store, &e2);
    assert_ne!(g, g2);
    let err = queue(
        &mut store,
        &e2,
        g2,
        &Control::new(),
        &QueueConfig::default(),
    )
    .unwrap_err();
    assert!(matches!(err, QueueError::Device(_)), "{err}");
    assert_eq!(store.queue_counts(g2).unwrap().embedded, 0);
    assert_eq!(store.queue_counts(g2).unwrap().pending(), 3);
    let old_vectors = store.vectors(g, 0, 100).unwrap();
    let fallback = Embedder::new(
        Arc::new(MockBackend::new()),
        EmbeddingProfile {
            dim: 128,
            prompts: PromptFormat::RAW,
        },
    )
    .unwrap();
    assert_eq!(generation(&store, &fallback), g2);
    let recovered = queue(
        &mut store,
        &fallback,
        g2,
        &Control::new(),
        &QueueConfig::default(),
    )
    .unwrap();
    assert_eq!(
        (recovered.embedded, recovered.failed, recovered.stop),
        (3, 0, Stop::Drained)
    );
    assert_eq!(store.queue_counts(g2).unwrap().pending(), 0);
    assert_eq!(store.vectors(g, 0, 100).unwrap(), old_vectors);
}
