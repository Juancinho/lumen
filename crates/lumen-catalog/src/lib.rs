//! The app/file catalog (T101): Pass 0 inventory and the Start-menu application list are
//! written to the item store, and [`CatalogProvider`] answers name queries instantly;
//! [`ContentProvider`] answers settled queries from file contents (T205).

#![forbid(unsafe_code)]

pub mod apps;
pub mod code;
pub mod content;
pub mod files;
pub mod locations;
pub mod path;
pub mod pdf;
pub mod provider;
pub mod rank;
pub mod text;
pub mod usage;

pub use apps::{AppsReport, sync_apps};
pub use content::{CONTENT_PROVIDER_ID, ContentProvider};
pub use files::{
    FilesReport, sync_changes, sync_changes_with_content_scope, sync_files,
    sync_files_with_progress,
};
pub use locations::{IndexLocations, LocationState, location_states};
pub use provider::CatalogProvider;
pub use usage::record_action;
