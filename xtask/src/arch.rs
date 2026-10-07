//! Dependency-direction check (ADR-002).
//!
//! Rules, applied to every workspace member located under `<root>/crates/`
//! ("core crates"), across all dependency kinds (normal, build, dev) and all
//! target platforms, transitively:
//!
//! 1. no presentation-shell crate (Tauri, WebView, native GUI toolkits);
//! 2. no workspace member located under `<root>/apps/`.
//!
//! The direction is therefore `apps/* -> crates/*`, never the reverse.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::Path;

use serde_json::Value;

/// Exact crate names that belong to a presentation shell.
const FORBIDDEN_EXACT: &[&str] = &[
    "tauri",
    "wry",
    "tao",
    "webview2",
    "webview2-com",
    "webview2-com-sys",
    "webkit2gtk",
    "webkit2gtk-sys",
    "javascriptcore-rs",
    "gtk",
    "gtk-sys",
    "muda",
    "tray-icon",
    "egui",
    "eframe",
    "winit",
];

/// Crate-name prefixes that belong to a presentation shell (`tauri-build`, `tauri-plugin-*`, ...).
const FORBIDDEN_PREFIXES: &[&str] = &["tauri-", "tauri_"];

pub(crate) fn is_forbidden_crate(name: &str) -> bool {
    FORBIDDEN_EXACT.contains(&name) || FORBIDDEN_PREFIXES.iter().any(|p| name.starts_with(p))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Violation {
    /// Dependency chain from the core crate to the offending crate (names).
    pub(crate) chain: Vec<String>,
    pub(crate) reason: Reason,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    ShellCrate,
    AppsMember,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.reason {
            Reason::ShellCrate => "presentation-shell crate",
            Reason::AppsMember => "crate under apps/",
        };
        write!(f, "{} ({what})", self.chain.join(" -> "))
    }
}

#[derive(Debug)]
pub(crate) struct Report {
    pub(crate) core_crates: Vec<String>,
    pub(crate) violations: Vec<Violation>,
}

struct Package<'a> {
    name: &'a str,
    manifest_path: &'a Path,
}

/// Runs the check against `cargo metadata --format-version 1` output.
pub(crate) fn check(metadata: &Value) -> Result<Report, String> {
    let root = metadata["workspace_root"]
        .as_str()
        .map(Path::new)
        .ok_or("metadata has no `workspace_root`")?;
    let crates_dir = root.join("crates");
    let apps_dir = root.join("apps");

    let mut packages: HashMap<&str, Package<'_>> = HashMap::new();
    for pkg in metadata["packages"]
        .as_array()
        .ok_or("metadata has no `packages`")?
    {
        let id = pkg["id"].as_str().ok_or("package without `id`")?;
        let name = pkg["name"].as_str().ok_or("package without `name`")?;
        let manifest_path = pkg["manifest_path"]
            .as_str()
            .map(Path::new)
            .ok_or("package without `manifest_path`")?;
        packages.insert(
            id,
            Package {
                name,
                manifest_path,
            },
        );
    }

    let members: Vec<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or("metadata has no `workspace_members`")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let member_set: HashSet<&str> = members.iter().copied().collect();
    let is_under = |id: &str, dir: &Path| {
        packages
            .get(id)
            .is_some_and(|p| p.manifest_path.starts_with(dir))
    };

    let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("metadata has no `resolve.nodes` (was --no-deps used?)")?
    {
        let id = node["id"].as_str().ok_or("resolve node without `id`")?;
        let deps = node["deps"]
            .as_array()
            .map(|deps| deps.iter().filter_map(|d| d["pkg"].as_str()).collect())
            .unwrap_or_default();
        edges.insert(id, deps);
    }

    let mut core_ids: Vec<&str> = members
        .iter()
        .copied()
        .filter(|id| is_under(id, &crates_dir))
        .collect();
    core_ids.sort_by_key(|id| packages.get(id).map(|p| p.name));
    if core_ids.is_empty() {
        // Guard against the policy silently matching nothing (e.g. a layout change).
        return Err(format!(
            "no workspace members found under {}; refusing to report success",
            crates_dir.display()
        ));
    }

    let name_of = |id: &str| packages.get(id).map_or(id, |p| p.name).to_owned();
    let mut violations = Vec::new();
    for &core in &core_ids {
        // BFS so each reported chain is a shortest path.
        let mut parent: HashMap<&str, &str> = HashMap::new();
        let mut seen: HashSet<&str> = HashSet::from([core]);
        let mut queue = VecDeque::from([core]);
        while let Some(current) = queue.pop_front() {
            for &dep in edges.get(current).map(Vec::as_slice).unwrap_or_default() {
                if !seen.insert(dep) {
                    continue;
                }
                parent.insert(dep, current);
                let reason = if member_set.contains(dep) && is_under(dep, &apps_dir) {
                    Some(Reason::AppsMember)
                } else if packages
                    .get(dep)
                    .is_some_and(|p| is_forbidden_crate(p.name))
                {
                    Some(Reason::ShellCrate)
                } else {
                    None
                };
                match reason {
                    Some(reason) => {
                        let mut chain = vec![name_of(dep)];
                        let mut cursor = dep;
                        while let Some(&up) = parent.get(cursor) {
                            chain.push(name_of(up));
                            cursor = up;
                        }
                        chain.reverse();
                        violations.push(Violation { chain, reason });
                        // Do not descend: one report per offending edge is enough.
                    }
                    None => queue.push_back(dep),
                }
            }
        }
    }

    // Declared dependencies too: cargo drops some invalid edges from `resolve` (e.g. a
    // dependency on a bin-only crate) with only a warning. Intent still violates the rule.
    for &core in &core_ids {
        let Some(manifest) = metadata["packages"]
            .as_array()
            .and_then(|pkgs| pkgs.iter().find(|p| p["id"].as_str() == Some(core)))
        else {
            continue;
        };
        for dep in manifest["dependencies"].as_array().into_iter().flatten() {
            let Some(dep_name) = dep["name"].as_str() else {
                continue;
            };
            let reason = if dep["path"]
                .as_str()
                .is_some_and(|path| Path::new(path).starts_with(&apps_dir))
            {
                Reason::AppsMember
            } else if is_forbidden_crate(dep_name) {
                Reason::ShellCrate
            } else {
                continue;
            };
            let chain = vec![name_of(core), dep_name.to_owned()];
            if !violations.iter().any(|v: &Violation| v.chain == chain) {
                violations.push(Violation { chain, reason });
            }
        }
    }

    Ok(Report {
        core_crates: core_ids.into_iter().map(name_of).collect(),
        violations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pkg(name: &str, path: &str) -> Value {
        json!({ "id": format!("{name}-id"), "name": name, "manifest_path": format!("{path}/Cargo.toml") })
    }

    fn node(name: &str, deps: &[&str]) -> Value {
        let deps: Vec<Value> = deps
            .iter()
            .map(|d| json!({ "pkg": format!("{d}-id") }))
            .collect();
        json!({ "id": format!("{name}-id"), "deps": deps })
    }

    /// Workspace: core (crates/) <- shell (apps/), plus a registry crate graph.
    fn metadata(core_deps: &[&str], extra_nodes: Vec<Value>) -> Value {
        let mut nodes = vec![
            node("lumen-core", core_deps),
            node("lumen-desktop", &["lumen-core", "tauri"]),
            node("tauri", &["wry"]),
            node("wry", &[]),
            node("helper", &["tauri-utils"]),
            node("tauri-utils", &[]),
            node("serde", &[]),
        ];
        nodes.extend(extra_nodes);
        json!({
            "workspace_root": "/ws",
            "workspace_members": ["lumen-core-id", "lumen-desktop-id"],
            "packages": [
                pkg("lumen-core", "/ws/crates/lumen-core"),
                pkg("lumen-desktop", "/ws/apps/desktop/src-tauri"),
                pkg("tauri", "/registry/tauri"),
                pkg("wry", "/registry/wry"),
                pkg("helper", "/registry/helper"),
                pkg("tauri-utils", "/registry/tauri-utils"),
                pkg("serde", "/registry/serde"),
            ],
            "resolve": { "nodes": nodes }
        })
    }

    #[test]
    fn clean_core_passes_even_though_shell_uses_tauri() {
        let report = check(&metadata(&["serde"], vec![])).unwrap();
        assert_eq!(report.core_crates, vec!["lumen-core"]);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn direct_shell_dependency_is_rejected() {
        let report = check(&metadata(&["tauri"], vec![])).unwrap();
        assert_eq!(
            report.violations,
            vec![Violation {
                chain: vec!["lumen-core".into(), "tauri".into()],
                reason: Reason::ShellCrate
            }]
        );
    }

    #[test]
    fn transitive_shell_dependency_reports_chain() {
        let report = check(&metadata(&["helper"], vec![])).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(
            report.violations[0].to_string(),
            "lumen-core -> helper -> tauri-utils (presentation-shell crate)"
        );
    }

    #[test]
    fn depending_on_an_apps_member_is_rejected() {
        let report = check(&metadata(&["lumen-desktop"], vec![])).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].reason, Reason::AppsMember);
    }

    #[test]
    fn declared_but_unresolved_apps_dependency_is_rejected() {
        // cargo drops dev-deps on bin-only crates from `resolve`; the manifest still declares it.
        let mut meta = metadata(&[], vec![]);
        meta["packages"][0]["dependencies"] = json!([
            { "name": "serde", "kind": null },
            { "name": "lumen-desktop", "kind": "dev", "path": "/ws/apps/desktop/src-tauri" }
        ]);
        let report = check(&meta).unwrap();
        assert_eq!(
            report.violations,
            vec![Violation {
                chain: vec!["lumen-core".into(), "lumen-desktop".into()],
                reason: Reason::AppsMember
            }]
        );
    }

    #[test]
    fn declared_and_resolved_violation_is_reported_once() {
        let mut meta = metadata(&["tauri"], vec![]);
        meta["packages"][0]["dependencies"] = json!([{ "name": "tauri", "kind": null }]);
        assert_eq!(check(&meta).unwrap().violations.len(), 1);
    }

    #[test]
    fn missing_core_crates_is_an_error_not_a_pass() {
        let mut meta = metadata(&[], vec![]);
        meta["workspace_members"] = json!(["lumen-desktop-id"]);
        assert!(check(&meta).is_err());
    }

    #[test]
    fn forbidden_names() {
        for name in [
            "tauri",
            "tauri-build",
            "tauri-plugin-shell",
            "wry",
            "webview2-com",
            "egui",
        ] {
            assert!(is_forbidden_crate(name), "{name} should be forbidden");
        }
        for name in ["serde", "taurine", "windows", "rusqlite", "usearch"] {
            assert!(!is_forbidden_crate(name), "{name} should be allowed");
        }
    }
}
