//! Optional actual q4 vision -> existing text backbone contract; no downloads in tests.
use lumen_embedding::{Embedder, EmbeddingProfile, ImageInput, Modality, dot};
use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig, init_runtime};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn native_vision_outputs_share_the_existing_text_space() {
    let (Ok(model), Ok(vision), Ok(dylib)) = (
        std::env::var("LUMEN_EG2_MODEL_DIR"),
        std::env::var("LUMEN_EG2_VISION_DIR"),
        std::env::var("LUMEN_ORT_DYLIB"),
    ) else {
        return;
    };
    init_runtime(PathBuf::from(dylib).as_path()).unwrap();
    let mut text_config = OrtConfig::new(&model, ModelVariant::Q4, Device::Cpu);
    text_config.threads = Some(2);
    let text = Embedder::new(
        Arc::new(OrtBackend::new(text_config.clone()).unwrap()),
        EmbeddingProfile::DEFAULT,
    )
    .unwrap();
    text_config.vision_dir = Some(vision.into());
    let backend = Arc::new(OrtBackend::new(text_config).unwrap());
    assert!(
        backend
            .capabilities()
            .model
            .modalities
            .contains(Modality::Image)
    );
    let visual = Embedder::new(backend.clone(), EmbeddingProfile::DEFAULT).unwrap();
    assert_eq!(text.space(), visual.space());
    let pixels = [220, 10, 10].repeat(48 * 48);
    let input = ImageInput {
        width: 48,
        height: 48,
        rgb: &pixels,
    };
    let a = visual.embed_images(&[input], None).unwrap().into_flat();
    let b = visual.embed_images(&[input], None).unwrap().into_flat();
    assert_eq!(a.len(), 256);
    assert!((dot(&a, &a) - 1.0).abs() < 0.0001);
    assert!(dot(&a, &b) > 0.99999);
    let query = text.embed_query("a red image", None).unwrap();
    assert!(dot(&a, &query).is_finite());
    // Reusing a CPU backbone after image inference must not pollute ordinary text.
    let shared_query = visual.embed_query("a red image", None).unwrap();
    assert!(dot(&query, &shared_query) > 0.99999);
    use lumen_embedding::EmbeddingBackend;
    backend.unload(Modality::Image).unwrap();
    assert!(!backend.is_warm(Modality::Image));
}
