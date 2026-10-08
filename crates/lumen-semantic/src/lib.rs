//! The semantic query lane (docs/SEARCH_AND_INDEXING.md §2, §4).
//!
//! - [`QueryEmbedder`] (T204): the warm, latest-wins query embedder with a cache and
//!   indexing preemption.
//! - [`SemanticIndex`] (T203): search over a persistent ANN generation — memory-mapped
//!   HNSW file + exact in-memory delta, every hit checked against SQLite — with
//!   [`build_file`], [`validate`] and [`cleanup_files`] for the indexing thread.
//!
//! Fusion with the lexical lanes (T205) builds on both.

#![forbid(unsafe_code)]

mod index;
mod query;

pub use index::{
    FileState, IndexError, IndexSettings, IndexStatus, Maintenance, SemanticHit, SemanticIndex,
    Validation, ann_config, build_file, cleanup_files, file_name, validate,
};
pub use query::{MakeEmbedder, QueryConfig, QueryEmbedder, QueryError, QueryStats};

#[cfg(test)]
mod index_tests;
#[cfg(test)]
mod tests;
