//! Indexing pipeline, starting with Pass 0: inventory (docs/SEARCH_AND_INDEXING.md §21).
//!
//! **Coverage guarantee (T009):** every file system entry under an indexed root is either
//! emitted as a [`ScanEntry`] (at least path + name, even when its metadata cannot be read) or
//! accounted for in the [`ScanReport`] — as an exclusion with the rule that matched, or as a
//! [`ScanIssue`] with a reason. Nothing is dropped silently.

#![forbid(unsafe_code)]

pub mod identity;
pub mod scan;
#[cfg(windows)]
pub mod winpath;

pub use identity::{FileIdentity, identity_of};
pub use scan::{
    EntryFlags, EntryKind, Excluded, Exclusions, IssueKind, IssueStage, SYSTEM_EXCLUSIONS,
    ScanEntry, ScanIssue, ScanOptions, ScanReport, scan,
};
