//! The semantic query lane (docs/SEARCH_AND_INDEXING.md §2, §4).
//!
//! - [`QueryEmbedder`] (T204): the warm, latest-wins query embedder with a cache and
//!   indexing preemption.
//! - [`SemanticIndex`] (T203): search over a persistent ANN generation — memory-mapped
//!   HNSW file + exact in-memory delta, every hit checked against SQLite — with
//!   [`build_file`], [`validate`] and [`cleanup_files`] for the indexing thread.
//!
//! - [`SemanticProvider`] (T205): both behind the root-search provider contract, fused
//!   with the name and content lanes by `lumen-search`.

#![forbid(unsafe_code)]

mod index;
mod provider;
mod query;

pub use index::{
    FileState, IndexError, IndexSettings, IndexStatus, Maintenance, SemanticHit, SemanticIndex,
    Validation, ann_config, build_file, cleanup_files, file_name, validate,
};
pub use provider::{SEMANTIC_PROVIDER_ID, SemanticConfig, SemanticProvider, SharedIndex};
pub use query::{MakeEmbedder, QueryConfig, QueryEmbedder, QueryError, QueryStats};

#[cfg(test)]
mod index_tests;
#[cfg(test)]
mod tests;
