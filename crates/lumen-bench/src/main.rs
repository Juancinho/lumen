//! Lumen benchmark harness. Always run release builds for evidence:
//!
//! ```text
//! cargo run --release -p lumen-bench -- embed --json target/bench/embed-mock.json
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::print_stdout)] // CLI output

mod ann;
mod catalog;
mod chunk;
mod corpus;
mod cpu;
mod device;
mod embed;
mod fidelity;
mod llama;
mod machine;
mod scan;
mod stats;
mod storage;
mod synth;

use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
Usage: lumen-bench <command> [options]

Commands:
  embed     Embedding backend latency/throughput/memory (T005/T006)
  ann       ANN index (USearch/HNSW) build/search/recall/memory/persistence (T008)
  storage   SQLite/FTS5 insert throughput, per-keystroke lexical latency, size (T007)
  scan      File inventory over real folders: counts, coverage, speed (T009; no paths stored)
  identity-check  Stable file identity under rename/move/copy/save/hard link (T009)
  catalog   App/file catalog: inventory -> SQLite, app discovery, keystroke name lookup (T101)
  chunk     Text/code extraction + chunking over real folders (T201; counts only)
  probe     Measure one embedding device for the device policy (T013), JSON probe
  device-policy   Device decisions from probe files across power/profile scenarios (T013)

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
  --variant NAME         ort: fp32 | fp16 | q8 | q4 | q4f16 (default: q4);
                         llama-server: weights label for the space (default: gguf-q8_0)
  --server HOST:PORT     llama-server address for --backend llama-server (default 127.0.0.1:8080)
  --server-target T      cpu | gpu: what that llama-server build runs on (reports only)
  --cpu-pid PID          report the throughput CPU share of PID (e.g. llama-server)
                         instead of this process
  --device NAME          cpu | dml:<adapter> | dml:high | dml:low (default: cpu)
  --threads N            intra-op threads (default: runtime default)
  --placement            report which execution provider runs each graph node
  --no-cpu-fallback      fail instead of running unsupported GPU nodes on CPU

ann options:
  --sizes A,B,..         vector counts (default: 100000)
  --dim N                dimension (default: 256)
  --dataset NAME         embedding-like | uniform (default: embedding-like)
  --scalars A,B,..       f32,f16,bf16,i8 (default: all)
  --connectivity N       HNSW M (default: 16)
  --expansion-add N      ef_construction (default: 128)
  --efs A,B,..           search ef sweep (default: 16,32,64,128,256)
  --queries N            measured queries (default: 500)
  --k N                  neighbours per query (default: 10)
  --threads N            build threads (default: all logical CPUs)
  --work-dir DIR         where index files are saved/loaded (default: temp dir)
  --vectors D.f32,Q.f32  real vectors (raw LE f32, --dim per row) instead of synthetic data
  --label TEXT / --json PATH   as for embed

storage options:
  --chunks N             chunks to insert (default: 100000)
  --words N              words per chunk (default: 120)
  --batch N              chunks per transaction (default: 1000)
  --work-dir DIR         database location (default: temp dir; deleted afterwards)
  --label TEXT / --json PATH

scan options:
  --root DIR             folder to inventory (repeatable)
  --identity             also read stable file identity (one handle open per entry)
  --exclude-name NAME    exclude entries with this name anywhere (repeatable)
  --no-system-exclusions include $Recycle.Bin, System Volume Information...
  --repeat N             passes over the same roots (default: 1)
  --show-issues N        print the first N issue paths to stderr (default: 10)
  --label TEXT / --json PATH

catalog options:
  --root DIR             folder to inventory (repeatable)
  --apps                 also discover Start-menu applications
  --app-defaults         apply the app's default exclusions (developer noise, build folders
                         next to project markers, system folders on a whole system drive)
  --exclude DIR          extra excluded folder (repeatable)
  --sample N             names sampled for keystroke queries (default: 300)
  --show QUERY           print the top results for QUERY to stderr (repeatable)
  --work-dir DIR         database location (default: temp dir; deleted afterwards)
  --label TEXT / --json PATH

chunk options:
  --root DIR             folder to read (repeatable; app default exclusions apply)
  --target N             target tokens per chunk (default: 128; max = 1.5 x target)
  --tokenizer FILE       tokenizer.json of the embedding model (feature `tokenizer`)
  --label TEXT / --json PATH

probe options (plus every embed option: --backend, --ort-dylib, --model-dir, --variant,
--device, --threads, --placement, --dim, --label, --json):
  --device-id NAME       policy device id (default: --device, or cpu)
  --integrated           the device is an integrated GPU
  --runtime-key TEXT     runtime + driver versions (default: backend runtime version)
  --save-vectors FILE    write this probe's vectors (run on cpu first)
  --cpu-vectors FILE     compare with the CPU probe's vectors
  --device-memory-mib N  device memory the session used (measured outside, e.g. PDH)
  --device-memory-total-mib N   the device's total memory
  --measured-queries N   timed queries (default: 40)

device-policy options:
  --probe FILE           probe JSON (repeatable; include the cpu probe)
  --space KEY            index generation space (default: the cpu probe's)
  --label TEXT / --json PATH

identity-check options:
  --dir DIR              scratch location on the volume under test (default: temp dir)
  --label TEXT / --json PATH
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("embed") => match parse_embed(&args[1..]).and_then(|(opts, json)| {
            embed::run(&opts).map(|r| (embed::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("ann") => match parse_ann(&args[1..]).and_then(|(opts, json)| {
            ann::run(&opts).map(|r| (ann::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("storage") => match parse_storage(&args[1..]).and_then(|(opts, json)| {
            storage::run(&opts).map(|r| (storage::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("scan") => match parse_scan(&args[1..]).and_then(|(opts, json)| {
            scan::run(&opts).map(|r| (scan::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("identity-check") => match parse_identity(&args[1..]).and_then(|(dir, label, json)| {
            scan::identity_check(&dir, label)
                .map(|r| (scan::summarize_identity(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("chunk") => match parse_chunk(&args[1..]).and_then(|(opts, json)| {
            chunk::run(&opts).map(|r| (chunk::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("catalog") => match parse_catalog(&args[1..]).and_then(|(opts, json)| {
            catalog::run(&opts).map(|r| (catalog::summarize(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("probe") => match parse_probe(&args[1..]).and_then(|(opts, json)| {
            device::run_probe(&opts).map(|r| (device::summarize_probe(&r), to_json(&r), json))
        }) {
            Ok((summary, json, path)) => emit(&summary, json, path),
            Err(err) => usage_error(&err),
        },
        Some("device-policy") => {
            match parse_policy(&args[1..]).and_then(|(files, space, label, json)| {
                device::run_policy(&files, space, label)
                    .map(|r| (device::summarize_policy(&r), to_json(&r), json))
            }) {
                Ok((summary, json, path)) => emit(&summary, json, path),
                Err(err) => usage_error(&err),
            }
        }
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

fn to_json<T: serde::Serialize>(report: &T) -> Result<String, String> {
    serde_json::to_string_pretty(report).map_err(|e| format!("serialize report: {e}"))
}

fn usage_error(err: &str) -> ExitCode {
    eprintln!("error: {err}\n\n{USAGE}");
    ExitCode::FAILURE
}

/// Prints the summary to stderr and the JSON report to `path` (or stdout).
fn emit(summary: &str, json: Result<String, String>, path: Option<String>) -> ExitCode {
    eprint!("{summary}");
    let json = match json {
        Ok(json) => json,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    match path {
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

fn parse_list(flag: &str, v: &str) -> Result<Vec<usize>, String> {
    v.split(',')
        .map(|s| {
            s.trim()
                .replace('_', "")
                .parse::<usize>()
                .map_err(|_| format!("{flag}: `{s}` is not a non-negative integer"))
        })
        .collect()
}

fn parse_ann(args: &[String]) -> Result<(ann::AnnOptions, Option<String>), String> {
    let mut opts = ann::AnnOptions::default();
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        let one = |v: &str| {
            parse_list(flag, v)
                .and_then(|l| l.first().copied().ok_or_else(|| format!("{flag}: empty")))
        };
        match flag.as_str() {
            "--sizes" => opts.sizes = parse_list(flag, &value)?,
            "--dim" => opts.dim = one(&value)?,
            "--dataset" => {
                opts.dataset = synth::Dataset::parse(&value)
                    .ok_or_else(|| format!("--dataset: `{value}` (embedding-like, uniform)"))?;
            }
            "--scalars" => {
                opts.scalars = value
                    .split(',')
                    .map(|s| {
                        lumen_vector::Scalar::parse(s.trim())
                            .ok_or_else(|| format!("--scalars: `{s}` (f32, f16, bf16, i8)"))
                    })
                    .collect::<Result<_, _>>()?;
            }
            "--connectivity" => opts.connectivity = one(&value)?,
            "--expansion-add" => opts.expansion_add = one(&value)?,
            "--efs" => opts.efs = parse_list(flag, &value)?,
            "--queries" => opts.queries = one(&value)?,
            "--k" => opts.k = one(&value)?,
            "--threads" => opts.threads = one(&value)?.max(1),
            "--work-dir" => opts.work_dir = value.into(),
            "--vectors" => {
                let (docs, queries) = value
                    .split_once(',')
                    .ok_or("--vectors needs DOCS.f32,QUERIES.f32")?;
                opts.vectors = Some((docs.into(), queries.into()));
            }
            "--label" => opts.label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

fn parse_storage(args: &[String]) -> Result<(storage::StorageOptions, Option<String>), String> {
    let mut opts = storage::StorageOptions::default();
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        let one = |v: &str| {
            parse_list(flag, v)
                .and_then(|l| l.first().copied().ok_or_else(|| format!("{flag}: empty")))
        };
        match flag.as_str() {
            "--chunks" => opts.chunks = one(&value)?,
            "--words" => opts.words = one(&value)?,
            "--batch" => opts.batch = one(&value)?.max(1),
            "--work-dir" => opts.work_dir = value.into(),
            "--label" => opts.label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

fn parse_scan(args: &[String]) -> Result<(scan::ScanBenchOptions, Option<String>), String> {
    let mut opts = scan::ScanBenchOptions {
        system_exclusions: true,
        repeat: 1,
        show_issues: 10,
        ..scan::ScanBenchOptions::default()
    };
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--identity" => {
                opts.identity = true;
                continue;
            }
            "--no-system-exclusions" => {
                opts.system_exclusions = false;
                continue;
            }
            _ => {}
        }
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        let one = |v: &str| {
            parse_list(flag, v)
                .and_then(|l| l.first().copied().ok_or_else(|| format!("{flag}: empty")))
        };
        match flag.as_str() {
            "--root" => opts.roots.push(value.into()),
            "--exclude-name" => opts.exclude_names.push(value),
            "--repeat" => opts.repeat = one(&value)?.max(1),
            "--show-issues" => opts.show_issues = one(&value)?,
            "--label" => opts.label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

type IdentityArgs = (std::path::PathBuf, Option<String>, Option<String>);

fn parse_identity(args: &[String]) -> Result<IdentityArgs, String> {
    let mut dir = std::env::temp_dir();
    let (mut label, mut json) = (None, None);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--dir" => dir = value.into(),
            "--label" => label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((dir, label, json))
}

fn parse_chunk(args: &[String]) -> Result<(chunk::ChunkOptions, Option<String>), String> {
    let mut opts = chunk::ChunkOptions::default();
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--root" => opts.roots.push(value.into()),
            "--target" => {
                opts.target_tokens = Some(
                    value
                        .parse()
                        .map_err(|_| format!("{flag}: not a positive integer"))?,
                );
            }
            "--tokenizer" => opts.tokenizer = Some(value.into()),
            "--label" => opts.label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

fn parse_catalog(args: &[String]) -> Result<(catalog::CatalogOptions, Option<String>), String> {
    let mut opts = catalog::CatalogOptions {
        sample: 300,
        ..catalog::CatalogOptions::default()
    };
    let mut json = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "--apps" {
            opts.apps = true;
            continue;
        }
        if flag == "--app-defaults" {
            opts.app_defaults = true;
            continue;
        }
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--root" => opts.roots.push(value.into()),
            "--exclude" => opts.exclude.push(value.into()),
            "--sample" => {
                opts.sample = value
                    .parse()
                    .map_err(|_| format!("{flag}: not a non-negative integer"))?;
            }
            "--show" => opts.show.push(value),
            "--work-dir" => opts.work_dir = Some(value.into()),
            "--label" => opts.label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((opts, json))
}

fn parse_probe(args: &[String]) -> Result<(device::ProbeOptions, Option<String>), String> {
    let mut opts = device::ProbeOptions {
        measured_queries: 40,
        ..device::ProbeOptions::default()
    };
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        let float = |v: String| {
            v.parse::<f64>()
                .map_err(|_| format!("{flag}: `{v}` is not a number"))
        };
        match flag.as_str() {
            "--device-id" => opts.device_id = Some(value()?),
            "--integrated" => opts.integrated = true,
            "--runtime-key" => opts.runtime_key = Some(value()?),
            "--save-vectors" => opts.save_vectors = Some(value()?.into()),
            "--cpu-vectors" => opts.cpu_vectors = Some(value()?.into()),
            "--device-memory-mib" => opts.device_memory_mib = Some(float(value()?)?),
            "--device-memory-total-mib" => opts.device_memory_total_mib = Some(float(value()?)?),
            "--measured-queries" => {
                opts.measured_queries = value()?
                    .parse()
                    .map_err(|_| format!("{flag}: not a non-negative integer"))?;
            }
            _ => rest.push(flag.clone()),
        }
    }
    let (embed, json) = parse_embed(&rest)?;
    opts.embed = embed;
    Ok((opts, json))
}

type PolicyArgs = (
    Vec<std::path::PathBuf>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_policy(args: &[String]) -> Result<PolicyArgs, String> {
    let (mut files, mut space, mut label, mut json) = (Vec::new(), None, None, None);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .cloned()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--probe" => files.push(value.into()),
            "--space" => space = Some(value),
            "--label" => label = Some(value),
            "--json" => json = Some(value),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((files, space, label, json))
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
            "--variant" => {
                let v = value()?;
                opts.llama.variant = Some(v.clone());
                opts.ort.variant = Some(v);
            }
            "--server" => opts.llama.addr = Some(value()?),
            "--server-target" => opts.llama.target = Some(value()?),
            "--cpu-pid" => {
                opts.cpu_pid = Some(
                    value()?
                        .parse()
                        .map_err(|_| "--cpu-pid: expected a process id")?,
                )
            }
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
    fn parses_ann_options() {
        let (opts, json) = parse_ann(&args(&[
            "--sizes",
            "100_000,1000000",
            "--scalars",
            "f32,i8",
            "--efs",
            "32,64",
            "--k",
            "5",
            "--dataset",
            "uniform",
            "--json",
            "a.json",
        ]))
        .unwrap();
        assert_eq!(opts.sizes, [100_000, 1_000_000]);
        assert_eq!(
            opts.scalars,
            [lumen_vector::Scalar::F32, lumen_vector::Scalar::I8]
        );
        assert_eq!(opts.efs, [32, 64]);
        assert_eq!(opts.k, 5);
        assert_eq!(opts.dataset, synth::Dataset::Uniform);
        assert_eq!(json.as_deref(), Some("a.json"));
        assert!(parse_ann(&args(&["--scalars", "f8"])).is_err());
        assert!(parse_ann(&args(&["--sizes"])).is_err());
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
