//! Staged, verified, atomic installation (ADR-034).
//!
//! Layout under the provisioning root (the app-data folder):
//!
//! ```text
//! <root>/<component id>/<version>/…          installed files + installed.json
//! <root>/<component id>/<version>.partial/   staging: verified files so far, .download/
//! ```
//!
//! Every download is checked against its pinned size and SHA-256 before it is used; an
//! archive member is checked again after extraction. The staging folder becomes the
//! installed folder with one rename after the marker is written, so a crash or a cancel
//! leaves either nothing usable or the previous state — and the next attempt resumes.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use lumen_core::CancellationToken;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fetch::{Fetch, FetchError, url_file_name};
use crate::manifest::{Component, Install};
use crate::zip;

const MARKER: &str = "installed.json";
const DOWNLOADS: &str = ".download";

#[derive(Debug)]
pub enum InstallError {
    /// The component has no build for this platform.
    Unsupported,
    Cancelled,
    Download(String),
    /// A file did not match its pinned size or hash (it was deleted).
    Corrupt(String),
    Io(std::io::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("not available for this platform"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Download(why) => write!(f, "download failed: {why}"),
            Self::Corrupt(what) => write!(f, "{what} did not match its checksum"),
            Self::Io(e) => write!(f, "disk error: {e}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<FetchError> for InstallError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Cancelled => Self::Cancelled,
            FetchError::Failed(why) => Self::Download(why),
        }
    }
}

/// Where a component stands on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    /// A previous attempt left this many verified or downloaded bytes (resumable).
    Partial {
        bytes: u64,
    },
    Installed {
        dir: PathBuf,
    },
}

/// Download progress over the whole component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    id: String,
    version: String,
    files: Vec<(String, u64, String)>,
}

/// Folder of the installed component.
#[must_use]
pub fn component_dir(root: &Path, c: &Component) -> PathBuf {
    rel(&root.join(c.id), c.version)
}

fn staging_dir(root: &Path, c: &Component) -> PathBuf {
    rel(&root.join(c.id), &format!("{}.partial", c.version))
}

fn rel(base: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(base.to_owned(), |p, part| p.join(part))
}

/// Every file the installed folder holds: (path inside, size, sha256).
fn expected(c: &Component) -> Vec<(&'static str, u64, &'static str)> {
    c.files
        .iter()
        .flat_map(|f| match f.install {
            Install::As(dest) => vec![(dest, f.size, f.sha256)],
            Install::Extract(members) => {
                members.iter().map(|m| (m.dest, m.size, m.sha256)).collect()
            }
        })
        .collect()
}

/// SHA-256 of a file, lowercase hex.
///
/// # Errors
/// I/O failure.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn matches(path: &Path, size: u64, sha256: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() == size)
        && sha256_file(path).is_ok_and(|h| h == sha256)
}

/// Current state. `Installed` needs the marker of this version and every file at its
/// expected size (hashes were checked at install; [`verify`] re-checks them).
#[must_use]
pub fn state(root: &Path, c: &Component) -> State {
    let dir = component_dir(root, c);
    let marker: Option<Marker> = fs::read_to_string(dir.join(MARKER))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    if marker.is_some_and(|m| m.id == c.id && m.version == c.version)
        && expected(c)
            .iter()
            .all(|(p, size, _)| fs::metadata(rel(&dir, p)).is_ok_and(|m| m.len() == *size))
    {
        return State::Installed { dir };
    }
    let staging = staging_dir(root, c);
    if staging.is_dir() {
        return State::Partial {
            bytes: dir_bytes(&staging),
        };
    }
    State::NotInstalled
}

fn dir_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => dir_bytes(&e.path()),
                Ok(_) => e.metadata().map_or(0, |m| m.len()),
                Err(_) => 0,
            })
            .sum()
    })
}

/// Re-hashes every installed file. Returns the paths (inside the component) that differ.
///
/// # Errors
/// The component is not installed.
pub fn verify(root: &Path, c: &Component) -> Result<Vec<&'static str>, InstallError> {
    let State::Installed { dir } = state(root, c) else {
        return Err(InstallError::Corrupt(format!(
            "{} (not installed)",
            c.title
        )));
    };
    Ok(expected(c)
        .into_iter()
        .filter(|(p, size, sha)| !matches(&rel(&dir, p), *size, sha))
        .map(|(p, _, _)| p)
        .collect())
}

/// Installs `c` under `root` (resuming a previous attempt), returns its folder.
///
/// # Errors
/// See [`InstallError`]; a cancelled or failed run keeps what was verified for the next.
pub fn install(
    root: &Path,
    c: &Component,
    fetch: &dyn Fetch,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, InstallError> {
    if !c.platform_ok {
        return Err(InstallError::Unsupported);
    }
    if let State::Installed { dir } = state(root, c) {
        return Ok(dir);
    }
    let total = c.download_bytes();
    let staging = staging_dir(root, c);
    let downloads = staging.join(DOWNLOADS);
    fs::create_dir_all(&downloads)?;
    let mut done_before = 0;
    for file in c.files {
        let already = match file.install {
            Install::As(dest) => matches(&rel(&staging, dest), file.size, file.sha256),
            Install::Extract(members) => members
                .iter()
                .all(|m| matches(&rel(&staging, m.dest), m.size, m.sha256)),
        };
        if !already {
            let name = url_file_name(file.url);
            let download = downloads.join(name);
            if fs::metadata(&download).is_ok_and(|m| m.len() > file.size) {
                fs::remove_file(&download)?;
            }
            if !matches(&download, file.size, file.sha256) {
                if fs::metadata(&download).is_ok_and(|m| m.len() == file.size) {
                    // Complete but wrong: start over.
                    fs::remove_file(&download)?;
                }
                let base = done_before;
                fetch.fetch(file.url, &download, cancel, &mut |b| {
                    progress(Progress {
                        done: base + b.min(file.size),
                        total,
                    });
                })?;
                if !matches(&download, file.size, file.sha256) {
                    let _ = fs::remove_file(&download);
                    return Err(InstallError::Corrupt(name.to_owned()));
                }
            }
            match file.install {
                Install::As(dest) => {
                    let to = rel(&staging, dest);
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(&download, &to)?;
                }
                Install::Extract(members) => {
                    let mut archive = File::open(&download)?;
                    let entries = zip::entries(&mut archive)?;
                    for m in members {
                        if cancel.is_cancelled() {
                            return Err(InstallError::Cancelled);
                        }
                        let entry = entries
                            .iter()
                            .find(|e| e.name == m.name)
                            .ok_or_else(|| InstallError::Corrupt(format!("{name}: {}", m.name)))?;
                        let to = rel(&staging, m.dest);
                        if let Some(parent) = to.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        let tmp = to.with_extension("extract");
                        let mut out = File::create(&tmp)?;
                        zip::extract(&mut archive, entry, &mut out)?;
                        drop(out);
                        if !matches(&tmp, m.size, m.sha256) {
                            let _ = fs::remove_file(&tmp);
                            return Err(InstallError::Corrupt(m.dest.to_owned()));
                        }
                        fs::rename(&tmp, &to)?;
                    }
                    drop(archive);
                    fs::remove_file(&download)?;
                }
            }
        }
        done_before += file.size;
        progress(Progress {
            done: done_before,
            total,
        });
    }
    let _ = fs::remove_dir_all(&downloads);
    let marker = Marker {
        id: c.id.to_owned(),
        version: c.version.to_owned(),
        files: expected(c)
            .into_iter()
            .map(|(p, s, h)| (p.to_owned(), s, h.to_owned()))
            .collect(),
    };
    fs::write(
        staging.join(MARKER),
        serde_json::to_vec_pretty(&marker).map_err(|e| InstallError::Io(e.into()))?,
    )?;
    let dir = component_dir(root, c);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::rename(&staging, &dir)?;
    remove_other_versions(root, c);
    Ok(dir)
}

/// Older versions of the component (a pinned revision changed) are deleted after an
/// install; failures (files in use) are left for the next install.
fn remove_other_versions(root: &Path, c: &Component) {
    let Ok(entries) = fs::read_dir(root.join(c.id)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != c.version && !name.starts_with(&format!("{}.", c.version)) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

/// Deletes the component (installed, staged and older versions).
///
/// # Errors
/// I/O failure (e.g. a DLL still loaded on Windows).
pub fn remove(root: &Path, c: &Component) -> std::io::Result<()> {
    let dir = root.join(c.id);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}
