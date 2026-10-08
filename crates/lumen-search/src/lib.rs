//! Root-search coordination (docs/ARCHITECTURE.md §20.B–C, docs/COMMAND_MODEL.md §4).
//!
//! - [`Coordinator`]: runs the registered providers for one query in latency-class order,
//!   fuses their results (weighted reciprocal-rank fusion, ADR-032) and reports each
//!   improvement as an [`Update`].
//! - [`SearchService`]: one background thread, latest query wins: a new query cancels the
//!   running one, older query ids are ignored, and nothing is queued per keystroke.
//!
//! - [`available`] / [`prepare`]: a result's contextual actions and the policy check before
//!   an executor runs one (results are looked up with [`SearchService::lookup`]).
//!
//! No Tauri/WebView types: the shell turns updates into IPC events.

mod actions;
mod coordinator;
mod service;

pub use actions::{ActionError, available, prepare};
pub use coordinator::{Coordinator, Outcome, RRF_K, SETTLED_BATCH, Update, fuse};
pub use service::{DEFAULT_SETTLE, Request, SearchService};
