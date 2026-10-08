//! The app/file catalog (T101): Pass 0 inventory and the Start-menu application list are
//! written to the item store, and [`CatalogProvider`] answers name queries instantly.

#![forbid(unsafe_code)]

pub mod apps;
pub mod files;
pub mod path;
pub mod provider;
pub mod text;

pub use apps::{AppsReport, sync_apps};
pub use files::{FilesReport, sync_files};
pub use provider::CatalogProvider;
