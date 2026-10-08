//! Lumen domain core.
//!
//! This crate (and every crate under `crates/`) is shell-agnostic. It must never
//! depend on Tauri, React, WebView2/WebKit or any other presentation technology,
//! so that the current Tauri + React shell can be replaced without rewriting
//! search, indexing, providers or actions (ADR-002). The rule is enforced by
//! `cargo xtask arch`, not by convention.
//!
//! # Universal command model (T011, ADR-007)
//!
//! - [`ids`]: [`ProviderId`], [`ActionId`], [`ResultId`], [`QueryId`].
//! - [`result`]: [`ResultItem`] — the one result type every provider returns.
//! - [`capability`]: what a result's target supports ([`CapabilitySet`]).
//! - [`action`]: [`ActionDescriptor`] metadata, risk class and panel grouping.
//! - [`contract`]: [`validate_result`], the provider contract check.
//! - [`execution`]: [`ActionRequest`] → [`ExecutionContext`] authorization, and
//!   [`CancellationToken`].
//!
//! - [`provider`]: the [`Provider`] contract and [`LatencyClass`] (T101).
//! - [`builtin`]: built-in action ids and descriptors (open, launch, reveal, copy path).
//!
//! Deliberately absent (owned by later tasks): provider registry/coordinator (T107/T401),
//! action registry and executors (T108/T109), preview references (T105), workflow
//! permissions (T501), serialization (shell DTOs, ADR-013).

#![forbid(unsafe_code)]

pub mod action;
mod build_info;
pub mod builtin;
pub mod capability;
pub mod contract;
pub mod execution;
pub mod ids;
pub mod provider;
pub mod result;

pub use action::{ActionDescriptor, ActionGroup, ActionLookup, ActionSafety};
pub use build_info::{CoreInfo, core_info};
pub use capability::{Capability, CapabilitySet};
pub use contract::{ContractViolation, validate_result};
pub use execution::{
    ActionRequest, AuthorizationError, CancellationToken, ExecutionContext, Invocation,
};
pub use ids::{ActionId, IdError, ProviderId, QueryId, ResultId};
pub use provider::{LatencyClass, Provider, ProviderError, ProviderQuery};
pub use result::{Confidence, IconRef, MatchKind, Payload, ResultItem, ResultKind, Score};
