//! Native notifications are bounded hints, never the source of catalog truth (ADR-037).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Maximum distinct paths awaiting reconciliation; overflow requests a full inventory.
pub const MAX_PATHS: usize = 4_096;
pub const QUIET: Duration = Duration::from_millis(300);
pub const MAX_DELAY: Duration = Duration::from_secs(2);
/// Recovery inventories are rate limited under persistent native-buffer overflow.
pub const RECOVERY_DELAY: Duration = Duration::from_secs(5);
pub type Notification = notify::Result<Event>;

/// A parent's directory-mtime notification is not loss of the recursive root handle.
#[must_use]
pub fn root_lifecycle(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Any
            | EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    /// Creation, removal or rename may affect an entire subtree.
    pub recursive: bool,
    /// A write notification invalidates content even if size/mtime did not change.
    pub content: bool,
    /// Old side of a rename; useful for case-only Windows renames where the old spelling
    /// still resolves to the same file. The destination is inventoried first/alone.
    pub renamed_from: bool,
}

#[derive(Debug, Default)]
pub struct Pending {
    paths: BTreeMap<PathBuf, Change>,
    rescan: bool,
    first: Option<Instant>,
    last: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct Batch {
    pub changes: Vec<Change>,
    pub rescan: bool,
}

impl Pending {
    /// Called under the shell's control lock; no filesystem or database work here.
    pub fn push(&mut self, event: notify::Result<Event>, now: Instant) -> bool {
        let Ok(event) = event else {
            self.require_rescan(now);
            return true;
        };
        if event.need_rescan() {
            self.require_rescan(now);
            return true;
        }
        if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
            return false;
        }
        if event.paths.is_empty() {
            self.require_rescan(now);
            return true;
        }
        let recursive =
            !matches!(event.kind, EventKind::Modify(k) if !matches!(k, ModifyKind::Name(_)));
        let content = matches!(
            event.kind,
            EventKind::Any | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_))
        );
        self.first.get_or_insert(now);
        self.last = Some(now);
        if !self.rescan {
            for (index, path) in event.paths.into_iter().enumerate() {
                if self.paths.len() >= MAX_PATHS && !self.paths.contains_key(&path) {
                    self.require_rescan(now);
                    break;
                }
                let entry = self.paths.entry(path.clone()).or_insert(Change {
                    path,
                    recursive: false,
                    content: false,
                    renamed_from: false,
                });
                entry.recursive |= recursive;
                entry.content |= content;
                entry.renamed_from |= matches!(
                    event.kind,
                    EventKind::Modify(ModifyKind::Name(RenameMode::From))
                ) || (index == 0
                    && matches!(
                        event.kind,
                        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                    ));
            }
        }
        true
    }

    pub fn require_rescan(&mut self, now: Instant) {
        // Retain already known hints (still capped at MAX_PATHS). A recovery inventory
        // must not forget a notified write that deliberately preserved size/mtime.
        self.rescan = true;
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// No timer exists while idle. A storm cannot postpone work beyond MAX_DELAY.
    #[must_use]
    pub fn delay(&self, now: Instant) -> Option<Duration> {
        if self.rescan {
            return Some((self.first? + RECOVERY_DELAY).saturating_duration_since(now));
        }
        Some(
            (self.last? + QUIET)
                .min(self.first? + MAX_DELAY)
                .saturating_duration_since(now),
        )
    }

    pub fn take(&mut self) -> Batch {
        let old = std::mem::take(self);
        Batch {
            changes: old.paths.into_values().collect(),
            rescan: old.rescan,
        }
    }
}

/// Register before scanning. A missing/offline root is retried at the next full pass;
/// parent watches also notice its removal/recreation. No polling backend is used.
pub struct NativeWatch {
    watcher: RecommendedWatcher,
    watched: BTreeMap<PathBuf, RecursiveMode>,
}

impl NativeWatch {
    /// # Errors
    /// Native notification backend could not start.
    pub fn new(handler: impl notify::EventHandler) -> notify::Result<Self> {
        Ok(Self {
            watcher: RecommendedWatcher::new(
                handler,
                Config::default().with_follow_symlinks(false),
            )?,
            watched: BTreeMap::new(),
        })
    }

    /// Failed roots are returned, not silently treated as watched.
    pub fn set_roots(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let mut wanted = BTreeMap::new();
        for root in roots {
            if let Some(parent) = root.parent().filter(|p| !p.as_os_str().is_empty()) {
                wanted
                    .entry(parent.to_path_buf())
                    .or_insert(RecursiveMode::NonRecursive);
            }
            wanted.insert(root.clone(), RecursiveMode::Recursive);
        }
        let remove: Vec<_> = self
            .watched
            .iter()
            .filter(|(p, mode)| wanted.get(*p) != Some(*mode))
            .map(|(p, _)| p.clone())
            .collect();
        for path in remove {
            let _ = self.watcher.unwatch(&path);
            self.watched.remove(&path);
        }
        let mut failed = Vec::new();
        for (path, mode) in wanted {
            if self.watched.contains_key(&path) {
                continue;
            }
            if self.watcher.watch(&path, mode).is_ok() {
                self.watched.insert(path, mode);
            } else {
                failed.push(path);
            }
        }
        failed
    }
}

/// Marker changes may change exclusion rules for siblings (or code repository context).
#[must_use]
pub fn is_marker(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name == ".git"
        || name == "pyvenv.cfg"
        || crate::PROJECT_MARKERS
            .iter()
            .any(|m| m.to_lowercase() == name)
        || name.ends_with(".csproj")
        || name.starts_with("build.gradle")
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, Flag, RenameMode};

    #[test]
    fn coalesces_writes_and_renames_without_losing_write_invalidation() {
        let now = Instant::now();
        let path = PathBuf::from("note.txt");
        let mut p = Pending::default();
        p.push(
            Ok(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
                    .add_path(path.clone()),
            ),
            now,
        );
        p.push(
            Ok(Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content))).add_path(path)),
            now + QUIET,
        );
        let batch = p.take();
        assert_eq!(batch.changes.len(), 1);
        assert!(batch.changes[0].content && batch.changes[0].recursive);
        assert_eq!(p.delay(now), None);
    }

    #[test]
    fn overflow_and_native_loss_bound_memory_and_request_recovery() {
        let now = Instant::now();
        let mut p = Pending::default();
        for n in 0..=MAX_PATHS {
            p.push(
                Ok(Event::new(EventKind::Create(CreateKind::File))
                    .add_path(PathBuf::from(n.to_string()))),
                now,
            );
        }
        assert_eq!(p.paths.len(), MAX_PATHS);
        let batch = p.take();
        assert!(batch.rescan);
        assert_eq!(batch.changes.len(), MAX_PATHS);
        p.push(Ok(Event::new(EventKind::Any).set_flag(Flag::Rescan)), now);
        assert!(p.take().rescan);
    }

    #[test]
    fn quiet_debounce_has_a_deadline_under_continuous_writes() {
        let now = Instant::now();
        let mut p = Pending::default();
        for n in 0..30 {
            p.push(
                Ok(Event::new(EventKind::Any).add_path(PathBuf::from("f"))),
                now + Duration::from_millis(n * 100),
            );
        }
        assert_eq!(p.delay(now + MAX_DELAY), Some(Duration::ZERO));
    }
}
