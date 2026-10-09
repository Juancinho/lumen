//! Model and runtime provisioning (T210, ADR-034).
//!
//! Lumen ships without the embedding model (174 MB) and, in development builds, without
//! the ONNX Runtime library. This crate installs them on explicit user request:
//!
//! - [`manifest`]: the pinned components — fixed URLs, sizes, SHA-256, licenses;
//! - [`fetch`]: where bytes come from — the system `curl` over HTTPS, or a local folder;
//! - [`install`]: staged, resumable, verified, atomic installation; state; removal.
//!
//! It never decides *whether* to download: the shell asks the user first and shows the
//! component's size, host and license.

#![forbid(unsafe_code)]

pub mod fetch;
pub mod install;
pub mod manifest;
mod zip;

pub use fetch::{CurlFetch, DirFetch, Fetch, FetchError};
pub use install::{
    InstallError, Progress, State, component_dir, install, remove, sha256_file, state, verify,
};
pub use manifest::{
    Component, EMBEDDING_MODEL, GPU_RUNTIME, INFERENCE_RUNTIME, Install, Member, RemoteFile,
    VISION_MODEL,
};

#[cfg(test)]
mod tests;
