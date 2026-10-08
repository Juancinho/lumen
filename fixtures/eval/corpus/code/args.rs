//! Command-line parsing for the export tool.

use std::path::PathBuf;

pub struct Args {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub verbose: bool,
}

/// Parses `--input FILE [--output FILE] [--verbose]` from the process arguments.
pub fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut input = None;
    let mut output = None;
    let mut verbose = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--input" => input = args.next().map(PathBuf::from),
            "--output" => output = args.next().map(PathBuf::from),
            "--verbose" => verbose = true,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(Args { input: input.ok_or("--input is required")?, output, verbose })
}
