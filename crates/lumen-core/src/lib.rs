//! Lumen domain core.
//!
//! This crate (and every crate under `crates/`) is shell-agnostic. It must never
//! depend on Tauri, React, WebView2/WebKit or any other presentation technology,
//! so that the current Tauri + React shell can be replaced without rewriting
//! search, indexing, providers or actions (ADR-002). The rule is enforced by
//! `cargo xtask arch`, not by convention.
//!
//! Domain contracts (`ResultItem`, `ProviderId`, `ActionDescriptor`, ...) arrive
//! with T011; this crate intentionally starts minimal.

#![forbid(unsafe_code)]

mod build_info;

pub use build_info::{CoreInfo, core_info};
