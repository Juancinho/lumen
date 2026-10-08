//! Windows OS adapters used by core crates (docs/ARCHITECTURE.md §16). No GUI, WebView or
//! shell (Tauri) types: the presentation shell keeps its own Windows code.
//!
//! Unsafe code is allowed only inside the modules that call Win32/COM, each call with a
//! SAFETY note.

#![deny(unsafe_code)]

pub mod apps;

pub use apps::{StartApp, start_apps};
