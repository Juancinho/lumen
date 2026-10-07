//! `cargo xtask test [args]`: the workspace test suite in two cargo invocations.
//!
//! The Tauri shell's build script (tauri-build, static VC runtime) writes an empty
//! `msvcrt.lib` into its OUT_DIR and adds that directory to the native search path. Cargo
//! hands every native search path of one invocation to rustdoc, so in a plain
//! `cargo test --workspace` on Windows the doctests of *other* crates (e.g. `lumen-core`) link
//! against the empty library and fail (`__CxxFrameHandler3`, `memmove`, `mainCRTStartup`
//! unresolved). Testing the shell in its own invocation keeps the static runtime for releases
//! and the doctests working everywhere.

use std::process::{Command, ExitCode};

/// The presentation shell crate (apps/desktop/src-tauri).
const SHELL: &str = "lumen-desktop";

pub(crate) fn run(args: impl Iterator<Item = String>) -> ExitCode {
    let extra: Vec<String> = args.collect();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    for (what, scope) in [
        (
            "workspace without the shell",
            vec!["--workspace", "--exclude", SHELL],
        ),
        ("shell", vec!["-p", SHELL]),
    ] {
        println!("test: {what}");
        let status = Command::new(&cargo)
            .arg("test")
            .args(&scope)
            .args(&extra)
            .status();
        if !status.is_ok_and(|s| s.success()) {
            eprintln!("test: {what} failed");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
