//! Lumen benchmark harness. Always run release builds for evidence:
//!
//! ```text
//! cargo run --release -p lumen-bench -- embed --json target/bench/embed-mock.json
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output

mod corpus;
mod embed;
mod fidelity;
mod machine;
mod stats;

use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
Usage: lumen-bench <command> [options]

Commands:
  embed     Embedding backend latency/throughput/memory (T005/T006)

embed options:
  --backend NAME         backend to measure (default: mock)
  --dim N                output dimension (default: 256)
  --warmup N             unmeasured warm-up queries (default: 20)
  --iterations N         measured queries (default: 200)
  --batch-sizes A,B,..   document batch sizes (default: 1,8,32)
  --docs N               synthetic document chunks (default: 256)
  --doc-words N          words per chunk (default: 200)
  --queries FILE         one query per line instead of the built-in set
  --long-words N         words of the long-input probe, 0 = off (default: 100 ≈ 128 tokens)
  --reference FILE       fidelity check against reference vectors (see fixtures/embedding/)
  --corpus FILE          corpus for --reference (default: fixtures/embedding/corpus.json)
  --label TEXT           free-form note stored in the report (machine, power state...)
  --json PATH            write the JSON report to PATH (default: stdout)
  --mock-load-ms N       mock: simulated model load
  --mock-call-ms N       mock: simulated per-call latency
  --mock-item-ms N       mock: simulated per-input latency

ort backend (build with --features ort, or directml on Windows):
  --ort-dylib PATH       onnxruntime.dll / libonnxruntime.so to load
  --model-dir DIR        copy of onnx-community/embeddinggemma-2-ONNX
  --variant NAME         fp32 | fp16 | q8 | q4 | q4f16 (default: q4)
  --device NAME          cpu | dml:<adapter> | dml:high | dml:low (default: cpu)
  --threads N            intra-op threads (default: runtime default)
  --placement            report which execution provider runs each graph node
  --no-cpu-fallback      fail instead of running unsupported GPU nodes on CPU
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("embed") => match parse_embed(&args[1..]).and_then(|(opts, json)| {
            let report = embed::run(&opts)?;
            Ok((report, json))
        }) {
            Ok((report, json_path)) => {
                eprint!("{}", embed::summarize(&report));
                let json = match serde_json::to_string_pretty(&report) {
                    Ok(json) => json,
                    Err(err) => {
                        eprintln!("error: serialize report: {err}");
                        return ExitCode::FAILURE;
                    }
                };
                match json_path {
                    Some(path) => {
                        if let Some(parent) = std::path::Path::new(&path).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(err) = std::fs::write(&path, json + "\n") {
                            eprintln!("error: write {path}: {err}");
                            return ExitCode::FAILURE;
                        }
                        eprintln!("  report: {path}");
                    }
                    None => println!("{json}"),
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("error: {err}\n\n{USAGE}");
                ExitCode::FAILURE
            }
        },
        Some("-h" | "--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn parse_embed(args: &[String]) -> Result<(embed::EmbedOptions, Option<String>), String> {
    let mut opts = embed::EmbedOptions::default();
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        let num = |v: String| {
            v.parse::<usize>()
                .map_err(|_| format!("{flag}: `{v}` is not a non-negative integer"))
        };
        let millis = |v: String| num(v).map(|n| Duration::from_millis(n as u64));
        match flag.as_str() {
            "--backend" => opts.backend = value()?,
            "--dim" => opts.dim = num(value()?)?,
            "--warmup" => opts.warmup = num(value()?)?,
            "--iterations" => opts.iterations = num(value()?)?,
            "--docs" => opts.docs = num(value()?)?,
            "--doc-words" => opts.doc_words = num(value()?)?,
            "--batch-sizes" => {
                opts.batch_sizes = value()?
                    .split(',')
                    .map(|s| num(s.trim().to_owned()))
                    .collect::<Result<_, _>>()?;
            }
            "--queries" => {
                let path = value()?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                opts.queries = Some(
                    text.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned)
                        .collect(),
                );
            }
            "--label" => opts.label = Some(value()?),
            "--long-words" => opts.long_words = num(value()?)?,
            "--reference" => opts.reference = Some(value()?.into()),
            "--corpus" => opts.corpus = value()?.into(),
            "--ort-dylib" => opts.ort.dylib = Some(value()?.into()),
            "--model-dir" => opts.ort.model_dir = Some(value()?.into()),
            "--variant" => opts.ort.variant = Some(value()?),
            "--device" => opts.ort.device = Some(value()?),
            "--threads" => opts.ort.threads = Some(num(value()?)?),
            "--placement" => opts.ort.placement = true,
            "--no-cpu-fallback" => opts.ort.no_cpu_fallback = true,
            "--json" => json = Some(value()?),
            "--mock-load-ms" => opts.mock_latency.load = millis(value()?)?,
            "--mock-call-ms" => opts.mock_latency.per_call = millis(value()?)?,
            "--mock-item-ms" => opts.mock_latency.per_item = millis(value()?)?,
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_options() {
        let (opts, json) = parse_embed(&args(&[
            "--dim",
            "512",
            "--iterations",
            "50",
            "--batch-sizes",
            "1, 16",
            "--json",
            "out.json",
            "--mock-call-ms",
            "3",
            "--label",
            "XPS on battery",
        ]))
        .unwrap();
        assert_eq!(opts.dim, 512);
        assert_eq!(opts.iterations, 50);
        assert_eq!(opts.batch_sizes, [1, 16]);
        assert_eq!(opts.mock_latency.per_call, Duration::from_millis(3));
        assert_eq!(opts.label.as_deref(), Some("XPS on battery"));
        assert_eq!(json.as_deref(), Some("out.json"));
    }

    #[test]
    fn rejects_bad_options() {
        assert!(
            parse_embed(&args(&["--dim"]))
                .unwrap_err()
                .contains("needs a value")
        );
        assert!(
            parse_embed(&args(&["--dim", "x"]))
                .unwrap_err()
                .contains("not a non-negative")
        );
        assert!(
            parse_embed(&args(&["--fast"]))
                .unwrap_err()
                .contains("unknown option")
        );
    }
}
