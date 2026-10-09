//! Text-layer PDF extraction only: no rendering, OCR, JavaScript or external resources.
//! Bounds and offsets are defined in ADR-039 and specs/T301-pdf-text.md.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use lopdf::{DecompressError, Document, LoadOptions};

use crate::{ChunkConfig, DocKind, Extracted, IndexedChunk, IndexedDocument, TokenCount, chunk};

#[derive(Debug, Clone, Copy)]
pub struct PdfLimits {
    pub max_file_bytes: u64,
    pub max_pages: u32,
    /// Per object/xref stream at load; total page content and each font CMap at extraction.
    pub max_stream_bytes: usize,
    pub max_text_bytes: usize,
    /// Cooperative deadline, checked around loading and each page; not a hard interrupt.
    pub max_elapsed: Duration,
}

impl Default for PdfLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_pages: 512,
            max_stream_bytes: 4 * 1024 * 1024,
            max_text_bytes: 4 * 1024 * 1024,
            max_elapsed: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    TooLarge,
    TooManyPages,
    DecompressionLimit,
    TextLimit,
    TimeBudget,
    Encrypted,
    NoText,
    Malformed,
    Decode,
    Cancelled,
    Io(std::io::ErrorKind),
}

impl PdfError {
    /// Stable coverage codes, never parser details, filenames or document text.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "pdf:too_large",
            Self::TooManyPages => "pdf:page_limit",
            Self::DecompressionLimit => "pdf:stream_limit",
            Self::TextLimit => "pdf:text_limit",
            Self::TimeBudget => "pdf:time_budget",
            Self::Encrypted => "pdf:encrypted",
            Self::NoText => "pdf:no_text",
            Self::Malformed => "pdf:malformed",
            Self::Decode => "pdf:decode_failed",
            Self::Cancelled => "pdf:cancelled",
            Self::Io(_) => "pdf:io",
        }
    }
}
impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for PdfError {}

/// Extract physical page numbers and prose chunks using a bounded, single-thread parser.
/// An unreadable page rejects the file so coverage cannot claim a complete partial index.
///
/// # Errors
/// [`PdfError`] identifies input limits, cancellation, missing text and decoding failures.
pub fn extract_pdf(
    path: &Path,
    limits: &PdfLimits,
    config: &ChunkConfig,
    counter: &dyn TokenCount,
    cancelled: &dyn Fn() -> bool,
) -> Result<IndexedDocument, PdfError> {
    let started = Instant::now();
    let check = || {
        if cancelled() {
            Err(PdfError::Cancelled)
        } else if started.elapsed() >= limits.max_elapsed {
            Err(PdfError::TimeBudget)
        } else {
            Ok(())
        }
    };
    check()?;
    let file = std::fs::File::open(path).map_err(|e| PdfError::Io(e.kind()))?;
    if file.metadata().map_err(|e| PdfError::Io(e.kind()))?.len() > limits.max_file_bytes {
        return Err(PdfError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(limits.max_file_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| PdfError::Io(e.kind()))?;
    if bytes.len() as u64 > limits.max_file_bytes {
        return Err(PdfError::TooLarge);
    }
    check()?;
    // The parser has recursion/reference limits; also bound eager object/xref decompression.
    let document = Document::load_mem_with_options(
        &bytes,
        LoadOptions {
            strict: true,
            max_decompressed_size: Some(limits.max_stream_bytes),
            ..LoadOptions::default()
        },
    )
    .map_err(|e| match e {
        lopdf::Error::InvalidPassword | lopdf::Error::Decryption(_) => PdfError::Encrypted,
        lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. }) => {
            PdfError::DecompressionLimit
        }
        _ => PdfError::Malformed,
    })?;
    if document.is_encrypted() || document.was_encrypted() {
        return Err(PdfError::Encrypted);
    }
    check()?;
    let pages = document
        .page_iter()
        .take(limits.max_pages as usize + 1)
        .count();
    if pages == 0 {
        return Err(PdfError::Malformed);
    }
    if pages > limits.max_pages as usize {
        return Err(PdfError::TooManyPages);
    }
    let mut doc = Extracted {
        kind: DocKind::Prose,
        text: String::new(),
        encoding: "pdf",
        lossy: false,
    };
    let mut chunks = Vec::new();
    for page_number in 1..=pages as u32 {
        check()?;
        let text = document
            .extract_text_with_limit(&[page_number], limits.max_stream_bytes)
            .map_err(|e| match e {
                lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. }) => {
                    PdfError::DecompressionLimit
                }
                _ => PdfError::Decode,
            })?;
        check()?;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if doc.text.len().saturating_add(text.len()).saturating_add(2) > limits.max_text_bytes {
            return Err(PdfError::TextLimit);
        }
        let page = Extracted {
            kind: DocKind::Prose,
            text,
            encoding: "pdf",
            lossy: false,
        };
        let base = doc.text.len();
        for mut c in chunk(&page, config, counter) {
            c.ordinal = u32::try_from(chunks.len()).map_err(|_| PdfError::TextLimit)?;
            c.start += base;
            c.end += base;
            chunks.push(IndexedChunk {
                chunk: c,
                page_number: Some(page_number),
            });
        }
        doc.text.push_str(&page.text);
        doc.text.push_str("\n\n");
        check()?;
    }
    if chunks.is_empty() {
        return Err(PdfError::NoText);
    }
    Ok(IndexedDocument { doc, chunks })
}

#[cfg(test)]
#[path = "pdf_tests.rs"]
mod tests;
