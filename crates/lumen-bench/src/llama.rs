//! `--backend llama-server` (T014): EmbeddingGemma 2 GGUF served by llama.cpp's
//! `llama-server --embedding`, reached over plain HTTP on localhost, so the same embed /
//! fidelity / probe harness measures llama.cpp builds (CPU, Vulkan, CUDA) without linking
//! them. Dev-only: Lumen never ships a server.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use lumen_embedding::{
    Capabilities, EmbeddingBackend, EmbeddingError, ExecutionTarget, Modality, ModalitySet,
    ModelInfo,
};

const NATIVE_DIM: usize = 768;

#[derive(Debug, Clone, Default)]
pub(crate) struct LlamaOptions {
    /// `host:port` of a running `llama-server --embedding` (default `127.0.0.1:8080`).
    pub(crate) addr: Option<String>,
    /// Weights label for the index space, e.g. `gguf-q8_0` (different weights, different
    /// space: ADR-014).
    pub(crate) variant: Option<String>,
    /// `cpu` or `gpu` (what the server build runs on; reports only).
    pub(crate) target: Option<String>,
}

pub(crate) struct LlamaServerBackend {
    addr: String,
    caps: Capabilities,
}

impl LlamaServerBackend {
    pub(crate) fn new(o: &LlamaOptions) -> Self {
        let target = match o.target.as_deref() {
            Some("gpu") => ExecutionTarget::Gpu,
            _ => ExecutionTarget::Cpu,
        };
        Self {
            addr: o.addr.clone().unwrap_or_else(|| "127.0.0.1:8080".into()),
            caps: Capabilities {
                backend: format!("llama.cpp-server-{}", target.as_str()),
                runtime_version: None,
                model: ModelInfo {
                    id: "embeddinggemma-2".into(),
                    revision: o.variant.clone().unwrap_or_else(|| "gguf-q8_0".into()),
                    native_dim: NATIVE_DIM,
                    matryoshka_dims: vec![128, 256, 512, 768],
                    max_input_tokens: 2048,
                    modalities: ModalitySet::TEXT,
                },
                target,
                device: None,
                max_batch: 16,
                concurrent_calls: false,
                preprocessing_version: 1,
            },
        }
    }

    fn post(&self, path: &str, body: &str) -> Result<String, String> {
        let mut stream =
            TcpStream::connect(&self.addr).map_err(|e| format!("connect {}: {e}", self.addr))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(300)))
            .map_err(|e| e.to_string())?;
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.addr,
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&raw);
        let (head, payload) = text
            .split_once("\r\n\r\n")
            .ok_or("malformed HTTP response")?;
        let status = head.lines().next().unwrap_or_default();
        if !status.contains(" 200") {
            return Err(format!("llama-server answered `{status}`"));
        }
        let body = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(payload)?
        } else {
            payload.to_owned()
        };
        Ok(body)
    }
}

/// Decodes an HTTP/1.1 chunked body.
fn dechunk(mut s: &str) -> Result<String, String> {
    let mut out = String::new();
    loop {
        let (size, rest) = s.split_once("\r\n").ok_or("bad chunk")?;
        let n = usize::from_str_radix(size.trim(), 16).map_err(|_| "bad chunk size")?;
        if n == 0 {
            return Ok(out);
        }
        out.push_str(rest.get(..n).ok_or("short chunk")?);
        s = rest.get(n + 2..).ok_or("short chunk")?;
    }
}

/// Embeddings from an OpenAI-style `/v1/embeddings` answer, ordered by `index`.
pub(crate) fn parse_embeddings(body: &str, expected: usize) -> Result<Vec<f32>, EmbeddingError> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| EmbeddingError::Backend(format!("json: {e}")))?;
    let data = v["data"]
        .as_array()
        .ok_or_else(|| EmbeddingError::Backend("no `data` in answer".into()))?;
    let mut rows: Vec<(u64, Vec<f32>)> = data
        .iter()
        .map(|d| {
            let index = d["index"].as_u64().unwrap_or(0);
            #[allow(clippy::cast_possible_truncation)]
            let emb = d["embedding"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_f64)
                        .map(|x| x as f32)
                        .collect()
                })
                .unwrap_or_default();
            (index, emb)
        })
        .collect();
    rows.sort_by_key(|(i, _)| *i);
    let out: Vec<f32> = rows.into_iter().flat_map(|(_, e)| e).collect();
    if out.len() != expected * NATIVE_DIM {
        return Err(EmbeddingError::OutputShape {
            expected: expected * NATIVE_DIM,
            actual: out.len(),
        });
    }
    Ok(out)
}

impl EmbeddingBackend for LlamaServerBackend {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn warm(&self, modality: Modality) -> Result<(), EmbeddingError> {
        if modality != Modality::Text {
            return Err(EmbeddingError::Unsupported(modality));
        }
        // The server loaded the model at start; one tiny request checks it answers.
        self.embed_text(&["warm"]).map(|_| ())
    }

    fn unload(&self, _modality: Modality) -> Result<(), EmbeddingError> {
        Ok(())
    }

    fn is_warm(&self, modality: Modality) -> bool {
        modality == Modality::Text
    }

    fn embed_text(&self, inputs: &[&str]) -> Result<Vec<f32>, EmbeddingError> {
        let body = serde_json::json!({ "input": inputs, "encoding_format": "float" }).to_string();
        let answer = self
            .post("/v1/embeddings", &body)
            .map_err(EmbeddingError::Backend)?;
        parse_embeddings(&answer, inputs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_style_answers_in_index_order() {
        let row = |i: usize, x: f32| serde_json::json!({ "index": i, "embedding": vec![x; NATIVE_DIM], "object": "embedding" });
        let body = serde_json::json!({ "data": [row(1, 2.0), row(0, 1.0)] }).to_string();
        let v = parse_embeddings(&body, 2).unwrap();
        assert_eq!(v.len(), 2 * NATIVE_DIM);
        assert!((v[0] - 1.0).abs() < 1e-6 && (v[NATIVE_DIM] - 2.0).abs() < 1e-6);
        assert!(matches!(
            parse_embeddings(&body, 3),
            Err(EmbeddingError::OutputShape { .. })
        ));
        assert_eq!(
            dechunk("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap(),
            "abcde"
        );
    }

    #[test]
    fn talks_http_to_a_server() {
        // A one-shot fake llama-server on a random port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body =
                serde_json::json!({ "data": [{ "index": 0, "embedding": vec![0.5; NATIVE_DIM] }] })
                    .to_string();
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            req
        });
        let b = LlamaServerBackend::new(&LlamaOptions {
            addr: Some(addr),
            ..LlamaOptions::default()
        });
        assert_eq!(b.embed_text(&["hola"]).unwrap().len(), NATIVE_DIM);
        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/embeddings"));
        assert!(req.contains("\"hola\""));
    }
}
