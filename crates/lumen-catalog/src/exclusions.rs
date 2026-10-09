//! Explicit user exclusions also apply to retained/offline catalog entries (T112).
use lumen_core::CancellationToken;
use lumen_storage::{ItemKind, StorageError, Store};

use crate::IndexLocations;

/// Removes only explicitly excluded file-source entries before the recovery inventory.
/// Bounded keyset pages/transactions, same writer and cancellation boundary. Files on
/// disk are never modified; unaffected chunks/vectors and application entries stay.
/// # Errors
/// SQLite failure.
pub fn prune_user_exclusions(
    store: &mut Store,
    model: &IndexLocations,
    cancel: &CancellationToken,
) -> Result<u64, StorageError> {
    if model.exclude_paths.is_empty()
        && model.exclude_names.is_empty()
        && model.exclude_extensions.is_empty()
    {
        return Ok(0);
    }
    let mut after = 0;
    let mut removed = 0;
    while !cancel.is_cancelled() {
        let rows = store.file_paths_page(after, 512)?;
        let Some(last) = rows.last() else { break };
        after = last.id;
        let doomed: Vec<_> = rows
            .iter()
            .filter(|r| {
                model.user_excludes(
                    &crate::path::decode(&r.path, r.raw_path.as_deref()),
                    r.kind == ItemKind::Folder,
                )
            })
            .map(|r| r.id)
            .collect();
        if cancel.is_cancelled() {
            break;
        }
        if !doomed.is_empty() {
            removed += store.delete_items(&doomed)?;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_storage::{GenerationSpec, NewChunk, NewItem, VectorWrite};
    use std::path::PathBuf;

    #[test]
    fn offline_cleanup_spans_pages_and_cascades_only_excluded_files() {
        let dir = std::env::temp_dir().join(format!("lumen-t112-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("test.db");
        let mut store = Store::open_writer(&db).unwrap();
        let generation = store
            .ensure_generation(
                GenerationSpec {
                    space_key: "t112-test",
                    chunker_version: 1,
                    dim: 2,
                },
                0,
            )
            .unwrap();
        let mut kept = Vec::new();
        let mut removed = Vec::new();
        for i in 0..1030 {
            let ext = if i % 2 == 0 { "JS" } else { "md" };
            let path = format!("/offline/entry-{i}.{ext}");
            let name = format!("entry-{i}.{ext}");
            let id = store.insert_item(&NewItem::file(&path, &name)).unwrap();
            let chunk = store
                .insert_chunks(&[NewChunk {
                    item_id: id,
                    ordinal: 0,
                    chunk_kind: "text",
                    text: "needle",
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                }])
                .unwrap()[0];
            store
                .write_vectors(
                    generation,
                    &[VectorWrite {
                        chunk_id: chunk,
                        result: Ok(&[0.6, 0.8]),
                    }],
                    0,
                )
                .unwrap();
            store
                .record_use(id, lumen_storage::UseKind::Primary, Some("needle"), 0)
                .unwrap();
            if i % 2 == 0 {
                removed.push((id, chunk));
            } else {
                kept.push((id, chunk));
            }
        }
        let folder = store
            .insert_item(&NewItem {
                kind: ItemKind::Folder,
                ..NewItem::file("/offline/folder.js", "folder.js")
            })
            .unwrap();
        crate::apps::write_apps(
            &mut store,
            &[crate::apps::DiscoveredApp {
                name: "application.js".into(),
                location: "/apps/application.js".into(),
                launch_target: "/apps/application.js".into(),
                source: crate::apps::AppSource::StartMenuShortcut,
            }],
        )
        .unwrap();
        let mut model = IndexLocations::standard(&[PathBuf::from("/offline")], 0);
        model.set_extension_excluded("js", true);
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            prune_user_exclusions(&mut store, &model, &cancelled).unwrap(),
            0
        );
        assert_eq!(store.vector_count(generation).unwrap(), 1030);
        assert_eq!(
            prune_user_exclusions(&mut store, &model, &CancellationToken::new()).unwrap(),
            515
        );
        assert_eq!(store.vector_count(generation).unwrap(), 515);
        for (id, chunk) in removed {
            assert!(store.catalog_item(id).unwrap().is_none());
            assert!(store.chunk_refs(&[chunk], 100).unwrap().is_empty());
        }
        for (id, chunk) in kept {
            assert!(store.catalog_item(id).unwrap().is_some());
            assert_eq!(store.chunk_refs(&[chunk], 100).unwrap().len(), 1);
        }
        assert!(store.catalog_item(folder).unwrap().is_some());
        assert!(
            store
                .item_id_by_path("/apps/application.js")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            prune_user_exclusions(&mut store, &model, &CancellationToken::new()).unwrap(),
            0
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn excluding_one_file_keeps_disk_and_unrelated_identity_and_restores_on_inventory() {
        let dir = std::env::temp_dir().join(format!("lumen-t112-reinclude-{}", std::process::id()));
        let root = dir.join("files");
        std::fs::create_dir_all(&root).unwrap();
        let excluded = root.join("one.json");
        let unrelated = root.join("one.jsonl");
        std::fs::write(&excluded, b"private").unwrap();
        std::fs::write(&unrelated, b"keep").unwrap();
        let mut store = Store::open_writer(&dir.join("test.db")).unwrap();
        let mut model = IndexLocations::standard(&[root], 0);
        crate::sync_files(&mut store, &model.scan_options(false), None).unwrap();
        let kept_id = store.item_id_by_path(&unrelated.to_string_lossy()).unwrap();
        model.exclude_path(&excluded);
        assert_eq!(
            prune_user_exclusions(&mut store, &model, &CancellationToken::new()).unwrap(),
            1
        );
        crate::sync_files(&mut store, &model.scan_options(false), None).unwrap();
        assert!(
            store
                .item_id_by_path(&excluded.to_string_lossy())
                .unwrap()
                .is_none()
        );
        assert_eq!(std::fs::read(&excluded).unwrap(), b"private");
        assert_eq!(
            store.item_id_by_path(&unrelated.to_string_lossy()).unwrap(),
            kept_id
        );
        model.unexclude_path(&excluded.to_string_lossy());
        crate::sync_files(&mut store, &model.scan_options(false), None).unwrap();
        assert!(
            store
                .item_id_by_path(&excluded.to_string_lossy())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store.item_id_by_path(&unrelated.to_string_lossy()).unwrap(),
            kept_id
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
