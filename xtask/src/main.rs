//! Repository tooling. Run via the cargo alias: `cargo xtask <command>`.

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // a CLI reporting to the terminal

mod arch;
mod bench;

use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
  arch    Verify the shell -> core dependency direction (ADR-002)
  bench   Release-mode benchmark suite, JSON reports in target/bench/ (T010)
          [--quick] [--out DIR]
";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("arch") => run_arch(),
        Some("bench") => bench::run(args),
        Some("-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
        None => {
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run_arch() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    // No `--filter-platform`: Windows-only dependencies must be checked on every host.
    let output = match Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!("arch: failed to run `cargo metadata`: {err}");
            return ExitCode::FAILURE;
        }
    };
    if !output.status.success() {
        eprintln!(
            "arch: `cargo metadata` failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return ExitCode::FAILURE;
    }
    let metadata: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("arch: could not parse `cargo metadata` output: {err}");
            return ExitCode::FAILURE;
        }
    };

    match arch::check(&metadata) {
        Ok(report) if report.violations.is_empty() => {
            println!(
                "arch: OK - {} core crate(s) are free of shell dependencies: {}",
                report.core_crates.len(),
                report.core_crates.join(", ")
            );
            ExitCode::SUCCESS
        }
        Ok(report) => {
            eprintln!("arch: dependency-direction violations (ADR-002):");
            for violation in &report.violations {
                eprintln!("  - {violation}");
            }
            eprintln!(
                "\nCrates under `crates/` must not depend on presentation-shell crates \
                 (Tauri/WebView/GUI toolkits) or on anything under `apps/`.\n\
                 Move the code into the shell, or put an interface in core and implement it in the shell."
            );
            ExitCode::FAILURE
        }
        Err(err) => {
            eprintln!("arch: {err}");
            ExitCode::FAILURE
        }
    }
}
