//! The app/file catalog (T101): Pass 0 inventory and the Start-menu application list are
//! written to the item store, and [`CatalogProvider`] answers name queries instantly.

#![forbid(unsafe_code)]

pub mod apps;
pub mod files;
pub mod locations;
pub mod path;
pub mod provider;
pub mod rank;
pub mod text;
pub mod usage;

pub use apps::{AppsReport, sync_apps};
pub use files::{FilesReport, sync_files, sync_files_with_progress};
pub use locations::{IndexLocations, LocationState, location_states};
pub use provider::CatalogProvider;
pub use usage::record_action;
