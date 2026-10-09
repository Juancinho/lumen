//! Code metadata is refreshed on the background content thread, never during a query.
//! The upgrade backfills existing chunks without recomputing their embeddings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lumen_core::CancellationToken;
use lumen_extract::{DocKind, kind_for_extension};
use lumen_storage::{StorageError, Store};

pub(crate) fn refresh(
    store: &Store,
    scope: &dyn Fn(&str) -> bool,
    cancel: &CancellationToken,
) -> Result<(), StorageError> {
    let mut after = 0;
    let mut cache = HashMap::new();
    loop {
        let candidates = store.code_candidates(after, 32)?;
        let Some(last) = candidates.last() else { break };
        after = last.item_id;
        for c in candidates {
            if cancel.is_cancelled() {
                return Ok(());
            }
            if !scope(&c.path) {
                continue;
            }
            let Some(DocKind::Code(language)) = kind_for_extension(&c.extension) else {
                continue;
            };
            let path = lumen_catalog::path::decode(&c.path, c.raw_path.as_deref());
            let repo = repository(&path, &mut cache);
            store.set_code_context(
                c.item_id,
                &c.path,
                language.as_str(),
                repo.as_deref().and_then(Path::to_str),
            )?;
        }
    }
    Ok(())
}

/// Nearest .git directory or worktree marker file. Never read Git contents, follow
/// marker symlinks or walk the repository. Cache shared ancestors for this pass only.
fn repository(path: &Path, cache: &mut HashMap<PathBuf, Option<PathBuf>>) -> Option<PathBuf> {
    let mut visited = Vec::new();
    let mut found = None;
    let mut resolved = false;
    for parent in path.ancestors().skip(1).take(32) {
        if let Some(repo) = cache.get(parent) {
            found.clone_from(repo);
            resolved = true;
            break;
        }
        visited.push(parent.to_path_buf());
        if std::fs::symlink_metadata(parent.join(".git")).is_ok_and(|m| m.is_dir() || m.is_file()) {
            found = Some(parent.to_path_buf());
            resolved = true;
            break;
        }
        if parent.parent().is_none() {
            resolved = true;
            break;
        }
    }
    // Bound the per-pass cache on large catalogs.
    if cache.len() + visited.len() > 4096 {
        cache.clear();
    }
    if resolved {
        for parent in visited {
            cache.insert(parent, found.clone());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_repository_and_worktree_markers_are_supported() {
        let root = std::env::temp_dir().join(format!("lumen-code-repo-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("nested/src")).unwrap();
        std::fs::write(root.join("nested/.git"), "gitdir: ../worktrees/nested").unwrap();
        let mut cache = HashMap::new();
        assert_eq!(
            repository(&root.join("nested/src/a.py"), &mut cache),
            Some(root.join("nested"))
        );
        assert_eq!(
            repository(&root.join("b.rs"), &mut cache),
            Some(root.clone())
        );
        assert_eq!(
            repository(&root.join("nested/src/b.ts"), &mut cache),
            Some(root.join("nested"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
