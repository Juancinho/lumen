//! The semantic query lane (docs/SEARCH_AND_INDEXING.md §2, §4).
//!
//! - [`QueryEmbedder`] (T204): the warm, latest-wins query embedder with a cache and
//!   indexing preemption.
//!
//! Vector search over index generations (T203) and fusion with the lexical lanes (T205)
//! build on it.

#![forbid(unsafe_code)]

mod query;

pub use query::{MakeEmbedder, QueryConfig, QueryEmbedder, QueryError, QueryStats};

#[cfg(test)]
mod tests;
