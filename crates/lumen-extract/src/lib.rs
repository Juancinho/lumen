//! Text and code extraction for indexing (docs/SEARCH_AND_INDEXING.md §9, T201).
//!
//! - [`kind_for_extension`]: which files are text, and how to chunk them.
//! - [`extract_file`] / [`decode`]: bounded reads, BOM/UTF-8/UTF-16 detection, Windows-1252
//!   fallback for legacy text, binary rejection; every skip has a reason (coverage, ADR-018).
//! - [`chunk`]: retrieval chunks of ~[`ChunkConfig::target_tokens`] tokens with byte offsets
//!   into the extracted text — paragraphs and sentences for prose, heading sections and intact
//!   fences for Markdown, top-level definitions (with symbol names) for code, line windows
//!   for data. Token counts come from a [`TokenCount`]; T202 plugs the model's tokenizer in,
//!   [`EstimateTokens`] is the dependency-free default.
//!
//! No Tree-sitter yet: indentation/keyword heuristics cover the supported languages without
//! native grammars in the build. T209 revisits that with relevance evidence.

#![forbid(unsafe_code)]

mod chunk;
mod decode;
mod indexed;
mod kinds;
mod pdf;

pub use chunk::{Chunk, ChunkConfig, ChunkKind, EstimateTokens, TokenCount, chunk};
pub use decode::{DEFAULT_MAX_BYTES, ExtractError, Extracted, Skip, decode, extract_file};
pub use indexed::{IndexError, IndexedChunk, IndexedDocument, extract_indexed_file};
pub use kinds::{DocKind, Language, TEXT_EXTENSIONS, kind_for_extension};
pub use pdf::{PdfError, PdfLimits, extract_pdf};

/// Version of what extraction + chunking produce for a file. Bump it whenever the chunks of
/// an unchanged file would differ: the content pass then re-reads every file (stored as
/// `items.extractor_version`), and it is part of the index generation (ADR-029).
pub const EXTRACTOR_VERSION: u32 = 1;
