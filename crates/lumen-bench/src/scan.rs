//! `lumen-bench scan` and `lumen-bench identity-check` (T009).
//!
//! `scan` measures Pass 0 inventory over real folders and reports counts only: no path or
//! file name is written to the JSON report (privacy; reports are committed as evidence).
//! Sample issue paths go to stderr for the person running it.
//!
//! `identity-check` exercises the file operations that matter for stable identity in a
//! scratch directory on the volume under test.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use lumen_indexer::{Exclusions, FileIdentity, ScanOptions, ScanReport, identity_of, scan};
use serde::Serialize;

use crate::machine::{MachineInfo, MemorySnapshot, memory};

#[derive(Debug, Clone, Default)]
pub(crate) struct ScanBenchOptions {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) identity: bool,
    pub(crate) system_exclusions: bool,
    pub(crate) exclude_names: Vec<String>,
    /// Passes over the same roots; the first is the coldest the OS cache allows.
    pub(crate) repeat: usize,
    pub(crate) show_issues: usize,
    pub(crate) label: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PassReport {
    elapsed_s: f64,
    entries_per_s: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScanBenchReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    roots: usize,
    identity: bool,
    passes: Vec<PassReport>,
    files: u64,
    dirs: u64,
    links: u64,
    other: u64,
    unknown: u64,
    bytes: u64,
    hidden: u64,
    system: u64,
    cloud_placeholders: u64,
    non_unicode_paths: u64,
    identity_skipped: u64,
    /// Entries with an identity shared by another path (hard links; or unstable ids).
    shared_identities: u64,
    excluded_by_rule: BTreeMap<String, u64>,
    /// `stage/kind` → count.
    issues: BTreeMap<String, u64>,
    /// Entries that might be missing: directories that could not be listed, etc.
    blocking_issues: u64,
    complete: bool,
    overlapping_roots: usize,
    memory_after: Option<MemorySnapshot>,
}

/// # Errors
/// No roots.
pub(crate) fn run(opts: &ScanBenchOptions) -> Result<ScanBenchReport, String> {
    if opts.roots.is_empty() {
        return Err("scan needs at least one --root".into());
    }
    let scan_opts = ScanOptions {
        roots: opts.roots.clone(),
        exclusions: Exclusions {
            system_defaults: opts.system_exclusions,
            user_paths: Vec::new(),
            user_names: opts.exclude_names.clone(),
        },
        identity: opts.identity,
    };
    let mut passes = Vec::new();
    let mut last: Option<(ScanReport, u64)> = None;
    for _ in 0..opts.repeat.max(1) {
        let mut seen: HashMap<FileIdentity, u32> = HashMap::new();
        let report = scan(
            &scan_opts,
            |e| {
                if let Some(id) = e.identity {
                    *seen.entry(id).or_insert(0) += 1;
                }
            },
            None,
        );
        let shared: u64 = seen
            .values()
            .filter(|&&n| n > 1)
            .map(|&n| u64::from(n))
            .sum();
        let secs = report.elapsed.as_secs_f64();
        #[allow(clippy::cast_precision_loss)]
        passes.push(PassReport {
            elapsed_s: secs,
            entries_per_s: report.emitted() as f64 / secs.max(1e-9),
        });
        last = Some((report, shared));
    }
    let (r, shared) = last.ok_or("no pass ran")?;

    if opts.show_issues > 0 {
        for i in r.issues.iter().take(opts.show_issues) {
            eprintln!(
                "  issue {:?}/{:?}: {} ({})",
                i.stage,
                i.kind,
                i.path.display(),
                i.message
            );
        }
    }

    let mut issues = BTreeMap::new();
    for i in &r.issues {
        *issues
            .entry(format!("{:?}/{:?}", i.stage, i.kind))
            .or_insert(0) += 1;
    }
    Ok(ScanBenchReport {
        schema_version: 1,
        kind: "scan",
        label: opts.label.clone(),
        machine: MachineInfo::collect(),
        roots: opts.roots.len(),
        identity: opts.identity,
        passes,
        files: r.files,
        dirs: r.dirs,
        links: r.links,
        other: r.other,
        unknown: r.unknown,
        bytes: r.bytes,
        hidden: r.hidden,
        system: r.system,
        cloud_placeholders: r.cloud_placeholders,
        non_unicode_paths: r.non_unicode_paths,
        identity_skipped: r.identity_skipped,
        shared_identities: shared,
        excluded_by_rule: r
            .excluded_by_rule()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        issues,
        blocking_issues: r.blocking_issues().count() as u64,
        complete: r.is_complete(),
        overlapping_roots: r.overlapping_roots.len(),
        memory_after: memory(),
    })
}

pub(crate) fn summarize(r: &ScanBenchReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "scan: {} roots, identity={}{}",
        r.roots,
        r.identity,
        r.label
            .as_deref()
            .map(|l| format!(" [{l}]"))
            .unwrap_or_default()
    );
    for (i, p) in r.passes.iter().enumerate() {
        let _ = writeln!(
            s,
            "  pass {}: {:.2} s, {:.0} entries/s",
            i + 1,
            p.elapsed_s,
            p.entries_per_s
        );
    }
    #[allow(clippy::cast_precision_loss)]
    let gib = r.bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let _ = writeln!(
        s,
        "  found: {} files ({gib:.2} GiB), {} dirs, {} links, {} other, {} unknown type",
        r.files, r.dirs, r.links, r.other, r.unknown
    );
    let _ = writeln!(
        s,
        "  flags: {} hidden, {} system, {} cloud placeholders, {} non-Unicode paths",
        r.hidden, r.system, r.cloud_placeholders, r.non_unicode_paths
    );
    if r.identity {
        let _ = writeln!(
            s,
            "  identity: {} skipped (links/recall-on-open), {} entries share an id (hard links)",
            r.identity_skipped, r.shared_identities
        );
    }
    let excluded: u64 = r.excluded_by_rule.values().sum();
    let _ = writeln!(s, "  excluded: {excluded} {:?}", r.excluded_by_rule);
    let issues: u64 = r.issues.values().sum();
    let _ = writeln!(
        s,
        "  issues: {issues} ({} may hide entries) {:?}",
        r.blocking_issues, r.issues
    );
    let _ = writeln!(
        s,
        "  coverage: {}",
        if r.complete {
            "COMPLETE — every entry emitted or excluded by a rule"
        } else {
            "INCOMPLETE — see blocking issues above"
        }
    );
    if r.machine.build_profile != "release" {
        s.push_str("  WARNING: debug build — not acceptance evidence\n");
    }
    s
}

/// One identity scenario.
#[derive(Debug, Serialize)]
pub(crate) struct IdentityCase {
    case: &'static str,
    /// What Lumen's design assumes (docs/ARCHITECTURE.md §5); `None` = either is legal.
    expected_same: Option<bool>,
    same: bool,
    ok: bool,
    note: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct IdentityReport {
    schema_version: u32,
    kind: &'static str,
    label: Option<String>,
    machine: MachineInfo,
    cases: Vec<IdentityCase>,
    all_ok: bool,
}

/// # Errors
/// I/O failures in the scratch directory.
pub(crate) fn identity_check(dir: &Path, label: Option<String>) -> Result<IdentityReport, String> {
    let work = dir.join(format!("lumen-identity-check-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let result = identity_cases(&work);
    let _ = fs::remove_dir_all(&work);
    let cases = result?;
    Ok(IdentityReport {
        schema_version: 1,
        kind: "identity-check",
        label,
        machine: MachineInfo::collect(),
        all_ok: cases.iter().all(|c| c.ok),
        cases,
    })
}

fn identity_cases(work: &Path) -> Result<Vec<IdentityCase>, String> {
    fn e(what: &'static str) -> impl Fn(std::io::Error) -> String {
        move |err| format!("{what}: {err}")
    }
    let id = |p: &Path| identity_of(p).map_err(|err| format!("identity {}: {err}", p.display()));
    fs::create_dir_all(work.join("sub")).map_err(e("create scratch dir"))?;
    let mut cases = Vec::new();
    let mut case = |case, expected: Option<bool>, same: bool, note| {
        cases.push(IdentityCase {
            case,
            expected_same: expected,
            same,
            ok: expected.is_none_or(|e| e == same),
            note,
        });
    };

    let a = work.join("a.txt");
    fs::write(&a, b"original").map_err(e("write"))?;
    let original = id(&a)?;

    let renamed = work.join("renamed.txt");
    fs::rename(&a, &renamed).map_err(e("rename"))?;
    case(
        "rename",
        Some(true),
        id(&renamed)? == original,
        "same item, new path",
    );

    let moved = work.join("sub").join("renamed.txt");
    fs::rename(&renamed, &moved).map_err(e("move"))?;
    case(
        "move_same_volume",
        Some(true),
        id(&moved)? == original,
        "same item, new parent",
    );

    fs::write(&moved, b"edited in place").map_err(e("overwrite"))?;
    case(
        "edit_in_place",
        Some(true),
        id(&moved)? == original,
        "content changed; fingerprint decides re-index",
    );

    let copy = work.join("copy.txt");
    fs::copy(&moved, &copy).map_err(e("copy"))?;
    case("copy", Some(false), id(&copy)? == original, "a new item");

    // Editor "safe save": write a temp file, then rename it over the original.
    let tmp = work.join("sub").join("renamed.txt.tmp");
    fs::write(&tmp, b"saved by replace").map_err(e("write temp"))?;
    fs::rename(&tmp, &moved).map_err(e("replace"))?;
    case(
        "save_by_replace",
        Some(false),
        id(&moved)? == original,
        "new id at the same path: T207 must treat same path + new id as an update",
    );

    let link = work.join("hardlink.txt");
    fs::hard_link(&moved, &link).map_err(e("hard link"))?;
    case(
        "hard_link",
        Some(true),
        id(&link)? == id(&moved)?,
        "two paths, one file: second path is an alias, not a new item",
    );

    let recreated = work.join("recreated.txt");
    fs::write(&recreated, b"x").map_err(e("write"))?;
    let first = id(&recreated)?;
    fs::remove_file(&recreated).map_err(e("delete"))?;
    fs::write(&recreated, b"x").map_err(e("recreate"))?;
    case(
        "delete_and_recreate",
        None,
        id(&recreated)? == first,
        "may be reused on Unix (inodes); never trust identity without size/mtime",
    );

    Ok(cases)
}

pub(crate) fn summarize_identity(r: &IdentityReport) -> String {
    use std::fmt::Write as _;
    let mut s = String::from("identity-check:\n");
    for c in &r.cases {
        let _ = writeln!(
            s,
            "  {:<20} same={:<5} expected={:<9} {}  {}",
            c.case,
            c.same,
            c.expected_same
                .map_or("either", |e| if e { "same" } else { "different" }),
            if c.ok { "ok  " } else { "DIFF" },
            c.note
        );
    }
    let _ = writeln!(s, "  all as expected: {}", r.all_ok);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_bench_counts_without_paths() {
        let dir = std::env::temp_dir().join(format!("lumen-bench-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/b/secret-name.txt"), b"x").unwrap();
        fs::hard_link(dir.join("a/b/secret-name.txt"), dir.join("a/alias.txt")).unwrap();
        let r = run(&ScanBenchOptions {
            roots: vec![dir.clone()],
            identity: true,
            system_exclusions: true,
            repeat: 2,
            ..ScanBenchOptions::default()
        })
        .unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(r.shared_identities, 2);
        assert_eq!(r.passes.len(), 2);
        assert!(r.complete);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("secret-name"), "no file names in the report");
        assert!(summarize(&r).contains("COMPLETE"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn identity_check_runs() {
        let r = identity_check(&std::env::temp_dir(), None).unwrap();
        let rename = r.cases.iter().find(|c| c.case == "rename").unwrap();
        assert!(rename.ok);
        assert!(r.cases.iter().find(|c| c.case == "hard_link").unwrap().ok);
        assert!(summarize_identity(&r).contains("save_by_replace"));
    }
}
