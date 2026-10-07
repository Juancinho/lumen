//! Pass 0 inventory: walk indexed roots and emit every entry (docs/SEARCH_AND_INDEXING.md §20).
//!
//! Invariant (ADR-018): for every directory entry the walk reaches, exactly one of these holds:
//! - it is emitted as a [`ScanEntry`] (metadata, flags and identity are best-effort: a failure
//!   to read them is recorded as a [`ScanIssue`] but never suppresses the entry);
//! - it matched an exclusion rule and is listed in [`ScanReport::excluded`] with that rule;
//! - it could not even be listed (its parent directory or the entry iterator failed), which is
//!   a [`ScanIssue`] on the closest path we know.
//!
//! Directories are walked iteratively (no recursion, so depth is bounded only by memory).
//! Symlinks, junctions and other name-surrogate reparse points are emitted but never followed,
//! which makes link loops impossible. Cloud placeholders (OneDrive "files on demand") are
//! emitted from directory metadata only: nothing here opens their content.

use std::collections::BTreeMap;
use std::fs::{self, DirEntry, FileType, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lumen_core::CancellationToken;

use crate::identity::{FileIdentity, identity_of};

/// Directory names excluded by default on every volume: OS-owned folders users never search
/// and that are usually unreadable anyway. Matched case-insensitively against the entry name.
pub const SYSTEM_EXCLUSIONS: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "$WinREAgent",
    "Config.Msi",
];

/// What to leave out. Every exclusion is reported, never silent.
#[derive(Debug, Clone)]
pub struct Exclusions {
    /// Apply [`SYSTEM_EXCLUSIONS`].
    pub system_defaults: bool,
    /// Absolute paths excluded with their subtree (user setting).
    pub user_paths: Vec<PathBuf>,
    /// Entry names excluded anywhere, e.g. `node_modules` (user setting).
    pub user_names: Vec<String>,
}

impl Default for Exclusions {
    fn default() -> Self {
        Self {
            system_defaults: true,
            user_paths: Vec::new(),
            user_names: Vec::new(),
        }
    }
}

/// Scan configuration.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Indexed roots. Overlapping roots are merged (a root inside another is skipped).
    /// A root that is itself a link is followed: the user picked it explicitly.
    pub roots: Vec<PathBuf>,
    pub exclusions: Exclusions,
    /// Read stable [`FileIdentity`] for every entry (one handle open per entry on Windows).
    pub identity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// Symlink, junction or other name-surrogate reparse point. Not followed.
    Symlink,
    /// Socket, device, FIFO… (Unix).
    Other,
    /// The type itself could not be read; the entry is still emitted.
    Unknown,
}

/// Attribute flags. On Unix `hidden` means a dot-name and the rest are false.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryFlags {
    pub hidden: bool,
    pub system: bool,
    pub readonly: bool,
    /// Content not on local disk (OneDrive/cloud files on demand). Never opened here.
    pub cloud_placeholder: bool,
}

/// One inventoried entry.
#[derive(Debug, Clone)]
pub struct ScanEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
    /// Bytes for files; `None` when metadata failed or for non-files.
    pub size: Option<u64>,
    /// Milliseconds since the Unix epoch.
    pub modified_ms: Option<i64>,
    pub created_ms: Option<i64>,
    pub flags: EntryFlags,
    /// `None` when not requested, skipped (links, recall-on-open placeholders) or failed.
    pub identity: Option<FileIdentity>,
}

impl ScanEntry {
    /// The final component (never empty for entries below a root).
    #[must_use]
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.path.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    PermissionDenied,
    NotFound,
    Other,
}

impl IssueKind {
    fn of(e: &io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Other,
        }
    }
}

/// Which step failed. Only [`IssueStage::OpenRoot`], [`IssueStage::ListDirectory`] and
/// [`IssueStage::ReadEntry`] mean that entries may be missing from the inventory; the other
/// stages degrade an entry that was still emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStage {
    OpenRoot,
    ListDirectory,
    ReadEntry,
    ReadMetadata,
    ReadIdentity,
}

impl IssueStage {
    /// True when this failure can hide entries (they must be retried later, e.g. by T101).
    #[must_use]
    pub fn hides_entries(self) -> bool {
        matches!(self, Self::OpenRoot | Self::ListDirectory | Self::ReadEntry)
    }
}

#[derive(Debug, Clone)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub stage: IssueStage,
    pub kind: IssueKind,
    pub message: String,
}

/// An entry left out on purpose, with the rule that matched (its subtree is not walked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    pub path: PathBuf,
    pub rule: String,
}

/// Totals and everything that was not emitted. Paths only appear in `excluded` / `issues`.
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub files: u64,
    pub dirs: u64,
    pub links: u64,
    pub other: u64,
    pub unknown: u64,
    /// Sum of file sizes.
    pub bytes: u64,
    pub hidden: u64,
    pub system: u64,
    pub cloud_placeholders: u64,
    /// Entries whose path is not valid Unicode (unpaired UTF-16 surrogates on Windows,
    /// non-UTF-8 bytes on Unix). Emitted like any other; storage must keep them losslessly.
    pub non_unicode_paths: u64,
    /// Identity deliberately not read (links, recall-on-open placeholders).
    pub identity_skipped: u64,
    pub excluded: Vec<Excluded>,
    pub issues: Vec<ScanIssue>,
    /// Roots skipped because another root already contains them.
    pub overlapping_roots: Vec<PathBuf>,
    /// The walk stopped early; the inventory is incomplete and must be resumed.
    pub cancelled: bool,
    pub elapsed: Duration,
}

impl ScanReport {
    /// Entries handed to the callback.
    #[must_use]
    pub fn emitted(&self) -> u64 {
        self.files + self.dirs + self.links + self.other + self.unknown
    }

    /// Exclusion counts per rule.
    #[must_use]
    pub fn excluded_by_rule(&self) -> BTreeMap<&str, u64> {
        let mut map = BTreeMap::new();
        for e in &self.excluded {
            *map.entry(e.rule.as_str()).or_insert(0) += 1;
        }
        map
    }

    /// Issues that may hide entries (see [`IssueStage::hides_entries`]).
    pub fn blocking_issues(&self) -> impl Iterator<Item = &ScanIssue> {
        self.issues.iter().filter(|i| i.stage.hides_entries())
    }

    /// Nothing was cancelled and no directory failed to list: every entry under the roots is
    /// either emitted or excluded.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.cancelled && self.blocking_issues().next().is_none()
    }

    fn issue(&mut self, path: &Path, stage: IssueStage, e: &io::Error) {
        self.issues.push(ScanIssue {
            path: path.to_path_buf(),
            stage,
            kind: IssueKind::of(e),
            message: e.to_string(),
        });
    }
}

/// Walks `opts.roots` and calls `on_entry` for every entry, roots included.
///
/// Order is depth-first but otherwise unspecified. `cancel` is polled between directories.
pub fn scan(
    opts: &ScanOptions,
    mut on_entry: impl FnMut(ScanEntry),
    cancel: Option<&CancellationToken>,
) -> ScanReport {
    let started = Instant::now();
    let mut report = ScanReport::default();
    let rules = Rules::new(&opts.exclusions);
    let mut stack: Vec<PathBuf> = Vec::new();

    for root in merge_roots(&opts.roots, &mut report) {
        // The root itself: follow a link here (explicit user choice).
        match fs::metadata(&root) {
            Ok(meta) => {
                let ft = meta.file_type();
                let entry = build_entry(&root, Some(ft), Ok(meta), opts.identity, &mut report);
                let is_dir = entry.kind == EntryKind::Dir;
                count(&entry, &mut report);
                on_entry(entry);
                if is_dir {
                    stack.push(root);
                }
            }
            Err(e) => report.issue(&root, IssueStage::OpenRoot, &e),
        }
    }

    while let Some(dir) = stack.pop() {
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            report.cancelled = true;
            break;
        }
        let reader = match fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                report.issue(&dir, IssueStage::ListDirectory, &e);
                continue;
            }
        };
        for item in reader {
            let de: DirEntry = match item {
                Ok(de) => de,
                Err(e) => {
                    report.issue(&dir, IssueStage::ReadEntry, &e);
                    continue;
                }
            };
            let path = de.path();
            if let Some(rule) = rules.matches(&path) {
                report.excluded.push(Excluded { path, rule });
                continue;
            }
            let ft = de.file_type().ok();
            // Not following links: DirEntry metadata is lstat-like (on Windows it comes from
            // the directory listing itself, no extra open).
            let meta = de.metadata();
            let entry = build_entry(&path, ft, meta, opts.identity, &mut report);
            let descend = entry.kind == EntryKind::Dir;
            count(&entry, &mut report);
            on_entry(entry);
            if descend {
                stack.push(path);
            }
        }
    }

    report.elapsed = started.elapsed();
    report
}

fn build_entry(
    path: &Path,
    ft: Option<FileType>,
    meta: io::Result<Metadata>,
    want_identity: bool,
    report: &mut ScanReport,
) -> ScanEntry {
    let meta = match meta {
        Ok(m) => Some(m),
        Err(e) => {
            report.issue(path, IssueStage::ReadMetadata, &e);
            None
        }
    };
    let ft = ft.or_else(|| meta.as_ref().map(Metadata::file_type));
    let kind = match ft {
        Some(t) if t.is_symlink() => EntryKind::Symlink,
        Some(t) if t.is_dir() => EntryKind::Dir,
        Some(t) if t.is_file() => EntryKind::File,
        Some(_) => EntryKind::Other,
        None => EntryKind::Unknown,
    };
    let flags = flags_of(path, meta.as_ref());
    let size = meta
        .as_ref()
        .filter(|_| kind == EntryKind::File)
        .map(Metadata::len);
    let modified_ms = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(epoch_ms);
    let created_ms = meta
        .as_ref()
        .and_then(|m| m.created().ok())
        .and_then(epoch_ms);

    let identity = if !want_identity {
        None
    } else if kind == EntryKind::Symlink || recall_on_open(meta.as_ref()) {
        // Opening a link would identify its target; opening a recall-on-open placeholder
        // would download it.
        report.identity_skipped += 1;
        None
    } else {
        match identity_of(path) {
            Ok(id) => Some(id),
            Err(e) => {
                report.issue(path, IssueStage::ReadIdentity, &e);
                None
            }
        }
    };

    ScanEntry {
        path: path.to_path_buf(),
        kind,
        size,
        modified_ms,
        created_ms,
        flags,
        identity,
    }
}

fn count(entry: &ScanEntry, report: &mut ScanReport) {
    match entry.kind {
        EntryKind::File => {
            report.files += 1;
            report.bytes += entry.size.unwrap_or(0);
        }
        EntryKind::Dir => report.dirs += 1,
        EntryKind::Symlink => report.links += 1,
        EntryKind::Other => report.other += 1,
        EntryKind::Unknown => report.unknown += 1,
    }
    report.hidden += u64::from(entry.flags.hidden);
    report.system += u64::from(entry.flags.system);
    report.cloud_placeholders += u64::from(entry.flags.cloud_placeholder);
    report.non_unicode_paths += u64::from(entry.path.to_str().is_none());
}

fn epoch_ms(t: SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|ms| -ms),
    }
}

#[cfg(windows)]
mod attrs {
    pub(super) const READONLY: u32 = 0x1;
    pub(super) const HIDDEN: u32 = 0x2;
    pub(super) const SYSTEM: u32 = 0x4;
    pub(super) const OFFLINE: u32 = 0x1000;
    pub(super) const RECALL_ON_OPEN: u32 = 0x4_0000;
    pub(super) const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    pub(super) fn of(meta: &std::fs::Metadata) -> u32 {
        std::os::windows::fs::MetadataExt::file_attributes(meta)
    }
}

#[cfg(windows)]
fn flags_of(_path: &Path, meta: Option<&Metadata>) -> EntryFlags {
    let Some(a) = meta.map(attrs::of) else {
        return EntryFlags::default();
    };
    EntryFlags {
        hidden: a & attrs::HIDDEN != 0,
        system: a & attrs::SYSTEM != 0,
        readonly: a & attrs::READONLY != 0,
        cloud_placeholder: a
            & (attrs::OFFLINE | attrs::RECALL_ON_OPEN | attrs::RECALL_ON_DATA_ACCESS)
            != 0,
    }
}

#[cfg(windows)]
fn recall_on_open(meta: Option<&Metadata>) -> bool {
    meta.is_some_and(|m| attrs::of(m) & attrs::RECALL_ON_OPEN != 0)
}

#[cfg(not(windows))]
fn flags_of(path: &Path, meta: Option<&Metadata>) -> EntryFlags {
    EntryFlags {
        hidden: path
            .file_name()
            .is_some_and(|n| n.as_encoded_bytes().first() == Some(&b'.')),
        system: false,
        readonly: meta.is_some_and(|m| m.permissions().readonly()),
        cloud_placeholder: false,
    }
}

#[cfg(not(windows))]
fn recall_on_open(_meta: Option<&Metadata>) -> bool {
    false
}

/// Comparison key: absolute, and case-folded on Windows (NTFS is case-insensitive by default).
fn path_key(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    if cfg!(windows) {
        PathBuf::from(abs.to_string_lossy().to_lowercase())
    } else {
        abs
    }
}

fn name_key(s: &str) -> String {
    s.to_lowercase()
}

/// Drops duplicate roots and roots nested inside another root (recorded in the report).
fn merge_roots(roots: &[PathBuf], report: &mut ScanReport) -> Vec<PathBuf> {
    let mut keyed: Vec<(PathBuf, PathBuf)> = roots
        .iter()
        .map(|r| {
            let abs = std::path::absolute(r).unwrap_or_else(|_| r.clone());
            (path_key(&abs), abs)
        })
        .collect();
    // Shorter first, so a parent is kept before its children.
    keyed.sort_by_key(|(k, _)| k.components().count());
    let mut kept: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (key, abs) in keyed {
        if kept.iter().any(|(k, _)| key.starts_with(k)) {
            report.overlapping_roots.push(abs);
        } else {
            kept.push((key, abs));
        }
    }
    kept.into_iter().map(|(_, abs)| abs).collect()
}

struct Rules {
    system: Vec<(String, &'static str)>,
    names: Vec<(String, String)>,
    paths: Vec<(PathBuf, PathBuf)>,
}

impl Rules {
    fn new(ex: &Exclusions) -> Self {
        let system = if ex.system_defaults {
            SYSTEM_EXCLUSIONS
                .iter()
                .map(|n| (name_key(n), *n))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            system,
            names: ex
                .user_names
                .iter()
                .map(|n| (name_key(n), n.clone()))
                .collect(),
            paths: ex
                .user_paths
                .iter()
                .map(|p| (path_key(p), p.clone()))
                .collect(),
        }
    }

    /// The rule label that excludes `path`, if any.
    fn matches(&self, path: &Path) -> Option<String> {
        let name = path.file_name().map(|n| name_key(&n.to_string_lossy()));
        if let Some(name) = &name {
            if let Some((_, label)) = self.system.iter().find(|(k, _)| k == name) {
                return Some(format!("system:{label}"));
            }
            if let Some((_, label)) = self.names.iter().find(|(k, _)| k == name) {
                return Some(format!("name:{label}"));
            }
        }
        if !self.paths.is_empty() {
            let key = path_key(path);
            if let Some((_, label)) = self.paths.iter().find(|(k, _)| key.starts_with(k)) {
                return Some(format!("path:{}", label.display()));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("lumen-scan-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn opts(roots: &[&Path]) -> ScanOptions {
        ScanOptions {
            roots: roots.iter().map(|p| p.to_path_buf()).collect(),
            ..ScanOptions::default()
        }
    }

    fn run(o: &ScanOptions) -> (Vec<ScanEntry>, ScanReport) {
        let mut out = Vec::new();
        let report = scan(o, |e| out.push(e), None);
        (out, report)
    }

    fn paths(entries: &[ScanEntry]) -> BTreeSet<PathBuf> {
        entries.iter().map(|e| e.path.clone()).collect()
    }

    /// Independent recursive walk (no exclusions, links not followed) as the oracle.
    fn oracle(dir: &Path, out: &mut BTreeSet<PathBuf>) {
        for de in fs::read_dir(dir).unwrap() {
            let de = de.unwrap();
            out.insert(de.path());
            if de.file_type().unwrap().is_dir() {
                oracle(&de.path(), out);
            }
        }
    }

    fn write(p: &Path, bytes: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    #[cfg(windows)]
    fn set_hidden(p: &Path) {
        let ok = std::process::Command::new("attrib")
            .arg("+h")
            .arg(p)
            .status()
            .unwrap()
            .success();
        assert!(ok, "attrib +h failed");
    }

    #[test]
    fn every_entry_is_emitted_or_excluded() {
        let t = TempDir::new("coverage");
        let r = &t.0;
        write(&r.join("a.txt"), b"aaa");
        write(&r.join(".hidden"), b"h");
        #[cfg(windows)]
        set_hidden(&r.join(".hidden"));
        write(&r.join("docs/notes.md"), b"# notes");
        write(&r.join("docs/deep/er/x.bin"), &[0u8; 10]);
        write(&r.join("node_modules/pkg/index.js"), b"x");
        write(&r.join("$RECYCLE.BIN/S-1-5/old.txt"), b"x");
        fs::create_dir_all(r.join("empty")).unwrap();

        let mut o = opts(&[r]);
        o.exclusions.user_names.push("node_modules".into());
        let (entries, report) = run(&o);

        let mut all = BTreeSet::new();
        oracle(r, &mut all);
        let emitted = paths(&entries);
        let excluded: BTreeSet<_> = report.excluded.iter().map(|e| e.path.clone()).collect();
        // Everything the oracle sees is emitted, excluded, or inside an excluded subtree.
        for p in &all {
            let covered = emitted.contains(p) || excluded.iter().any(|x| p.starts_with(x));
            assert!(covered, "{} neither emitted nor excluded", p.display());
        }
        assert!(emitted.contains(r), "root itself is emitted");
        assert!(report.is_complete(), "{:?}", report.issues);
        assert_eq!(report.files, 4, "a.txt .hidden notes.md x.bin");
        assert_eq!(report.dirs, 5, "root docs deep er empty");
        assert_eq!(report.bytes, 3 + 1 + 7 + 10);
        assert_eq!(report.hidden, 1);
        assert_eq!(report.emitted(), entries.len() as u64);
        let by_rule = report.excluded_by_rule();
        assert_eq!(by_rule.get("name:node_modules"), Some(&1));
        assert_eq!(
            by_rule.get("system:$Recycle.Bin"),
            Some(&1),
            "case-insensitive"
        );
    }

    #[test]
    fn exclusions_can_be_disabled_and_paths_excluded() {
        let t = TempDir::new("exclusions");
        let r = &t.0;
        write(&r.join("$Recycle.Bin/x.txt"), b"x");
        write(&r.join("private/secret.txt"), b"x");
        write(&r.join("public/ok.txt"), b"x");

        let mut o = opts(&[r]);
        o.exclusions.system_defaults = false;
        o.exclusions.user_paths.push(r.join("private"));
        let (entries, report) = run(&o);
        let emitted = paths(&entries);
        assert!(emitted.contains(&r.join("$Recycle.Bin/x.txt")));
        assert!(emitted.contains(&r.join("public/ok.txt")));
        assert!(!emitted.iter().any(|p| p.starts_with(r.join("private"))));
        assert_eq!(report.excluded.len(), 1);
        assert!(report.excluded[0].rule.starts_with("path:"));
    }

    #[test]
    fn overlapping_roots_emit_each_entry_once() {
        let t = TempDir::new("overlap");
        let r = &t.0;
        write(&r.join("sub/f.txt"), b"x");
        let (entries, report) = run(&opts(&[&r.join("sub"), r, r]));
        let all: Vec<_> = entries.iter().map(|e| e.path.clone()).collect();
        let unique = paths(&entries);
        assert_eq!(all.len(), unique.len(), "duplicates emitted");
        assert_eq!(report.overlapping_roots.len(), 2);
        assert_eq!(report.files, 1);
    }

    #[test]
    fn missing_root_is_an_issue_and_file_root_is_emitted() {
        let t = TempDir::new("roots");
        let f = t.0.join("single.txt");
        write(&f, b"12345");
        let missing = t.0.join("nope");
        let (entries, report) = run(&opts(&[&f, &missing]));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryKind::File);
        assert_eq!(entries[0].size, Some(5));
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].stage, IssueStage::OpenRoot);
        assert_eq!(report.issues[0].kind, IssueKind::NotFound);
        assert!(!report.is_complete());
    }

    #[test]
    fn deep_trees_do_not_recurse() {
        let t = TempDir::new("deep");
        let mut p = t.0.clone();
        for i in 0..300 {
            p.push(format!("d{i}"));
        }
        // > 260 characters, the classic Windows MAX_PATH.
        assert!(p.as_os_str().len() > 1000);
        write(&p.join("leaf.txt"), b"x");
        let (_, report) = run(&opts(&[&t.0]));
        assert_eq!(report.dirs, 301);
        assert_eq!(report.files, 1);
        assert!(report.is_complete());
    }

    #[test]
    fn identity_is_read_when_requested() {
        let t = TempDir::new("identity");
        write(&t.0.join("a.txt"), b"a");
        write(&t.0.join("b.txt"), b"b");
        let mut o = opts(&[&t.0]);
        let (entries, _) = run(&o);
        assert!(entries.iter().all(|e| e.identity.is_none()));

        o.identity = true;
        let (entries, report) = run(&o);
        let ids: BTreeSet<_> = entries.iter().map(|e| e.identity.unwrap()).collect();
        assert_eq!(ids.len(), entries.len(), "identities are distinct");
        assert!(report.issues.is_empty());
    }

    #[test]
    fn cancellation_stops_and_is_reported() {
        let t = TempDir::new("cancel");
        write(&t.0.join("a/b/c.txt"), b"x");
        let token = CancellationToken::new();
        token.cancel();
        let report = scan(&opts(&[&t.0]), |_| {}, Some(&token));
        assert!(report.cancelled);
        assert!(!report.is_complete());
        assert_eq!(report.dirs, 1, "only the root was emitted");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_emitted_not_followed() {
        let t = TempDir::new("links");
        let r = &t.0;
        write(&r.join("real/f.txt"), b"x");
        std::os::unix::fs::symlink(r, r.join("real/loop")).unwrap();
        std::os::unix::fs::symlink(r.join("real/f.txt"), r.join("flink")).unwrap();
        std::os::unix::fs::symlink(r.join("gone"), r.join("dangling")).unwrap();
        let mut o = opts(&[r]);
        o.identity = true;
        let (entries, report) = run(&o);
        assert_eq!(report.links, 3);
        assert_eq!(report.files, 1, "loop not followed");
        assert_eq!(report.identity_skipped, 3);
        assert!(
            entries
                .iter()
                .filter(|e| e.kind == EntryKind::Symlink)
                .all(|e| e.identity.is_none())
        );
        assert!(report.is_complete(), "{:?}", report.issues);
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_names_are_emitted() {
        use std::os::unix::ffi::OsStrExt;
        let t = TempDir::new("bytes");
        let name = std::ffi::OsStr::from_bytes(b"bad-\xff-name.txt");
        write(&t.0.join(name), b"x");
        let (entries, report) = run(&opts(&[&t.0]));
        assert_eq!(report.files, 1);
        assert_eq!(report.non_unicode_paths, 1);
        assert!(entries.iter().any(|e| e.path.file_name() == Some(name)));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_is_emitted_and_reported() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempDir::new("perm");
        let locked = t.0.join("locked");
        write(&locked.join("inside.txt"), b"x");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(&locked).is_ok(); // root ignores permissions
        let (entries, report) = run(&opts(&[&t.0]));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            paths(&entries).contains(&locked),
            "the directory itself is emitted"
        );
        if !readable_anyway {
            let issue = report.blocking_issues().next().expect("issue recorded");
            assert_eq!(issue.path, locked);
            assert_eq!(issue.stage, IssueStage::ListDirectory);
            assert_eq!(issue.kind, IssueKind::PermissionDenied);
            assert!(!report.is_complete());
        }
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_emitted_not_followed() {
        let t = TempDir::new("junction");
        let r = &t.0;
        write(&r.join("real/f.txt"), b"x");
        // A junction back to the root: following it would loop forever.
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(r.join("real").join("loop"))
            .arg(r)
            .status()
            .unwrap()
            .success();
        assert!(ok, "mklink /J failed");
        let mut o = opts(&[r]);
        o.identity = true;
        let (entries, report) = run(&o);
        assert_eq!(report.links, 1);
        assert_eq!(report.files, 1, "junction not followed");
        assert_eq!(report.identity_skipped, 1);
        assert!(entries.iter().any(|e| e.kind == EntryKind::Symlink));
        assert!(report.is_complete(), "{:?}", report.issues);
    }

    #[cfg(windows)]
    #[test]
    fn names_win32_rewrites_are_emitted_with_identity() {
        let t = TempDir::new("win32names");
        let root = crate::winpath::verbatim(&std::path::absolute(&t.0).unwrap()).unwrap();
        for name in ["trailing.", "space ", "aux.txt"] {
            fs::write(root.join(name), b"x").unwrap();
        }
        let mut o = opts(&[&t.0]);
        o.identity = true;
        let (entries, report) = run(&o);
        assert_eq!(report.files, 3);
        assert!(
            entries.iter().all(|e| e.identity.is_some()),
            "{:?}",
            report.issues
        );
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        // Plain `remove_dir_all` cannot delete these names either.
        for name in ["trailing.", "space ", "aux.txt"] {
            fs::remove_file(root.join(name)).unwrap();
        }
    }
}
