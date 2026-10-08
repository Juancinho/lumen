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

/// Schema version this build reads and writes.
pub const VERSION: u32 = 1;

/// Name of the toggle for build folders next to project markers in `disabled`.
pub const BUILD_DIRS_RULE: &str = "build-dirs";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    #[serde(default)]
    pub added_ms: i64,
    /// `names` now; `names+content` once content indexing exists (M2).
    #[serde(default = "names")]
    pub content: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn names() -> String {
    "names".into()
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
        serde_json::from_value(value).map_err(|e| LocationsError::Invalid(e.to_string()))
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
            content: names(),
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

    /// Excludes a folder subtree. Returns `false` if it was already excluded.
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
        assert_eq!(back.locations[0].content, "names");
        assert!(back.default_rules.dev_noise && back.build_dirs_excluded());
    }

    #[test]
    fn newer_or_broken_values_are_refused() {
        assert_eq!(
            IndexLocations::parse(r#"{"version":2,"locations":[]}"#),
            Err(LocationsError::NewerVersion(2))
        );
        assert!(matches!(
            IndexLocations::parse("not json"),
            Err(LocationsError::Invalid(_))
        ));
        // Old minimal value: defaults filled in.
        let l = IndexLocations::parse(r#"{"version":1,"locations":[{"path":"D:\\Proyectos"}]}"#)
            .unwrap();
        assert_eq!(l.locations[0].content, "names");
        assert!(l.default_rules.dev_noise);
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
