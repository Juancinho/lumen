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

#[path = "../../../fixtures/pdf/mod.rs"]
mod pdf_fixture;

struct Visual {
    inner: MockBackend,
    caps: Capabilities,
    calls: std::sync::Mutex<Vec<Modality>>,
    edit: Option<PathBuf>,
}
impl Visual {
    fn new(edit: Option<PathBuf>) -> Self {
        let inner = MockBackend::new();
        let mut caps = inner.capabilities().clone();
        caps.model.modalities =
            lumen_embedding::ModalitySet::of(&[Modality::Text, Modality::Image]);
        Self {
            inner,
            caps,
            calls: Default::default(),
            edit,
        }
    }
}
impl EmbeddingBackend for Visual {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
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
        self.calls.lock().unwrap().push(Modality::Text);
        self.inner.embed_text(inputs)
    }
    fn embed_images(
        &self,
        images: &[lumen_embedding::ImageInput<'_>],
    ) -> Result<Vec<f32>, EmbeddingError> {
        assert_eq!(images.len(), 1);
        self.calls.lock().unwrap().push(Modality::Image);
        if let Some(path) = &self.edit {
            image::RgbImage::from_pixel(4, 3, image::Rgb([0, 255, 0]))
                .save(path)
                .unwrap();
        }
        let mut raw = vec![0.0; self.caps.model.native_dim];
        raw[0] = 2.0;
        Ok(raw)
    }
}

fn image_pass(store: &mut Store) -> PassReport {
    run_image_pass(store, &|_| true, &CancellationToken::new(), &|| 1).unwrap()
}

struct TestOcr {
    calls: usize,
    text: &'static str,
    cancel: Option<CancellationToken>,
    edit: Option<PathBuf>,
    remove: Option<PathBuf>,
}
impl ocr::Recognizer for TestOcr {
    fn recognize(
        &mut self,
        _: u32,
        _: u32,
        _: &[u8],
        _: &dyn Fn() -> bool,
    ) -> Result<ocr::OcrText, ocr::OcrError> {
        self.calls += 1;
        if let Some(path) = &self.remove {
            std::fs::remove_file(path).unwrap();
        }
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        if let Some(path) = &self.edit {
            image::RgbImage::from_pixel(4, 3, image::Rgb([0, 255, 0]))
                .save(path)
                .unwrap();
        }
        Ok(ocr::OcrText {
            text: self.text.into(),
            language: "es-ES".into(),
        })
    }
}
fn test_ocr(text: &'static str) -> TestOcr {
    TestOcr {
        calls: 0,
        text,
        cancel: None,
        edit: None,
        remove: None,
    }
}
fn ocr_pass(store: &mut Store, recognizer: &mut TestOcr) -> PassReport {
    ocr::run_ocr_slice(
        store,
        &|_| true,
        &CancellationToken::new(),
        &|| false,
        recognizer,
        0,
        Slice::unbounded(),
    )
    .unwrap()
}

#[test]
fn ocr_fts_off_resume_moves_and_edits_preserve_visual_units_and_unrelated_vectors() {
    use lumen_core::{Provider, ProviderQuery, QueryId, ResultKind};
    let t = Temp::new("ocr-pipeline");
    let path = t.files().join("0001.png");
    image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    std::fs::write(t.files().join("notes.txt"), "unrelated text").unwrap();
    let mut store = t.store();
    sync(&mut store, &t.files());
    image_pass(&mut store);
    pass(&mut store, &PassConfig::default());
    let visual = embedder(Arc::new(Visual::new(None)));
    let g = generation(&store, &visual);
    queue(
        &mut store,
        &visual,
        g,
        &Control::new(),
        &QueueConfig::default(),
    )
    .unwrap();
    store.promote_first(g, 2).unwrap();
    let original = store.vectors(g, 0, 100).unwrap();
    let candidate = store.ocr_candidates(0, 1).unwrap().remove(0);
    let mut recognizer = test_ocr("ERROR 42\nBuscar texto en imágenes");
    let denied = ocr::run_ocr_slice(
        &mut store,
        &|_| false,
        &CancellationToken::new(),
        &|| false,
        &mut recognizer,
        0,
        Slice::unbounded(),
    )
    .unwrap();
    assert_eq!((denied.files, recognizer.calls), (0, 0));
    assert_eq!(store.ocr_counts(&|_| true).unwrap().pending, 1);
    assert_eq!(ocr_pass(&mut store, &mut recognizer).indexed, 1);
    assert!(store.ocr_candidates(0, 1).unwrap().is_empty());
    assert_eq!(store.vectors(g, 0, 100).unwrap(), original);
    drop(store);
    let mut store = t.store();
    let preview = store.image_ocr_preview(candidate.item).unwrap().unwrap();
    assert_eq!(
        (preview.state.as_str(), preview.language.as_deref()),
        ("indexed", Some("es-ES"))
    );
    let provider =
        lumen_catalog::ContentProvider::new(Store::open_reader(&t.0.join("lumen.db")).unwrap());
    let query = ProviderQuery {
        id: QueryId::new(1).unwrap(),
        text: "\"ERROR 42\" type:image ext:png",
        typing: false,
        limit: 10,
    };
    let result = provider
        .search(&query, &CancellationToken::new())
        .unwrap()
        .remove(0);
    assert_eq!(result.kind, ResultKind::Image);
    assert_eq!(result.id.as_str(), format!("item:{}", candidate.item));
    assert!(result.subtitle.as_deref().unwrap().contains("ERROR 42"));
    assert!(
        result.offers(&lumen_core::builtin::OPEN)
            && result.offers(&lumen_core::builtin::REVEAL)
            && result.offers(&lumen_core::builtin::COPY_PATH)
    );
    drop(provider);
    // Same source after a notified rename retains the OCR unit and its vector.
    let opts = incremental_opts(&t.files());
    sync_files(&mut store, &opts, None).unwrap();
    let moved = t.files().join("0002.png");
    std::fs::rename(&path, &moved).unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(&[path, moved.clone()], true),
        None,
    )
    .unwrap();
    assert_eq!(store.vectors(g, 0, 100).unwrap(), original);
    assert_eq!(
        store
            .image_ocr_preview(candidate.item)
            .unwrap()
            .unwrap()
            .text,
        preview.text
    );
    // Off removes lexical text and coverage, keeping expensive visual inference intact.
    assert_eq!(store.clear_ocr_page().unwrap(), 1);
    assert_eq!(store.clear_ocr_page().unwrap(), 0);
    assert!(store.image_ocr_preview(candidate.item).unwrap().is_none());
    assert_eq!(store.vectors(g, 0, 100).unwrap(), original);
    let provider =
        lumen_catalog::ContentProvider::new(Store::open_reader(&t.0.join("lumen.db")).unwrap());
    assert!(
        provider
            .search(&query, &CancellationToken::new())
            .unwrap()
            .is_empty()
    );
    drop(provider);
    assert_eq!(image_pass(&mut store).files, 0);
    assert_eq!(ocr_pass(&mut store, &mut recognizer).indexed, 1);
    // A source edit invalidates its OCR + visual unit, while the unrelated text survives.
    image::RgbImage::from_pixel(4, 3, image::Rgb([0, 0, 255]))
        .save(&moved)
        .unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(std::slice::from_ref(&moved), true),
        None,
    )
    .unwrap();
    assert!(store.image_ocr_preview(candidate.item).unwrap().is_none());
    assert_eq!(store.vectors(g, 0, 100).unwrap().len(), 1);
    image_pass(&mut store);
    ocr_pass(&mut store, &mut recognizer);
    std::fs::remove_file(&moved).unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[moved], true), None).unwrap();
    assert!(store.image_ocr_preview(candidate.item).unwrap().is_none());
}

#[test]
fn ocr_cancel_empty_pixel_bounds_and_changed_pixels_are_distinct_and_resumable() {
    let t = Temp::new("ocr-controls");
    let path = t.files().join("a.png");
    image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    image::RgbImage::from_pixel(4097, 1, image::Rgb([255, 0, 0]))
        .save(t.files().join("b.png"))
        .unwrap();
    let mut store = t.store();
    sync(&mut store, &t.files());
    image_pass(&mut store);
    let candidate = store.ocr_candidates(0, 1).unwrap().remove(0);
    let mut recognizer = test_ocr("");
    let held = ocr::run_ocr_slice(
        &mut store,
        &|_| true,
        &CancellationToken::new(),
        &|| true,
        &mut recognizer,
        0,
        Slice::unbounded(),
    )
    .unwrap();
    assert!(held.cancelled);
    assert_eq!((held.cursor, recognizer.calls), (0, 0));
    let token = CancellationToken::new();
    recognizer.cancel = Some(token.clone());
    let cancelled = ocr::run_ocr_slice(
        &mut store,
        &|_| true,
        &token,
        &|| false,
        &mut recognizer,
        0,
        Slice::unbounded(),
    )
    .unwrap();
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.cursor, 0);
    assert!(store.image_ocr_preview(candidate.item).unwrap().is_none());
    recognizer.cancel = None;
    let report = ocr_pass(&mut store, &mut recognizer);
    assert_eq!((report.indexed, report.skipped), (1, 1));
    let counts = store.ocr_counts(&|_| true).unwrap();
    assert_eq!((counts.empty, counts.skipped, counts.pending), (1, 1, 0));
    assert_eq!(recognizer.calls, 2); // Oversized header never reaches recognition.
    assert!(
        store
            .write_ocr(
                &candidate,
                &"x".repeat(16 * 1024 + 1),
                Some("es-ES"),
                "indexed",
                None
            )
            .is_err()
    );
    assert!(
        store
            .write_ocr(
                &candidate,
                "",
                None,
                "failed",
                Some("private backend detail")
            )
            .is_err()
    );
    store.clear_ocr_page().unwrap();
    recognizer.text = "stale";
    recognizer.edit = Some(path.clone());
    ocr_pass(&mut store, &mut recognizer);
    assert!(store.image_ocr_preview(candidate.item).unwrap().is_none());
    assert_eq!(image_pass(&mut store).indexed, 1);
    recognizer.edit = None;
    recognizer.remove = Some(path);
    assert_eq!(ocr_pass(&mut store, &mut recognizer).failed, 1);
    let failed = store.image_ocr_preview(candidate.item).unwrap().unwrap();
    assert_eq!(failed.reason.as_deref(), Some("image:io"));
    assert!(failed.text.is_empty());
}

#[test]
fn bounded_extraction_resumes_inside_a_page_and_keeps_cancelled_work() {
    let t = Temp::new("sliced-extraction");
    let mut store = seeded(&t);
    let cfg = PassConfig {
        max_bytes: 4096,
        ..PassConfig::default()
    };
    let cancel = CancellationToken::new();
    let slice = Slice {
        max_files: 1,
        max_run: Duration::MAX,
    };
    let mut cursor = 0;
    let mut files = 0;
    loop {
        let r = run_content_slice(
            &mut store,
            &cfg,
            &EstimateTokens,
            &|_| true,
            &cancel,
            &|| 1,
            &mut |_| {},
            cursor,
            slice,
        )
        .unwrap();
        assert!(r.files <= 1);
        files += r.files;
        cursor = r.cursor;
        if r.exhausted {
            break;
        }
        assert!(files <= 5);
    }
    assert_eq!(files, 5);
    assert_eq!(store.content_counts().unwrap().indexed, 3);
    let coverage = store
        .file_coverage(lumen_extract::TEXT_EXTENSIONS, &|_| true)
        .unwrap();
    assert_eq!(
        (
            coverage.total,
            coverage.read,
            coverage.skipped,
            coverage.failed
        ),
        (5, 5, 2, 0)
    );
    assert_eq!(
        store
            .file_coverage(lumen_extract::TEXT_EXTENSIONS, &|_| false)
            .unwrap()
            .total,
        0
    );
    assert_eq!(pass(&mut store, &cfg).files, 0);

    let t = Temp::new("sliced-images");
    for n in 0..3 {
        image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
            .save(t.files().join(format!("{n}.png")))
            .unwrap();
    }
    let mut store = t.store();
    sync(&mut store, &t.files());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let r = run_image_slice(&mut store, &|_| true, &cancelled, &|| 1, 0, slice).unwrap();
    assert!(r.cancelled);
    assert_eq!(r.cursor, 0);
    assert!(!r.exhausted);
    let mut cursor = 0;
    let mut files = 0;
    loop {
        let r = run_image_slice(&mut store, &|_| true, &cancel, &|| 1, cursor, slice).unwrap();
        assert!(r.files <= 1);
        files += r.files;
        cursor = r.cursor;
        if r.exhausted {
            break;
        }
        assert!(files <= 3);
    }
    assert_eq!(files, 3);
    let coverage = store.file_coverage(IMAGE_EXTENSIONS, &|_| true).unwrap();
    assert_eq!((coverage.total, coverage.read), (3, 3));
    assert_eq!(image_pass(&mut store).files, 0);
}

#[test]
fn images_get_a_turn_before_a_large_text_backlog_drains_without_skipping_text() {
    let t = Temp::new("image-fairness");
    for n in 0..2 {
        image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
            .save(t.files().join(format!("photo{n}.png")))
            .unwrap();
    }
    for n in 0..100 {
        std::fs::write(
            t.files().join(format!("note{n}.txt")),
            "searchable document contents",
        )
        .unwrap();
    }
    let mut store = t.store();
    sync(&mut store, &t.files());
    image_pass(&mut store); // Image chunk IDs precede the entire text backlog.
    pass(&mut store, &PassConfig::default());
    let backend = Arc::new(Visual::new(None));
    let e = embedder(backend.clone());
    let g = generation(&store, &e);
    let r = queue(
        &mut store,
        &e,
        g,
        &Control::new(),
        &QueueConfig {
            batch: 1,
            ..QueueConfig::default()
        },
    )
    .unwrap();
    assert_eq!(r.stop, Stop::Drained);
    assert_eq!(r.embedded, 102);
    let calls = backend.calls.lock().unwrap();
    assert_eq!(&calls[..8], &[Modality::Text; 8]);
    assert_eq!(calls[8], Modality::Image);
    assert_eq!(calls[17], Modality::Image);
    assert_eq!(store.image_counts(Some(g)).unwrap().indexed, 2);
    assert_eq!(store.queue_counts(g).unwrap().pending(), 0);
}

#[test]
fn images_are_consent_bound_deferred_resumable_and_keep_text_vectors_and_actions() {
    use lumen_core::{ImageState, Payload, Provider, ProviderQuery, QueryId, ResultKind};
    let t = Temp::new("image-pipeline");
    let path = t.files().join("0001.PNG");
    image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    std::fs::write(t.files().join("notes.txt"), "text contents").unwrap();
    let mut store = t.store();
    sync(&mut store, &t.files());
    let r = run_image_pass(&mut store, &|_| false, &CancellationToken::new(), &|| 1).unwrap();
    assert_eq!(r.files, 0);
    assert_eq!(image_pass(&mut store).indexed, 1);
    pass(&mut store, &PassConfig::default());
    let text = embedder(Arc::new(MockBackend::new()));
    let g = generation(&store, &text);
    let r = queue(
        &mut store,
        &text,
        g,
        &Control::new(),
        &QueueConfig::default(),
    )
    .unwrap();
    assert_eq!((r.embedded, r.failed, r.stop), (1, 0, Stop::Drained));
    let original = store.vectors(g, 0, 100).unwrap();
    let pending = store.pending_chunks(g, 0, 100).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        (pending[0].kind.as_str(), pending[0].text.as_str()),
        ("image", "")
    );
    assert_eq!(store.image_counts(Some(g)).unwrap().pending, 1);
    drop(store);
    let mut store = t.store();
    assert_eq!(image_pass(&mut store).files, 0);
    assert_eq!(store.pending_chunks(g, 0, 100).unwrap(), pending);
    let visual = embedder(Arc::new(Visual::new(None)));
    assert_eq!(visual.space(), text.space());
    assert_eq!(
        queue(
            &mut store,
            &visual,
            g,
            &Control::new(),
            &QueueConfig::default()
        )
        .unwrap()
        .embedded,
        1
    );
    store.promote_first(g, 2).unwrap();
    assert_eq!(store.image_counts(Some(g)).unwrap().indexed, 1);
    assert!(store.vectors(g, 0, 100).unwrap().contains(&original[0]));
    store.checkpoint().unwrap();
    let provider =
        lumen_catalog::CatalogProvider::new(Store::open_reader(&t.0.join("lumen.db")).unwrap());
    let result = provider
        .search(
            &ProviderQuery {
                id: QueryId::new(1).unwrap(),
                text: "0001 type:image",
                typing: false,
                limit: 10,
            },
            &CancellationToken::new(),
        )
        .unwrap()
        .remove(0);
    assert_eq!(result.kind, ResultKind::Image);
    assert!(lumen_core::validate_result(&result, &lumen_core::builtin::DESCRIPTORS).is_empty());
    assert!(
        result.offers(&lumen_core::builtin::REVEAL)
            && result.offers(&lumen_core::builtin::COPY_PATH)
    );
    let Payload::Image(target) = result.payload else {
        panic!("missing image context");
    };
    assert_eq!(target.visual_state, ImageState::Indexed);
    assert_eq!((target.width, target.height), (Some(4), Some(3)));
    drop(provider);
    // A notified unchanged rename retains vectors; an edit invalidates only that image.
    let opts = incremental_opts(&t.files());
    sync_files(&mut store, &opts, None).unwrap();
    let before = store.vectors(g, 0, 100).unwrap();
    let moved = t.files().join("0002.png");
    std::fs::rename(&path, &moved).unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(&[path, moved.clone()], true),
        None,
    )
    .unwrap();
    assert_eq!(store.vectors(g, 0, 100).unwrap(), before);
    image::RgbImage::from_pixel(4, 3, image::Rgb([0, 0, 255]))
        .save(&moved)
        .unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(std::slice::from_ref(&moved), true),
        None,
    )
    .unwrap();
    assert_eq!(store.vectors(g, 0, 100).unwrap(), original);
    assert_eq!(image_pass(&mut store).indexed, 1);
    assert_eq!(store.image_counts(Some(g)).unwrap().pending, 1);
    // A metadata I/O failure retains a unit for retry, but coverage counts the file once.
    let candidate = store
        .content_candidates(0, &["png"], 2, 1)
        .unwrap()
        .remove(0);
    store
        .write_content(
            &[lumen_storage::ContentWrite {
                item_id: candidate.item_id,
                fingerprint: &candidate.fingerprint(),
                outcome: lumen_storage::ContentOutcome::Failed("image:io"),
            }],
            1,
            3,
        )
        .unwrap();
    let coverage = store.image_counts(Some(g)).unwrap();
    assert_eq!(
        (coverage.indexed, coverage.pending, coverage.failed),
        (0, 0, 1)
    );
    std::fs::remove_file(&moved).unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[moved], true), None).unwrap();
    assert_eq!(store.image_counts(Some(g)).unwrap(), Default::default());
}

#[test]
fn images_prioritize_text_reject_stale_pixels_and_bound_failed_work() {
    let t = Temp::new("image-stale");
    let path = t.files().join("0001.png");
    image::RgbImage::from_pixel(4, 3, image::Rgb([255, 0, 0]))
        .save(&path)
        .unwrap();
    std::fs::write(t.files().join("notes.txt"), "document text").unwrap();
    std::fs::write(t.files().join("bad.png"), b"broken").unwrap();
    std::fs::write(t.files().join("unsupported.gif"), b"GIF89a").unwrap();
    let mut store = t.store();
    sync(&mut store, &t.files());
    let r = image_pass(&mut store);
    assert_eq!((r.indexed, r.skipped), (1, 2));
    pass(&mut store, &PassConfig::default());
    let backend = Arc::new(Visual::new(Some(path)));
    let e = embedder(backend.clone());
    let g = generation(&store, &e);
    let r = queue(&mut store, &e, g, &Control::new(), &QueueConfig::default()).unwrap();
    assert_eq!(
        *backend.calls.lock().unwrap(),
        [Modality::Text, Modality::Image]
    );
    assert_eq!((r.embedded, r.failed, r.stale_images), (1, 0, 1));
    assert_eq!(store.image_counts(Some(g)).unwrap().indexed, 0);
    assert_eq!(store.image_counts(Some(g)).unwrap().skipped, 2);
    let bad_id = store
        .item_id_by_path(&t.files().join("bad.png").to_string_lossy())
        .unwrap()
        .unwrap();
    let row = lumen_catalog::provider::to_result(
        &store.catalog_item(bad_id).unwrap().unwrap(),
        lumen_core::Score::new(
            lumen_core::Confidence::CERTAIN,
            lumen_core::MatchKind::Exact,
        ),
    )
    .unwrap();
    let lumen_core::Payload::Image(image) = row.payload else {
        panic!("skipped image should expose coverage");
    };
    assert_eq!(image.visual_state, lumen_core::ImageState::Skipped);
    assert_eq!((image.width, image.height), (None, None));
    assert_eq!(image.issue, Some("image:unsupported"));
    assert_eq!(image_pass(&mut store).indexed, 1);
    assert_eq!(store.pending_chunks(g, 0, 10).unwrap().len(), 1);
    assert_eq!(image_pass(&mut store).files, 0);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        run_image_pass(&mut store, &|_| true, &cancel, &|| 2)
            .unwrap()
            .files
            == 0
    );
}

#[test]
fn pdf_content_pages_queue_resume_scope_and_existing_vectors() {
    use lumen_core::{Payload, Provider, ProviderQuery, QueryId, ResultKind};
    let t = Temp::new("pdf-pipeline");
    let mut store = seeded(&t);
    pass(&mut store, &PassConfig::default());
    let generation = vectors_for_all_chunks(&mut store);
    let original = store.vectors(generation, 0, 100).unwrap();
    let path = t.files().join("ocean.PDF");
    pdf_fixture::document(&[
        b"solar panel energy",
        b"",
        b"coral ocean habitat protection",
    ])
    .save(&path)
    .unwrap();
    sync(&mut store, &t.files());
    // Names-only consent leaves the PDF pending, and no file is read.
    let r = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &EstimateTokens,
        &|_| false,
        &CancellationToken::new(),
        &|| 1,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.files, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
    assert_eq!(store.vectors(generation, 0, 100).unwrap(), original);
    let pending = store.pending_chunks(generation, 0, 100).unwrap();
    assert_eq!(pending.len(), 2);
    let refs = store
        .chunk_refs(
            &pending.iter().map(|c| c.chunk_id).collect::<Vec<_>>(),
            4000,
        )
        .unwrap();
    assert_eq!(
        refs.iter().map(|r| r.page_number).collect::<Vec<_>>(),
        [Some(1), Some(3)]
    );
    assert_eq!(refs[1].excerpt.trim(), "coral ocean habitat protection");
    let query = ProviderQuery {
        id: QueryId::new(1).unwrap(),
        text: "habitat ext:pdf",
        typing: false,
        limit: 10,
    };
    store.checkpoint().unwrap();
    let provider =
        lumen_catalog::ContentProvider::new(Store::open_reader(&t.0.join("lumen.db")).unwrap());
    let result = provider
        .search(&query, &CancellationToken::new())
        .unwrap()
        .remove(0);
    assert_eq!(result.kind, ResultKind::PdfPage);
    assert!(lumen_core::validate_result(&result, &lumen_core::builtin::DESCRIPTORS).is_empty());
    assert_eq!(result.primary_action, lumen_core::builtin::OPEN);
    assert!(
        result.offers(&lumen_core::builtin::REVEAL)
            && result.offers(&lumen_core::builtin::COPY_PATH)
    );
    let Payload::Pdf(pdf) = result.payload else {
        panic!("missing PDF target");
    };
    assert_eq!(pdf.page_number.get(), 3);
    assert_eq!(pdf.path, path);
    assert!(pdf.passage.contains("habitat"));
    drop(provider);
    drop(store);
    let mut store = t.store();
    assert_eq!(pass(&mut store, &PassConfig::default()).files, 0);
    assert_eq!(store.pending_chunks(generation, 0, 100).unwrap(), pending);
    let vectors: Vec<_> = pending
        .iter()
        .map(|p| lumen_storage::VectorWrite {
            chunk_id: p.chunk_id,
            result: Ok(&[1.0, 0.0]),
        })
        .collect();
    store.write_vectors(generation, &vectors, 2).unwrap();
    assert_eq!(store.queue_counts(generation).unwrap().pending(), 0);
    let opts = incremental_opts(&t.files());
    sync_files(&mut store, &opts, None).unwrap();
    let before = store.vectors(generation, 0, 100).unwrap();
    let moved = t.files().join("marine.pdf");
    std::fs::rename(&path, &moved).unwrap();
    lumen_catalog::sync_changes(
        &mut store,
        &opts,
        &hints(&[path, moved.clone()], true),
        None,
    )
    .unwrap();
    assert_eq!(
        store.vectors(generation, 0, 100).unwrap(),
        before,
        "unchanged PDF rename preserves vectors"
    );
    assert_eq!(pass(&mut store, &PassConfig::default()).files, 0);
    // A notified edit removes stale PDF content/vectors and re-queues its new text.
    pdf_fixture::document(&[b"new replacement text"])
        .save(&moved)
        .unwrap();
    lumen_catalog::sync_changes(&mut store, &opts, &hints(&[moved], true), None).unwrap();
    assert_eq!(store.vectors(generation, 0, 100).unwrap(), original);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
    assert_eq!(store.pending_chunks(generation, 0, 100).unwrap().len(), 1);
}

#[test]
fn pdf_failures_are_visible_and_cancellation_never_commits_partial_pages() {
    let t = Temp::new("pdf-skips");
    pdf_fixture::document(&[b""])
        .save(t.files().join("scan.pdf"))
        .unwrap();
    std::fs::write(t.files().join("broken.pdf"), b"%PDF-1.7\ninvalid").unwrap();
    let mut store = t.store();
    sync(&mut store, &t.files());
    let r = pass(&mut store, &PassConfig::default());
    assert_eq!((r.files, r.skipped, r.chunks), (2, 2, 0));
    let errors: Vec<String> = store
        .connection()
        .prepare("SELECT content_error FROM items WHERE kind = 'file' ORDER BY content_error")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(errors, ["pdf:malformed", "pdf:no_text"]);
    assert_eq!(
        pass(&mut store, &PassConfig::default()).files,
        0,
        "unchanged unsupported PDFs do not retry forever"
    );
    pdf_fixture::document(&[b"first page", b"second page"])
        .save(t.files().join("cancel.pdf"))
        .unwrap();
    sync(&mut store, &t.files());
    let cancel = CancellationToken::new();
    struct Cancelling<'a>(&'a CancellationToken);
    impl lumen_extract::TokenCount for Cancelling<'_> {
        fn count(&self, text: &str) -> usize {
            self.0.cancel();
            EstimateTokens.count(text)
        }
    }
    let r = run_content_pass(
        &mut store,
        &PassConfig::default(),
        &Cancelling(&cancel),
        &|_| true,
        &cancel,
        &|| 1,
        &mut |_| {},
    )
    .unwrap();
    assert!(r.cancelled);
    assert_eq!((r.files, r.indexed, r.skipped), (0, 0, 0));
    assert_eq!(store.queue_counts(0).unwrap().chunks, 0);
    assert_eq!(pass(&mut store, &PassConfig::default()).indexed, 1);
    assert_eq!(store.queue_counts(0).unwrap().chunks, 2);
}

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
