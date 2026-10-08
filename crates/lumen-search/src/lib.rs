//! Root-search coordination (docs/ARCHITECTURE.md §B–§C, docs/COMMAND_MODEL.md §4).
//!
//! - [`Coordinator`]: runs the registered providers for one query in latency-class order,
//!   merges their results and reports each improvement as an [`Update`].
//! - [`SearchService`]: one background thread, latest query wins: a new query cancels the
//!   running one, older query ids are ignored, and nothing is queued per keystroke.
//!
//! No Tauri/WebView types: the shell turns updates into IPC events.

mod coordinator;
mod service;

pub use coordinator::{Coordinator, Outcome, Update, merge};
pub use service::{Request, SearchService};
