//! `cargo xtask bench [--quick] [--out DIR]`: the release-mode benchmark suite (T010).
//!
//! Runs every `lumen-bench` subcommand that needs no downloaded model, in release mode, and
//! writes one JSON report per benchmark into `--out` (default `target/bench/<quick|full>`).
//! CI runs `--quick` on every push and uploads the reports as artifacts; it does not gate on
//! numbers yet (docs/PERFORMANCE.md §12: fail only on stable, meaningful regressions).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

struct Bench {
    name: &'static str,
    args: Vec<String>,
}

fn suite(quick: bool, repo: &Path, work: &Path) -> Vec<Bench> {
    let s = |v: &[&str]| v.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    let path = |p: &Path| p.display().to_string();
    let mut scan = s(&["scan", "--root"]);
    scan.push(path(repo));
    for name in ["target", "node_modules", ".git", ".cache", "worktrees"] {
        scan.push("--exclude-name".into());
        scan.push(name.into());
    }
    scan.extend(s(&["--identity", "--repeat", "2", "--show-issues", "0"]));

    let mut ann = if quick {
        s(&[
            "ann",
            "--sizes",
            "20000",
            "--scalars",
            "f16",
            "--efs",
            "64,256",
            "--queries",
            "200",
        ])
    } else {
        s(&[
            "ann",
            "--sizes",
            "100000",
            "--scalars",
            "f32,f16",
            "--efs",
            "32,64,128,256",
            "--queries",
            "500",
        ])
    };
    ann.extend(["--work-dir".into(), path(&work.join("ann"))]);

    let mut storage = s(&[
        "storage",
        "--chunks",
        if quick { "20000" } else { "100000" },
    ]);
    storage.extend(["--work-dir".into(), path(&work.join("storage"))]);

    let mut ann_gen = s(&[
        "ann-gen",
        "--vectors",
        if quick { "20000" } else { "100000" },
        "--delta",
        if quick { "2000" } else { "5000" },
    ]);
    ann_gen.extend(["--work-dir".into(), path(&work.join("ann-gen"))]);

    let mut identity = s(&["identity-check", "--dir"]);
    identity.push(path(work));

    vec![
        Bench {
            name: "embed-mock",
            args: if quick {
                s(&["embed", "--iterations", "50", "--docs", "64"])
            } else {
                s(&["embed"])
            },
        },
        Bench {
            name: "ann",
            args: ann,
        },
        Bench {
            name: "ann-gen",
            args: ann_gen,
        },
        Bench {
            name: "storage",
            args: storage,
        },
        Bench {
            name: "scan-repo",
            args: scan,
        },
        Bench {
            name: "identity-check",
            args: identity,
        },
    ]
}

pub(crate) fn run(args: impl Iterator<Item = String>) -> ExitCode {
    let mut quick = false;
    let mut out: Option<PathBuf> = None;
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--quick" => quick = true,
            "--out" => match args.next() {
                Some(dir) => out = Some(dir.into()),
                None => {
                    eprintln!("bench: --out needs a directory");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("bench: unknown option `{other}` (expected --quick, --out DIR)");
                return ExitCode::FAILURE;
            }
        }
    }

    let repo = repo_root();
    let out = out.unwrap_or_else(|| {
        repo.join("target")
            .join("bench")
            .join(if quick { "quick" } else { "full" })
    });
    let work = std::env::temp_dir().join(format!("lumen-xtask-bench-{}", std::process::id()));
    if let Err(err) = std::fs::create_dir_all(&out).and(std::fs::create_dir_all(&work)) {
        eprintln!("bench: create output dirs: {err}");
        return ExitCode::FAILURE;
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    println!("bench: building lumen-bench (release)");
    let built = Command::new(&cargo)
        .current_dir(&repo)
        .args(["build", "--release", "--locked", "-p", "lumen-bench"])
        .status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("bench: release build failed");
        return ExitCode::FAILURE;
    }
    let exe = target_dir(&repo)
        .join("release")
        .join(format!("lumen-bench{}", std::env::consts::EXE_SUFFIX));

    let mut failed = Vec::new();
    for bench in suite(quick, &repo, &work) {
        let json = out.join(format!("{}.json", bench.name));
        println!("bench: {} -> {}", bench.name, json.display());
        let started = Instant::now();
        let status = Command::new(&exe)
            .args(&bench.args)
            .arg("--label")
            .arg(format!(
                "xtask bench {}",
                if quick { "quick" } else { "full" }
            ))
            .arg("--json")
            .arg(&json)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!(
                    "bench: {} ok in {:.1} s",
                    bench.name,
                    started.elapsed().as_secs_f64()
                );
            }
            Ok(s) => failed.push(format!("{} ({s})", bench.name)),
            Err(err) => failed.push(format!("{} ({err})", bench.name)),
        }
    }
    let _ = std::fs::remove_dir_all(&work);

    if failed.is_empty() {
        println!("bench: all reports in {}", out.display());
        ExitCode::SUCCESS
    } else {
        eprintln!("bench: failed: {}", failed.join(", "));
        ExitCode::FAILURE
    }
}

/// The workspace root: xtask lives at `<root>/xtask`.
fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map_or(manifest.clone(), Path::to_path_buf)
}

fn target_dir(repo: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| repo.join("target"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_suite_covers_every_model_free_benchmark() {
        let suite = suite(true, Path::new("repo"), Path::new("work"));
        let names: Vec<_> = suite.iter().map(|b| b.name).collect();
        assert_eq!(
            names,
            [
                "embed-mock",
                "ann",
                "ann-gen",
                "storage",
                "scan-repo",
                "identity-check"
            ]
        );
        let scan = &suite[4].args;
        assert!(
            scan.windows(2)
                .any(|w| w[0] == "--exclude-name" && w[1] == "target")
        );
        assert!(suite[1].args.contains(&"20000".to_owned()));
    }
}
