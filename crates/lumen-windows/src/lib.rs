//! Windows OS adapters (docs/ARCHITECTURE.md §16). No GUI framework, WebView or shell
//! (Tauri) types: window handles cross the API as plain integers, and WebView2 code stays in
//! the presentation shell.
//!
//! Unsafe code is allowed only inside the modules that call Win32/COM, each call with a
//! SAFETY note.

#![deny(unsafe_code)]

pub mod apps;
pub mod material;
pub mod process;
pub mod system;

pub use apps::{StartApp, start_apps};
