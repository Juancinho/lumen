//! Indexed locations and exclusions (T111, spec `docs/specs/T111-indexed-locations.md`): one
//! typed, versioned settings value (`index.locations`) that becomes the inventory's
//! [`ScanOptions`], plus the per-location state shown to the user.
//!
//! Rules: a saved list is the only source of truth (never silently replaced); unknown fields
//! are kept on save; a value from a newer Lumen is never overwritten.

use std::path::{Path, PathBuf};

use lumen_indexer::{DEV_NOISE_NAMES, Exclusions, IssueStage, ScanOptions, ScanReport};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Settings key in `lumen.db`.
pub const SETTING_KEY: &str = "index.locations";

/// v3 (T112): literal extension exclusions. v2 (T202): `content` is meaningful;
/// v1 values (where `names` was the only possible value) upgrade to [`CONTENT_FULL`].
pub const VERSION: u32 = 3;

/// `Location::content`: catalogue names only.
pub const CONTENT_NAMES: &str = "names";
/// `Location::content`: names plus text/code content (lexical + semantic, ADR-029).
pub const CONTENT_FULL: &str = "names+content";

/// Name of the toggle for build folders next to project markers in `disabled`.
pub const BUILD_DIRS_RULE: &str = "build-dirs";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    #[serde(default)]
    pub added_ms: i64,
    /// [`CONTENT_FULL`] (default) or [`CONTENT_NAMES`]; any other value (from a newer
    /// Lumen) counts as content-indexed and is kept.
    #[serde(default = "full")]
    pub content: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn full() -> String {
    CONTENT_FULL.into()
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefaultRules {
    /// Exclude the developer-noise names ([`DEV_NOISE_NAMES`]) except `disabled` ones.
    #[serde(default = "yes")]
    pub dev_noise: bool,
    /// Exclude build folders next to a project marker (unless `disabled` has
    /// [`BUILD_DIRS_RULE`]).
    #[serde(default = "yes")]
    pub build_next_to_marker: bool,
    /// Individual defaults switched off (`.git`, `build-dirs`, …).
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for DefaultRules {
    fn default() -> Self {
        Self {
            dev_noise: true,
            build_next_to_marker: true,
            disabled: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexLocations {
    pub version: u32,
    pub locations: Vec<Location>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    #[serde(default)]
    pub exclude_names: Vec<String>,
    #[serde(default)]
    pub exclude_extensions: Vec<String>,
    #[serde(default)]
    pub default_rules: DefaultRules,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Why a stored value cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationsError {
    /// Written by a newer Lumen: use it read-only, never overwrite it.
    NewerVersion(u32),
    Invalid(String),
}

impl std::fmt::Display for LocationsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewerVersion(v) => write!(f, "settings version {v} is newer than {VERSION}"),
            Self::Invalid(e) => write!(f, "invalid locations setting: {e}"),
        }
    }
}

impl std::error::Error for LocationsError {}

/// Case-insensitive on Windows, like the file system; trailing separators ignored except
/// for drive roots (`D:\`).
fn key(path: &str) -> String {
    let trimmed = path.trim();
    let t = if trimmed.len() > 3 {
        trimmed.trim_end_matches(['\\', '/'])
    } else {
        trimmed
    };
    if cfg!(windows) {
        t.to_lowercase()
    } else {
        t.to_owned()
    }
}

fn same(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

/// `path` equals `dir` or lies inside it.
fn within(path: &str, dir: &str) -> bool {
    let (p, d) = (key(path), key(dir));
    let d = d.trim_end_matches(['\\', '/']);
    p == d
        || p.strip_prefix(d)
            .is_some_and(|r| r.starts_with(['\\', '/']))
}

impl IndexLocations {
    /// First run: the standard folders, as ordinary entries the user can remove.
    #[must_use]
    pub fn standard(folders: &[PathBuf], now_ms: i64) -> Self {
        let mut me = Self {
            version: VERSION,
            locations: Vec::new(),
            exclude_paths: Vec::new(),
            exclude_names: Vec::new(),
            exclude_extensions: Vec::new(),
            default_rules: DefaultRules::default(),
            extra: Map::new(),
        };
        for f in folders {
            me.add_location(f, now_ms, &[]);
        }
        me
    }

    /// Reads a stored value.
    ///
    /// # Errors
    /// [`LocationsError::NewerVersion`] for a value from a newer build, `Invalid` otherwise.
    pub fn parse(json: &str) -> Result<Self, LocationsError> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| LocationsError::Invalid(e.to_string()))?;
        let version = value
            .get("version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| LocationsError::Invalid("missing version".into()))?;
        if version > VERSION {
            return Err(LocationsError::NewerVersion(version));
        }
        let mut me: Self =
            serde_json::from_value(value).map_err(|e| LocationsError::Invalid(e.to_string()))?;
        if version < 2 {
            // v1 could only say `names` (content indexing did not exist): not a choice.
            for l in &mut me.locations {
                l.content = full();
            }
        }
        me.version = VERSION;
        let mut extensions = Vec::new();
        for extension in &me.exclude_extensions {
            let normalized = lumen_indexer::scan::normalize_extension(extension)
                .ok_or_else(|| LocationsError::Invalid("invalid excluded extension".into()))?;
            if !extensions.contains(&normalized) {
                extensions.push(normalized);
            }
        }
        me.exclude_extensions = extensions;
        Ok(me)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    #[must_use]
    pub fn roots(&self) -> Vec<PathBuf> {
        self.locations
            .iter()
            .map(|l| PathBuf::from(&l.path))
            .collect()
    }

    /// The default names currently excluded.
    #[must_use]
    pub fn active_default_names(&self) -> Vec<String> {
        if !self.default_rules.dev_noise {
            return Vec::new();
        }
        DEV_NOISE_NAMES
            .iter()
            .filter(|n| !self.default_rules.disabled.iter().any(|d| d == *n))
            .map(|n| (*n).to_owned())
            .collect()
    }

    #[must_use]
    pub fn build_dirs_excluded(&self) -> bool {
        self.default_rules.build_next_to_marker
            && !self
                .default_rules
                .disabled
                .iter()
                .any(|d| d == BUILD_DIRS_RULE)
    }

    /// Inventory options for these locations.
    #[must_use]
    pub fn scan_options(&self, identity: bool) -> ScanOptions {
        ScanOptions {
            roots: self.roots(),
            exclusions: Exclusions {
                system_defaults: true,
                user_paths: self.exclude_paths.iter().map(PathBuf::from).collect(),
                user_names: self.exclude_names.clone(),
                user_extensions: self.exclude_extensions.clone(),
                default_names: self.active_default_names(),
                build_dirs_next_to_markers: self.build_dirs_excluded(),
            },
            identity,
        }
    }

    /// Adds a folder or drive; `prefill` are exclusions added with it (the system folders
    /// when the location is the system drive). Returns `false` if it is already listed.
    pub fn add_location(&mut self, path: &Path, now_ms: i64, prefill: &[PathBuf]) -> bool {
        let text = path.to_string_lossy().into_owned();
        if self.locations.iter().any(|l| same(&l.path, &text)) {
            return false;
        }
        self.locations.push(Location {
            path: text,
            added_ms: now_ms,
            content: full(),
            extra: Map::new(),
        });
        for p in prefill {
            self.exclude_path(p);
        }
        true
    }

    /// Returns `false` if it was not listed.
    pub fn remove_location(&mut self, path: &str) -> bool {
        let before = self.locations.len();
        self.locations.retain(|l| !same(&l.path, path));
        self.locations.len() != before
    }

    /// Excludes an exact file path or a folder subtree. False if already excluded.
    pub fn exclude_path(&mut self, path: &Path) -> bool {
        let text = path.to_string_lossy().into_owned();
        if self.exclude_paths.iter().any(|p| same(p, &text)) {
            return false;
        }
        self.exclude_paths.push(text);
        true
    }

    pub fn unexclude_path(&mut self, path: &str) -> bool {
        let before = self.exclude_paths.len();
        self.exclude_paths.retain(|p| !same(p, path));
        self.exclude_paths.len() != before
    }

    /// Toggle one literal extension; false for invalid input or an unchanged rule.
    pub fn set_extension_excluded(&mut self, extension: &str, excluded: bool) -> bool {
        let Some(extension) = lumen_indexer::scan::normalize_extension(extension) else {
            return false;
        };
        let has = self.exclude_extensions.contains(&extension);
        if has == excluded {
            return false;
        }
        if excluded {
            self.exclude_extensions.push(extension);
        } else {
            self.exclude_extensions.retain(|e| e != &extension);
        }
        true
    }

    /// User rules against known catalog metadata; no disk reads or default-rule probing.
    #[must_use]
    pub fn user_excludes(&self, path: &Path, is_dir: bool) -> bool {
        self.exclude_paths
            .iter()
            .any(|e| within(&path.to_string_lossy(), e))
            || (!self.exclude_names.is_empty()
                && path
                    .ancestors()
                    .take_while(|p| {
                        // Full inventory only visits the selected root and its descendants.
                        self.locations
                            .iter()
                            .any(|l| within(&p.to_string_lossy(), &l.path))
                    })
                    .any(|p| {
                        p.file_name().is_some_and(|n| {
                            self.exclude_names
                                .iter()
                                .any(|e| n.to_string_lossy().to_lowercase() == e.to_lowercase())
                        })
                    }))
            || (!is_dir
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .and_then(lumen_indexer::scan::normalize_extension)
                    .is_some_and(|e| self.exclude_extensions.contains(&e)))
    }

    /// Switches one default rule (a [`DEV_NOISE_NAMES`] entry or [`BUILD_DIRS_RULE`]).
    pub fn set_default_enabled(&mut self, rule: &str, enabled: bool) {
        self.default_rules.disabled.retain(|d| d != rule);
        if !enabled {
            self.default_rules.disabled.push(rule.to_owned());
        }
    }

    #[must_use]
    pub fn default_enabled(&self, rule: &str) -> bool {
        let group = if rule == BUILD_DIRS_RULE {
            self.default_rules.build_next_to_marker
        } else {
            self.default_rules.dev_noise
        };
        group && !self.default_rules.disabled.iter().any(|d| d == rule)
    }

    /// Whether the content of files at `path` is indexed: the innermost location holding
    /// it is not names-only (and the path is covered at all).
    #[must_use]
    pub fn indexes_content(&self, path: &str) -> bool {
        self.covers(path)
            && !self.user_excludes(Path::new(path), false)
            && self
                .locations
                .iter()
                .filter(|l| within(path, &l.path))
                .max_by_key(|l| key(&l.path).len())
                .is_some_and(|l| l.content != CONTENT_NAMES)
    }

    /// Switches content indexing for one location. Returns `false` if not listed or
    /// unchanged.
    pub fn set_content(&mut self, path: &str, enabled: bool) -> bool {
        let value = if enabled { CONTENT_FULL } else { CONTENT_NAMES };
        match self.locations.iter_mut().find(|l| same(&l.path, path)) {
            Some(l) if (l.content != CONTENT_NAMES) != enabled => {
                l.content = value.to_owned();
                true
            }
            _ => false,
        }
    }

    /// Whether `path` is inside a location and not inside an excluded folder.
    #[must_use]
    pub fn covers(&self, path: &str) -> bool {
        self.locations.iter().any(|l| within(path, &l.path))
            && !self.exclude_paths.iter().any(|e| within(path, e))
    }
}

/// What the last pass found for one location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationState {
    /// Listed completely.
    Ok,
    /// Drive disconnected, share offline or folder missing: its items are kept.
    NotAvailable,
    /// Some directories could not be listed (their items are kept).
    Partial { unlisted: u64 },
}

/// The state of every root of `report`'s pass, in `roots` order.
#[must_use]
pub fn location_states(roots: &[PathBuf], report: &ScanReport) -> Vec<LocationState> {
    let abs = |p: &Path| {
        std::path::absolute(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .into_owned()
    };
    roots
        .iter()
        .map(|root| {
            let r = abs(root);
            let missing = report
                .issues
                .iter()
                .any(|i| i.stage == IssueStage::OpenRoot && same(&abs(&i.path), &r));
            if missing {
                return LocationState::NotAvailable;
            }
            let unlisted = report
                .blocking_issues()
                .filter(|i| within(&abs(&i.path), &r))
                .count() as u64;
            if unlisted > 0 {
                LocationState::Partial { unlisted }
            } else {
                LocationState::Ok
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_round_trip_keeps_unknown_fields() {
        let mut l = IndexLocations::standard(&[PathBuf::from("/home/joao/Documents")], 5);
        l.extra.insert("future".into(), Value::Bool(true));
        l.locations[0]
            .extra
            .insert("schedule".into(), Value::String("nightly".into()));
        let back = IndexLocations::parse(&l.to_json()).unwrap();
        assert_eq!(back, l);
        assert_eq!(back.locations[0].content, CONTENT_FULL);
        assert!(back.default_rules.dev_noise && back.build_dirs_excluded());
    }

    #[test]
    fn newer_or_broken_values_are_refused() {
        assert_eq!(
            IndexLocations::parse(r#"{"version":4,"locations":[]}"#),
            Err(LocationsError::NewerVersion(4))
        );
        assert!(matches!(
            IndexLocations::parse("not json"),
            Err(LocationsError::Invalid(_))
        ));
        // Old minimal value: defaults filled in; v1 `names` upgrades to content.
        let l = IndexLocations::parse(
            r#"{"version":1,"locations":[{"path":"D:\\Proyectos","content":"names"}]}"#,
        )
        .unwrap();
        assert_eq!(
            (l.version, l.locations[0].content.as_str()),
            (VERSION, CONTENT_FULL)
        );
        assert!(l.default_rules.dev_noise);
        // v2 keeps a names-only choice.
        let l = IndexLocations::parse(
            r#"{"version":2,"locations":[{"path":"D:\\Fotos","content":"names"}]}"#,
        )
        .unwrap();
        assert_eq!(l.locations[0].content, CONTENT_NAMES);
    }

    #[test]
    fn edits_dedupe_and_prefill_system_exclusions() {
        let mut l = IndexLocations::standard(&[], 0);
        assert!(l.add_location(Path::new("/data"), 1, &[]));
        assert!(!l.add_location(Path::new("/data/"), 2, &[]), "same folder");
        assert!(l.add_location(
            Path::new("/"),
            3,
            &[PathBuf::from("/usr"), PathBuf::from("/proc")]
        ));
        assert_eq!(l.exclude_paths, ["/usr", "/proc"]);
        assert!(l.covers("/data/x.txt"));
        assert!(!l.covers("/usr/bin/ls"), "excluded wins");
        assert!(l.remove_location("/data"));
        assert!(!l.remove_location("/data"));
        assert!(l.unexclude_path("/proc"));
        assert_eq!(l.roots(), [PathBuf::from("/")]);
    }

    #[test]
    fn content_follows_the_innermost_location() {
        let mut l = IndexLocations::standard(&[PathBuf::from("/d")], 0);
        l.add_location(Path::new("/d/media"), 1, &[]);
        assert!(l.set_content("/d/media", false));
        assert!(!l.set_content("/d/media", false), "unchanged");
        assert!(!l.set_content("/elsewhere", true), "not listed");
        assert!(l.indexes_content("/d/notes.md"));
        assert!(!l.indexes_content("/d/media/song.txt"));
        assert!(!l.indexes_content("/other/x.md"));
        l.exclude_path(Path::new("/d/private"));
        assert!(!l.indexes_content("/d/private/a.md"));
    }

    #[test]
    fn v2_extensions_upgrade_and_manual_rules_are_reversible() {
        let mut l = IndexLocations::parse(
            r#"{"version":2,"locations":[{"path":"/d","content":"names"}],"future":true,"exclude_extensions":[".JS","js","LOG"]}"#,
        ).unwrap();
        assert_eq!(l.version, VERSION);
        assert_eq!(l.locations[0].content, CONTENT_NAMES);
        assert_eq!(l.exclude_extensions, ["js", "log"]);
        assert_eq!(l.extra["future"], Value::Bool(true));
        assert!(l.set_content("/d", true));
        assert!(!l.set_extension_excluded(".Js", true));
        assert!(!l.set_extension_excluded("*.json", true));
        assert!(l.user_excludes(Path::new("/d/app.JS"), false));
        assert!(!l.user_excludes(Path::new("/d/folder.js"), true));
        assert!(!l.user_excludes(Path::new("/d/app.jsx"), false));
        assert!(!l.indexes_content("/d/app.js"));
        assert!(l.set_extension_excluded("JS", false));
        assert!(l.indexes_content("/d/app.js"));
        l.exclude_path(Path::new("/d/one.json"));
        assert!(l.user_excludes(Path::new("/d/one.json"), false));
        assert!(!l.user_excludes(Path::new("/d/one.jsonl"), false));
        l.exclude_names.push("private".into());
        assert!(l.user_excludes(Path::new("/d/Private/old.md"), false));
        let mut nested = IndexLocations::standard(&[PathBuf::from("/d/Private/docs")], 0);
        nested.exclude_names.push("private".into());
        assert!(
            !nested.user_excludes(Path::new("/d/Private/docs/old.md"), false),
            "ancestor above an explicit root is not visited by inventory"
        );
        assert!(nested.user_excludes(Path::new("/d/Private/docs/Private/old.md"), false));
        assert_eq!(IndexLocations::parse(&l.to_json()).unwrap(), l);
        assert!(matches!(
            IndexLocations::parse(r#"{"version":3,"locations":[],"exclude_extensions":["../js"]}"#),
            Err(LocationsError::Invalid(_))
        ));
    }

    #[test]
    fn default_rules_toggle_individually() {
        let mut l = IndexLocations::standard(&[], 0);
        assert!(
            l.active_default_names()
                .contains(&"node_modules".to_owned())
        );
        l.set_default_enabled(".git", false);
        assert!(!l.active_default_names().contains(&".git".to_owned()));
        assert!(!l.default_enabled(".git"));
        l.set_default_enabled(BUILD_DIRS_RULE, false);
        assert!(!l.scan_options(false).exclusions.build_dirs_next_to_markers);
        l.set_default_enabled(".git", true);
        assert!(l.default_enabled(".git"));
        l.default_rules.dev_noise = false;
        assert!(l.scan_options(false).exclusions.default_names.is_empty());
    }

    #[test]
    fn states_from_a_scan_report() {
        use lumen_indexer::{IssueKind, ScanIssue};
        let roots = [
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ];
        let issue = |p: &str, stage| ScanIssue {
            path: PathBuf::from(p),
            stage,
            kind: IssueKind::Other,
            message: String::new(),
        };
        let report = ScanReport {
            issues: vec![
                issue("/b", IssueStage::OpenRoot),
                issue("/c/x", IssueStage::ListDirectory),
                issue("/c/y", IssueStage::ListDirectory),
                issue("/a/z.txt", IssueStage::ReadMetadata), // degraded, not hidden
            ],
            ..ScanReport::default()
        };
        assert_eq!(
            location_states(&roots, &report),
            [
                LocationState::Ok,
                LocationState::NotAvailable,
                LocationState::Partial { unlisted: 2 }
            ]
        );
    }
}
