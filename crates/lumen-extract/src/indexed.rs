//! Shared representation for text/code and page-bounded PDF chunks (T301).

use std::path::Path;

use crate::{Chunk, ChunkConfig, ExtractError, Extracted, PdfError, PdfLimits, TokenCount, chunk};

#[derive(Debug, Clone)]
pub struct IndexedChunk {
    pub chunk: Chunk,
    /// Physical, one-based PDF page, including blank pages. None for ordinary text/code.
    pub page_number: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct IndexedDocument {
    pub doc: Extracted,
    pub chunks: Vec<IndexedChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    Text(ExtractError),
    Pdf(PdfError),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(err) => err.fmt(f),
            Self::Pdf(err) => err.fmt(f),
        }
    }
}
impl std::error::Error for IndexError {}

/// Extract with the unchanged text/code chunker, or independently chunk each PDF page.
/// Cancellation never returns a partially indexed PDF.
///
/// # Errors
/// Typed read/skip/parse/limit errors; no parser messages containing document content.
pub fn extract_indexed_file(
    path: &Path,
    max_text_bytes: u64,
    pdf: &PdfLimits,
    config: &ChunkConfig,
    counter: &dyn TokenCount,
    cancelled: &dyn Fn() -> bool,
) -> Result<IndexedDocument, IndexError> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        return crate::extract_pdf(path, pdf, config, counter, cancelled).map_err(IndexError::Pdf);
    }
    let doc = crate::extract_file(path, max_text_bytes).map_err(IndexError::Text)?;
    let chunks = chunk(&doc, config, counter)
        .into_iter()
        .map(|chunk| IndexedChunk {
            chunk,
            page_number: None,
        })
        .collect();
    Ok(IndexedDocument { doc, chunks })
}
