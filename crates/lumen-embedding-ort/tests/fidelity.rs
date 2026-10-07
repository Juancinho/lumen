//! Fidelity of the Rust ONNX Runtime path against the Python fp32 reference
//! (`fixtures/embedding/reference-eg2-onnx-fp32-d256.json`, made by
//! `scripts/embedding/make_reference.py`).
//!
//! Needs a model and a runtime, so it is skipped unless both are configured:
//!
//! ```text
//! LUMEN_EG2_MODEL_DIR=<copy of onnx-community/embeddinggemma-2-ONNX>
//! LUMEN_ORT_DYLIB=<path to onnxruntime.dll / libonnxruntime.so>
//! LUMEN_EG2_VARIANTS=fp32,q8,q4        # optional
//! LUMEN_EG2_DEVICE=cpu | dml:0         # optional, dml needs --features directml
//! cargo test -p lumen-embedding-ort --release --test fidelity -- --nocapture
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_embedding::{Embedder, EmbeddingProfile, EmbeddingTask, TextInput, dot};
use lumen_embedding_ort::{Device, ModelVariant, OrtBackend, OrtConfig, init_runtime};
use serde_json::Value;

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn min_cosine(variant: ModelVariant) -> f32 {
    match variant {
        // Same weights as the reference: only kernel/threading differences.
        ModelVariant::Fp32 => 0.9999,
        ModelVariant::Fp16 | ModelVariant::Q8 => 0.999,
        // onnx-community reports 0.975 worst case (0.988 text-only) for q4.
        ModelVariant::Q4 | ModelVariant::Q4F16 => 0.97,
    }
}

fn parse_device(s: &str) -> Device {
    match s.strip_prefix("dml:") {
        Some(n) => Device::DirectMl {
            adapter: n.parse().expect("dml:<adapter>"),
        },
        None => Device::Cpu,
    }
}

#[test]
fn matches_python_reference() {
    let (Ok(model_dir), Ok(dylib)) = (
        std::env::var("LUMEN_EG2_MODEL_DIR"),
        std::env::var("LUMEN_ORT_DYLIB"),
    ) else {
        eprintln!("skipped: set LUMEN_EG2_MODEL_DIR and LUMEN_ORT_DYLIB to run");
        return;
    };
    init_runtime(PathBuf::from(dylib).as_path()).expect("load ONNX Runtime");
    let device = parse_device(&std::env::var("LUMEN_EG2_DEVICE").unwrap_or_default());
    let variants: Vec<ModelVariant> = std::env::var("LUMEN_EG2_VARIANTS")
        .unwrap_or_else(|_| "fp32,q8,q4".into())
        .split(',')
        .map(|v| ModelVariant::parse(v.trim()).expect("variant"))
        .collect();

    let corpus: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_file("fixtures/embedding/corpus.json")).unwrap(),
    )
    .unwrap();
    let reference: Value = serde_json::from_str(
        &std::fs::read_to_string(repo_file(
            "fixtures/embedding/reference-eg2-onnx-fp32-d256.json",
        ))
        .unwrap(),
    )
    .unwrap();
    let ref_vec = |id: &str| -> Vec<f32> {
        reference["vectors"][id]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect()
    };

    let queries = corpus["queries"].as_array().unwrap();
    let docs = corpus["documents"].as_array().unwrap();
    let doc_inputs: Vec<TextInput<'_>> = docs
        .iter()
        .map(|d| TextInput::with_title(d["text"].as_str().unwrap(), d["title"].as_str().unwrap()))
        .collect();
    let query_inputs: Vec<TextInput<'_>> = queries
        .iter()
        .map(|q| TextInput::new(q["text"].as_str().unwrap()))
        .collect();

    for variant in variants {
        let mut config = OrtConfig::new(&model_dir, variant, device);
        config.max_batch = 8;
        let backend = Arc::new(OrtBackend::new(config).expect("backend"));
        let embedder = Embedder::new(backend, EmbeddingProfile::DEFAULT).unwrap();

        let qv = embedder
            .embed(EmbeddingTask::SearchQuery, &query_inputs, None)
            .unwrap();
        // Batched (padded) documents...
        let dv = embedder
            .embed(EmbeddingTask::SearchDocument, &doc_inputs, None)
            .unwrap();
        // ...must equal unpadded single-document embeddings.
        for (i, input) in doc_inputs.iter().enumerate().step_by(7) {
            let single = embedder
                .embed(
                    EmbeddingTask::SearchDocument,
                    std::slice::from_ref(input),
                    None,
                )
                .unwrap();
            let c = dot(single.get(0).unwrap(), dv.get(i).unwrap());
            assert!(c > 0.9999, "{variant:?}: padding changed doc {i}: cos {c}");
        }

        let mut cosines = Vec::new();
        for (q, v) in queries.iter().zip(qv.iter()) {
            cosines.push(dot(v, &ref_vec(q["id"].as_str().unwrap())));
        }
        for (d, v) in docs.iter().zip(dv.iter()) {
            cosines.push(dot(v, &ref_vec(d["id"].as_str().unwrap())));
        }
        let min = cosines.iter().copied().fold(f32::INFINITY, f32::min);
        let mean = cosines.iter().sum::<f32>() / cosines.len() as f32;

        let doc_ids: Vec<&str> = docs.iter().map(|d| d["id"].as_str().unwrap()).collect();
        let mut hits = 0;
        let mut ranks: HashMap<&str, usize> = HashMap::new();
        for (q, v) in queries.iter().zip(qv.iter()) {
            let mut scored: Vec<(f32, &str)> = dv
                .iter()
                .zip(&doc_ids)
                .map(|(d, id)| (dot(v, d), *id))
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0));
            let rank = scored
                .iter()
                .position(|(_, id)| *id == q["relevant"].as_str().unwrap())
                .unwrap()
                + 1;
            hits += usize::from(rank == 1);
            ranks.insert(q["id"].as_str().unwrap(), rank);
        }
        let recall1 = hits as f64 / queries.len() as f64;
        eprintln!(
            "{variant:?} on {device}: cos vs fp32 reference min {min:.5} mean {mean:.5}; recall@1 {recall1:.3}"
        );
        assert!(min >= min_cosine(variant), "{variant:?}: min cosine {min}");
        assert!(
            recall1 >= reference["retrieval"]["recall_at_1"].as_f64().unwrap() - 0.05,
            "{variant:?}: recall@1 {recall1} ranks {ranks:?}"
        );
    }
}
