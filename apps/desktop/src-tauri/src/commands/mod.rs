//! Tauri IPC commands, grouped by feature (one module per area; avoid a single
//! command file). Commands are thin: call into core/application code, map the
//! result to a DTO, return.
//!
//! Commands are `async` so Tauri runs them off the main (UI/event-loop) thread.
//! Never perform blocking disk, database or inference work in a sync command.

pub(crate) mod actions;
pub(crate) mod app_info;
pub(crate) mod overlay;
pub(crate) mod search;
