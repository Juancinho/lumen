//! Windows release evidence with generated public text only, in an isolated catalog.
#[cfg(windows)]
mod native {
    use lumen_content::ocr::{OcrError, OcrText, Recognizer};
    use lumen_core::{CancellationToken, Provider, ProviderQuery, QueryId};
    use lumen_storage::{GenerationSpec, Store, VectorWrite};
    use std::{path::PathBuf, time::Instant};
    struct Ocr(lumen_windows::ocr::Engine);
    impl Recognizer for Ocr {
        fn recognize(
            &mut self,
            w: u32,
            h: u32,
            rgb: &[u8],
            stop: &dyn Fn() -> bool,
        ) -> Result<OcrText, OcrError> {
            self.0
                .recognize(w, h, rgb, stop)
                .map(|r| OcrText {
                    text: r.text,
                    language: r.language,
                })
                .map_err(|_| OcrError::Failed("ocr:recognition"))
        }
    }
    pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
        if cfg!(debug_assertions) {
            return Err("release required".into());
        }
        let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
        if args.len() != 2 {
            return Err("synthetic-fixture.png output.json".into());
        }
        let root = std::env::temp_dir().join(format!("lumen-t304-ocr-{}", std::process::id()));
        let files = root.join("files");
        std::fs::create_dir_all(&files)?;
        for n in 0..5 {
            std::fs::copy(&args[0], files.join(format!("{n}.png")))?;
        }
        let db = root.join("lumen.db");
        let mut store = Store::open_writer(&db)?;
        lumen_catalog::sync_files(
            &mut store,
            &lumen_indexer::ScanOptions {
                roots: vec![files],
                exclusions: Default::default(),
                identity: false,
            },
            None,
        )?;
        lumen_content::run_image_pass(&mut store, &|_| true, &CancellationToken::new(), &|| 1)?;
        let candidates = store.ocr_candidates(0, 10)?;
        let generation = store.ensure_generation(
            GenerationSpec {
                space_key: "synthetic-ocr-preservation",
                chunker_version: 1,
                dim: 2,
            },
            1,
        )?;
        for c in &candidates {
            store.write_vectors(
                generation,
                &[VectorWrite {
                    chunk_id: c.chunk,
                    result: Ok(&[1.0, 0.0]),
                }],
                1,
            )?;
        }
        let vectors = store.vectors(generation, 0, 10)?;
        let sequences: Vec<i64> = store
            .connection()
            .prepare("SELECT seq FROM chunk_vectors ORDER BY seq")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let memory_before = memory_stats::memory_stats().map(|m| m.physical_mem);
        let start = Instant::now();
        let mut ocr = Ocr(Default::default());
        let language = ocr.0.language()?;
        let creation_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut timings = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            lumen_content::ocr::run_ocr_slice(
                &mut store,
                &|_| true,
                &CancellationToken::new(),
                &|| false,
                &mut ocr,
                0,
                lumen_content::Slice {
                    max_files: 1,
                    max_run: std::time::Duration::MAX,
                },
            )?;
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let memory_after = memory_stats::memory_stats().map(|m| m.physical_mem);
        let counts = store.ocr_counts(&|_| true)?;
        assert_eq!(counts.indexed, 5);
        assert_eq!(counts.pending, 0);
        assert_eq!(store.vectors(generation, 0, 10)?, vectors);
        let provider = lumen_catalog::ContentProvider::new(Store::open_reader(&db)?);
        let results = provider.search(
            &ProviderQuery {
                id: QueryId::new(1).ok_or("invalid synthetic query id")?,
                text: "\"ERROR 42\" type:image ext:png",
                typing: false,
                limit: 10,
            },
            &CancellationToken::new(),
        )?;
        assert_eq!(results.len(), 5);
        drop(provider);
        assert_eq!(store.clear_ocr_page()?, 5);
        assert_eq!(store.vectors(generation, 0, 10)?, vectors);
        let after: Vec<i64> = store
            .connection()
            .prepare("SELECT seq FROM chunk_vectors ORDER BY seq")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        assert_eq!(after, sequences);
        std::fs::write(
            &args[1],
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version":1,"language":language,"fixture":"generated 1200x300 visible text",
                "creation_ms":creation_ms,"decode_recognize_verify_commit_ms":timings,
                "working_set_before_bytes":memory_before,"working_set_after_bytes":memory_after,
                "root_phrase_type_ext_hits":5,"visual_vectors_and_sequences_preserved":true,"off_removes_ocr":true,
                "cpu":std::env::var("PROCESSOR_IDENTIFIER").ok(),"logical_cpus":std::thread::available_parallelism().ok().map(usize::from),
            }))?,
        )?;
        drop(store);
        drop(ocr);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}
#[cfg(not(windows))]
fn main() {
    let _ = std::io::Write::write_all(
        &mut std::io::stderr(),
        b"Native Windows OCR evidence requires Windows\n",
    );
}
