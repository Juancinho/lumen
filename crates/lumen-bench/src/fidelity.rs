//! Fidelity of a backend against committed reference vectors (T006).

use std::path::Path;

use lumen_embedding::{Embedder, EmbeddingTask, TextInput, dot};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FidelityReport {
    pub(crate) reference: String,
    pub(crate) texts: usize,
    pub(crate) min_cosine: f32,
    pub(crate) mean_cosine: f32,
    pub(crate) recall_at_1: f64,
    pub(crate) reference_recall_at_1: f64,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn str_field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("corpus entry without `{key}`"))
}

/// Embeds the corpus with `embedder` and compares against `reference` vectors.
///
/// # Errors
/// Unreadable files, dimension mismatch, embedding failures.
pub(crate) fn evaluate(
    embedder: &Embedder,
    corpus_path: &Path,
    reference_path: &Path,
) -> Result<FidelityReport, String> {
    let corpus = read_json(corpus_path)?;
    let reference = read_json(reference_path)?;
    let dim = reference["dim"].as_u64().ok_or("reference without `dim`")?;
    if usize::try_from(dim).ok() != Some(embedder.profile().dim) {
        return Err(format!(
            "reference is {dim}d but profile is {}d",
            embedder.profile().dim
        ));
    }
    let empty = Vec::new();
    let queries = corpus["queries"].as_array().unwrap_or(&empty);
    let docs = corpus["documents"].as_array().unwrap_or(&empty);

    let mut query_inputs = Vec::with_capacity(queries.len());
    for q in queries {
        query_inputs.push(TextInput::new(str_field(q, "text")?));
    }
    let mut doc_inputs = Vec::with_capacity(docs.len());
    for d in docs {
        doc_inputs.push(TextInput::with_title(
            str_field(d, "text")?,
            str_field(d, "title")?,
        ));
    }
    let qv = embedder
        .embed(EmbeddingTask::SearchQuery, &query_inputs, None)
        .map_err(|e| e.to_string())?;
    let dv = embedder
        .embed(EmbeddingTask::SearchDocument, &doc_inputs, None)
        .map_err(|e| e.to_string())?;

    let ref_vec = |id: &str| -> Result<Vec<f32>, String> {
        #[allow(clippy::cast_possible_truncation)]
        reference["vectors"][id]
            .as_array()
            .ok_or_else(|| format!("reference has no vector for {id}"))?
            .iter()
            .map(|x| {
                x.as_f64()
                    .map(|f| f as f32)
                    .ok_or_else(|| "non-numeric".to_owned())
            })
            .collect()
    };
    let mut cosines = Vec::new();
    for (entry, v) in queries
        .iter()
        .zip(qv.iter())
        .chain(docs.iter().zip(dv.iter()))
    {
        cosines.push(dot(v, &ref_vec(str_field(entry, "id")?)?));
    }

    let doc_ids: Vec<&str> = docs.iter().filter_map(|d| d["id"].as_str()).collect();
    let mut hits = 0_usize;
    for (q, v) in queries.iter().zip(qv.iter()) {
        let best = dv
            .iter()
            .zip(&doc_ids)
            .max_by(|a, b| dot(v, a.0).total_cmp(&dot(v, b.0)))
            .map(|(_, id)| *id);
        if best == q["relevant"].as_str() {
            hits += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let (mean, recall) = (
        cosines.iter().sum::<f32>() / cosines.len().max(1) as f32,
        hits as f64 / queries.len().max(1) as f64,
    );
    Ok(FidelityReport {
        reference: reference_path.display().to_string(),
        texts: cosines.len(),
        min_cosine: cosines.iter().copied().fold(f32::INFINITY, f32::min),
        mean_cosine: mean,
        recall_at_1: recall,
        reference_recall_at_1: reference["retrieval"]["recall_at_1"]
            .as_f64()
            .unwrap_or(f64::NAN),
    })
}
