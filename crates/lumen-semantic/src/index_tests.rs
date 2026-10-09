use std::path::PathBuf;

use lumen_core::CancellationToken;
use lumen_storage::content::{GenerationSpec, VectorWrite};
use lumen_storage::{NewChunk, NewItem, Store};

use crate::index::*;

const DIM: usize = 8;

struct Fixture {
    dir: PathBuf,
    store: Store,
    generation: i64,
    item: i64,
    next_ordinal: i64,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lumen-semantic-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_writer(&dir.join("lumen.db")).unwrap();
        let generation = store
            .ensure_generation(
                GenerationSpec {
                    space_key: "test",
                    chunker_version: 1,
                    dim: DIM,
                },
                0,
            )
            .unwrap();
        let item = store.insert_item(&NewItem::file("/a.md", "a.md")).unwrap();
        Self {
            dir,
            store,
            generation,
            item,
            next_ordinal: 0,
        }
    }

    fn vectors_dir(&self) -> PathBuf {
        self.dir.join("vectors")
    }

    /// Inserts chunks and embeds chunk `i` as `unit(seeds[i])`.
    fn add(&mut self, seeds: &[usize]) -> Vec<i64> {
        let first = self.next_ordinal;
        self.next_ordinal += i64::try_from(seeds.len()).unwrap();
        let chunks: Vec<NewChunk<'_>> = (0..seeds.len())
            .map(|i| NewChunk {
                item_id: self.item,
                ordinal: first + i64::try_from(i).unwrap(),
                chunk_kind: "text",
                text: "t",
                symbol_name: None,
                page_number: None,
                start_offset: None,
                end_offset: None,
            })
            .collect();
        let ids = self.store.insert_chunks(&chunks).unwrap();
        for (&id, &seed) in ids.iter().zip(seeds) {
            self.embed(id, seed);
        }
        ids
    }

    fn embed(&mut self, chunk_id: i64, seed: usize) {
        let v = unit(seed);
        self.store
            .write_vectors(
                self.generation,
                &[VectorWrite {
                    chunk_id,
                    result: Ok(&v),
                }],
                0,
            )
            .unwrap();
    }

    fn build(&self) {
        let record = build_file(
            &self.store,
            &self.vectors_dir(),
            self.generation,
            &CancellationToken::new(),
            1,
        )
        .unwrap();
        self.store.set_ann_file(&record).unwrap();
    }

    fn open(&self, settings: IndexSettings) -> SemanticIndex {
        let g = self
            .store
            .generations()
            .unwrap()
            .into_iter()
            .find(|g| g.id == self.generation)
            .unwrap();
        SemanticIndex::open(&self.store, &self.vectors_dir(), g, settings).unwrap()
    }

    fn top(&self, index: &SemanticIndex, seed: usize, k: usize) -> Vec<i64> {
        index
            .search(&self.store, &unit(seed), k)
            .unwrap()
            .into_iter()
            .map(|h| h.chunk_id)
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Distinct normalized directions: a strong component on axis `seed % DIM`, a smaller one
/// on `(seed / DIM) % DIM`.
fn unit(seed: usize) -> Vec<f32> {
    let mut v = [0.02_f32; DIM];
    v[seed % DIM] += 1.0;
    v[(seed / DIM + 1) % DIM] += 0.3 + 0.05 * f32::from(u8::try_from(seed % 7).unwrap());
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

#[test]
fn file_plus_delta_finds_current_vectors_only() {
    let mut f = Fixture::new("file-delta");
    let ids = f.add(&[0, 1, 2, 3, 4, 5]);
    f.build();
    let mut index = f.open(IndexSettings::default());
    assert_eq!(index.status().file, FileState::Current);
    assert_eq!(index.status().file_vectors, 6);
    assert_eq!(f.top(&index, 3, 1), [ids[3]]);

    // A new chunk after the build is found through the delta.
    let late = f.add(&[6]);
    index.refresh(&f.store).unwrap();
    assert_eq!(index.status().delta_vectors, 1);
    assert_eq!(f.top(&index, 6, 1), late);

    // Re-embedding chunk 3 with another vector: the file's old vector no longer matches it.
    f.embed(ids[3], 6 + DIM);
    index.refresh(&f.store).unwrap();
    let near_old = f.top(&index, 3, 1);
    assert_ne!(near_old, [ids[3]], "stale file vector must not be returned");
    assert!(f.top(&index, 6 + DIM, 2).contains(&ids[3]));

    // A deleted chunk disappears even though the file still has it.
    f.store
        .connection()
        .execute("DELETE FROM chunks WHERE id = ?1", [ids[0]])
        .unwrap();
    assert!(!f.top(&index, 0, 3).contains(&ids[0]));
    let hits = index.search(&f.store, &unit(1), 10).unwrap();
    assert!(hits.windows(2).all(|w| w[0].similarity >= w[1].similarity));
    assert_eq!(
        hits.len(),
        6,
        "5 file rows left (one rewritten) + the late chunk"
    );
}

#[test]
fn a_reused_chunk_id_never_returns_the_old_vector() {
    let mut f = Fixture::new("reuse");
    let ids = f.add(&[0, 1, 2]);
    f.build();
    // SQLite reuses the highest rowid after it is deleted.
    f.store
        .connection()
        .execute("DELETE FROM chunks WHERE id = ?1", [ids[2]])
        .unwrap();
    let reused = f.add(&[5]);
    assert_eq!(reused, [ids[2]], "rowid reused");
    let index = f.open(IndexSettings::default());
    assert_eq!(f.top(&index, 5, 1), reused);
    let near_two = index.search(&f.store, &unit(2), 3).unwrap();
    let hit = near_two.iter().find(|h| h.chunk_id == ids[2]).unwrap();
    let expected: f32 = unit(2).iter().zip(unit(5)).map(|(a, b)| a * b).sum();
    assert!(
        (hit.similarity - expected).abs() < 1e-3,
        "scored with the new vector"
    );
}

#[test]
fn missing_or_mismatching_files_degrade_to_rebuild() {
    let mut f = Fixture::new("degrade");
    let ids = f.add(&[0, 1, 2, 3]);
    let settings = IndexSettings {
        rebuild_delta_min: 2,
        ..IndexSettings::default()
    };
    // No file yet: the delta serves everything; a rebuild is due.
    let index = f.open(settings);
    assert_eq!(index.status().file, FileState::Missing);
    assert_eq!(f.top(&index, 2, 1), [ids[2]]);
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::Rebuild);

    f.build();
    let index = f.open(settings);
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::None);
    let record = f.store.ann_file(f.generation).unwrap().unwrap();
    // Windows cannot remove an open memory-mapped index. Model disappearance between
    // process lifetimes, then exercise the same degraded open/reopen path.
    drop(index);
    std::fs::remove_file(f.vectors_dir().join(&record.file_name)).unwrap();
    let mut index = f.open(settings);
    index.reopen_file(&f.store).unwrap();
    assert!(matches!(index.status().file, FileState::Unusable(_)));
    assert_eq!(f.top(&index, 3, 1), [ids[3]]);
    // Without a usable file the delta reloads every row (bounded by `delta_limit`), so
    // search keeps working, and maintenance asks for a rebuild.
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::Rebuild);
    f.store.clear_ann_file(f.generation).unwrap();
    f.build();
    index.reopen_file(&f.store).unwrap();
    assert_eq!(f.top(&index, 1, 1), [ids[1]]);
}

#[test]
fn stale_files_and_large_deltas_ask_for_a_rebuild() {
    let mut f = Fixture::new("maintenance");
    let ids = f.add(&[0, 1, 2, 3, 4]);
    f.build();
    let settings = IndexSettings {
        rebuild_delta_min: 3,
        rebuild_delta_fraction: 0.5,
        rebuild_stale_fraction: 0.4,
        delta_limit: 4,
    };
    let mut index = f.open(settings);
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::None);
    f.add(&[5, 6]);
    index.refresh(&f.store).unwrap();
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::None);
    f.add(&[7]);
    index.refresh(&f.store).unwrap();
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::Rebuild);

    f.build();
    index.reopen_file(&f.store).unwrap();
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::None);
    for &id in &ids[..4] {
        f.store
            .connection()
            .execute("DELETE FROM chunks WHERE id = ?1", [id])
            .unwrap();
    }
    assert_eq!(index.maintenance(&f.store).unwrap(), Maintenance::Rebuild);

    // The delta is bounded; beyond the limit search reports it is incomplete.
    let mut g = Fixture::new("delta-limit");
    g.add(&[0, 1, 2, 3, 4, 5]);
    let index = g.open(settings);
    assert!(index.status().incomplete);
    assert_eq!(index.status().delta_vectors, 4);
    assert_eq!(index.maintenance(&g.store).unwrap(), Maintenance::Rebuild);
}

#[test]
fn validation_and_cleanup() {
    let mut f = Fixture::new("validate");
    f.add(&(0..40).collect::<Vec<_>>());
    f.build();
    let index = f.open(IndexSettings::default());
    let v = validate(&f.store, &index, 16).unwrap();
    assert!(v.ok, "{v:?}");
    assert!(
        v.complete && v.sampled >= 16 && v.self_recall >= 0.95,
        "{v:?}"
    );

    // A chunk without a result: not complete.
    let item = f.item;
    f.store
        .insert_chunks(&[NewChunk {
            item_id: item,
            ordinal: 99,
            chunk_kind: "text",
            text: "pending",
            symbol_name: None,
            page_number: None,
            start_offset: None,
            end_offset: None,
        }])
        .unwrap();
    let v = validate(&f.store, &index, 8).unwrap();
    assert!(!v.complete && !v.ok, "{v:?}");

    // Cleanup keeps the recorded file and removes others (old builds, leftovers).
    let dir = f.vectors_dir();
    std::fs::write(dir.join(file_name(f.generation, 1)), b"old").unwrap();
    std::fs::write(dir.join("gen-9-9.usearch.tmp"), b"tmp").unwrap();
    std::fs::write(dir.join("notes.txt"), b"not ours").unwrap();
    drop(index);
    assert_eq!(cleanup_files(&f.store, &dir).unwrap(), 2);
    let record = f.store.ann_file(f.generation).unwrap().unwrap();
    assert!(dir.join(record.file_name).exists() && dir.join("notes.txt").exists());

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        build_file(&f.store, &dir, f.generation, &cancelled, 2),
        Err(IndexError::Cancelled)
    ));
}

mod provider {
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use lumen_core::{MatchKind, Provider, ProviderQuery, QueryId};
    use lumen_embedding::{Embedder, EmbeddingProfile, MockBackend, MockLatency};

    use super::*;
    use crate::{QueryConfig, QueryEmbedder, SemanticConfig, SemanticProvider};

    fn query(text: &str, typing: bool) -> ProviderQuery<'_> {
        ProviderQuery {
            id: QueryId::new(1).unwrap(),
            text,
            typing,
            limit: 5,
        }
    }

    #[test]
    fn settled_queries_find_files_through_the_active_generation() {
        let dir =
            std::env::temp_dir().join(format!("lumen-semantic-provider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("lumen.db");
        let mut store = Store::open_writer(&db).unwrap();
        let make = || {
            Embedder::new(
                Arc::new(MockBackend::with_latency(MockLatency {
                    per_call: Duration::ZERO,
                    ..MockLatency::default()
                })),
                EmbeddingProfile::DEFAULT,
            )
            .map_err(|e| e.to_string())
        };
        let embedder =
            Arc::new(QueryEmbedder::start(Box::new(make), None, QueryConfig::default()).unwrap());
        let never = CancellationToken::new();
        let space = make().unwrap().space().key();
        let generation = store
            .ensure_generation(
                GenerationSpec {
                    space_key: &space,
                    chunker_version: 1,
                    dim: 256,
                },
                0,
            )
            .unwrap();
        let texts = ["budget spreadsheet", "beach photos", "kubernetes ingress"];
        for (i, text) in texts.iter().enumerate() {
            let item = store
                .insert_item(&NewItem::file(&format!("/d/{i}.md"), &format!("{i}.md")))
                .unwrap();
            let ids = store
                .insert_chunks(&[NewChunk {
                    item_id: item,
                    ordinal: 0,
                    chunk_kind: "text",
                    text,
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                }])
                .unwrap();
            // The stored vector is the query vector of the same text: an exact neighbour.
            let v = embedder.embed(text, &never).unwrap();
            store
                .write_vectors(
                    generation,
                    &[VectorWrite {
                        chunk_id: ids[0],
                        result: Ok(&v),
                    }],
                    0,
                )
                .unwrap();
        }
        let shared: crate::SharedIndex = Arc::new(RwLock::new(None));
        let p = SemanticProvider::new(
            Arc::clone(&embedder),
            Arc::clone(&shared),
            Store::open_reader(&db).unwrap(),
            SemanticConfig::default(),
        );
        // No index yet; then a building (not active) generation: nothing, no error.
        assert!(
            p.search(&query("beach photos", false), &never)
                .unwrap()
                .is_empty()
        );
        let info = |store: &Store| {
            store
                .generations()
                .unwrap()
                .into_iter()
                .find(|g| g.id == generation)
                .unwrap()
        };
        *shared.write().unwrap() = Some(
            SemanticIndex::open(
                &store,
                &dir.join("v"),
                info(&store),
                IndexSettings::default(),
            )
            .unwrap(),
        );
        assert!(
            p.search(&query("beach photos", false), &never)
                .unwrap()
                .is_empty()
        );

        store.promote_first(generation, 1).unwrap();
        *shared.write().unwrap() = Some(
            SemanticIndex::open(
                &store,
                &dir.join("v"),
                info(&store),
                IndexSettings::default(),
            )
            .unwrap(),
        );
        assert!(
            p.search(&query("beach photos", true), &never)
                .unwrap()
                .is_empty()
        );
        assert!(p.search(&query("be", false), &never).unwrap().is_empty());
        let r = p.search(&query("beach photos", false), &never).unwrap();
        assert_eq!(r[0].title, "1.md");
        assert_eq!(r[0].score.match_kind, MatchKind::Semantic);
        assert!(r[0].score.confidence.get() > 0.99);
        assert_eq!(r[0].subtitle.as_deref(), Some("beach photos"));
        assert_eq!(r[0].provider, crate::SEMANTIC_PROVIDER_ID);
        let filtered = p
            .search(
                &query("beach photos ext:md in:/d type:document", false),
                &never,
            )
            .unwrap();
        assert_eq!(filtered[0].id, r[0].id);
        assert_eq!(
            filtered[0].score, r[0].score,
            "operators must not enter the query embedding"
        );
        for text in [
            "beach photos ext:png",
            "beach photos type:app",
            "beach photos before:bad",
            "\"beach photos\"",
            "ext:md",
        ] {
            assert!(
                p.search(&query(text, false), &never).unwrap().is_empty(),
                "{text}"
            );
        }

        // A narrow filter must look past the ordinary candidate window. Validate
        // both the exact delta and the persisted ANN using synthetic vectors.
        let vector = embedder.embed("beach photos", &never).unwrap();
        for i in 0..65 {
            let path = if i == 64 {
                "/allowed/wanted.md".to_owned()
            } else {
                format!("/excluded/{i}.txt")
            };
            let item = store
                .insert_item(&NewItem::file(
                    &path,
                    if i == 64 { "wanted.md" } else { "noise.txt" },
                ))
                .unwrap();
            let chunk_id = store
                .insert_chunks(&[NewChunk {
                    item_id: item,
                    ordinal: 0,
                    chunk_kind: "text",
                    text: "beach photos",
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                }])
                .unwrap()[0];
            store
                .write_vectors(
                    generation,
                    &[VectorWrite {
                        chunk_id,
                        result: Ok(&vector),
                    }],
                    0,
                )
                .unwrap();
        }
        shared
            .write()
            .unwrap()
            .as_mut()
            .unwrap()
            .refresh(&store)
            .unwrap();
        let assert_filtered = || {
            let mut q = query("beach photos in:/allowed ext:md", false);
            q.limit = 1;
            let results = p.search(&q, &never).unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].title, "wanted.md");
        };
        assert_filtered();
        let record = build_file(&store, &dir.join("v"), generation, &never, 1).unwrap();
        store.set_ann_file(&record).unwrap();
        shared
            .write()
            .unwrap()
            .as_mut()
            .unwrap()
            .reopen_file(&store)
            .unwrap();
        assert_filtered();
        // The semantic lane carries exactly the same code context as lexical results.
        let code_id = store
            .insert_item(&NewItem::file("/repo/client.py", "client.py"))
            .unwrap();
        let chunk_id = store
            .insert_chunks(&[NewChunk {
                item_id: code_id,
                ordinal: 0,
                chunk_kind: "code",
                text: "def retry(): pass",
                symbol_name: Some("retry"),
                page_number: None,
                start_offset: Some(9000),
                end_offset: Some(9020),
            }])
            .unwrap()[0];
        store
            .set_code_context(code_id, "/repo/client.py", "python", Some("/repo"))
            .unwrap();
        let vector = embedder.embed("retry failed requests", &never).unwrap();
        store
            .write_vectors(
                generation,
                &[VectorWrite {
                    chunk_id,
                    result: Ok(&vector),
                }],
                0,
            )
            .unwrap();
        shared
            .write()
            .unwrap()
            .as_mut()
            .unwrap()
            .refresh(&store)
            .unwrap();
        let code = p
            .search(&query("retry failed requests", false), &never)
            .unwrap()
            .remove(0);
        assert_eq!(code.kind, lumen_core::ResultKind::Code);
        assert!(lumen_core::validate_result(&code, &lumen_core::builtin::DESCRIPTORS).is_empty());
        let lumen_core::Payload::Code(target) = &code.payload else {
            panic!("missing code target")
        };
        assert_eq!(target.symbol.as_deref(), Some("retry"));
        assert_eq!(target.language, "python");
        assert_eq!(target.start_offset, Some(9000));
        assert_eq!(target.passage, "def retry(): pass");
        drop(p);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
