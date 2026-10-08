//! Records what the user did with a catalog result (T109 → ADR-023 usage store), so the
//! ranking learns: the result id names the item, the action picks the [`UseKind`] weight,
//! and the folded query text becomes the learned-choice key.

use lumen_core::builtin::{COPY_PATH, LAUNCH, OPEN, REVEAL};
use lumen_core::{ActionId, ResultId};
use lumen_storage::{StorageError, Store, UseKind};

use crate::rank::ParsedQuery;

/// The item behind a catalog result id (`item:<id>`).
#[must_use]
pub fn item_id(result: &ResultId) -> Option<i64> {
    result.as_str().strip_prefix("item:")?.parse().ok()
}

/// How an action counts for frecency; `None` for actions that are not a use.
#[must_use]
pub fn use_kind(action: &ActionId) -> Option<UseKind> {
    if *action == OPEN || *action == LAUNCH {
        Some(UseKind::Primary)
    } else if *action == REVEAL {
        Some(UseKind::Reveal)
    } else if *action == COPY_PATH {
        Some(UseKind::Copy)
    } else {
        None
    }
}

/// Records `action` on `result` after the user typed `query_text`. Returns whether anything
/// was recorded (non-catalog results and non-use actions are ignored).
///
/// # Errors
/// SQLite failure.
pub fn record_action(
    store: &mut Store,
    result: &ResultId,
    action: &ActionId,
    query_text: &str,
    now_ms: i64,
) -> Result<bool, StorageError> {
    let (Some(item), Some(kind)) = (item_id(result), use_kind(action)) else {
        return Ok(false);
    };
    let key = ParsedQuery::parse(query_text).map(|q| q.key);
    store.record_use(item, kind, key.as_deref(), now_ms)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lumen_indexer::{Exclusions, ScanOptions};

    use super::*;
    use crate::sync_files;

    #[test]
    fn parses_item_ids_and_maps_actions() {
        assert_eq!(item_id(&ResultId::new("item:42").unwrap()), Some(42));
        assert_eq!(item_id(&ResultId::new("calc:1+1").unwrap()), None);
        assert_eq!(use_kind(&OPEN), Some(UseKind::Primary));
        assert_eq!(use_kind(&COPY_PATH), Some(UseKind::Copy));
    }

    #[test]
    fn a_recorded_open_becomes_a_learned_choice() {
        let dir = std::env::temp_dir().join(format!("lumen-catalog-usage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("root")).unwrap();
        fs::write(dir.join("root/Presupuesto.xlsx"), b"x").unwrap();
        let mut store = Store::open_writer(&dir.join("c.db")).unwrap();
        sync_files(
            &mut store,
            &ScanOptions {
                roots: vec![dir.join("root")],
                exclusions: Exclusions::default(),
                identity: false,
            },
            None,
        )
        .unwrap();
        let id: i64 = store
            .connection()
            .query_row(
                "SELECT id FROM items WHERE display_name = 'Presupuesto.xlsx'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let result = ResultId::new(format!("item:{id}")).unwrap();
        assert!(record_action(&mut store, &result, &OPEN, "Presu", 1_000).unwrap());
        assert!(store.learned_choices("presu", 5).unwrap().contains(&id));
        let other = ResultId::new("calc:2").unwrap();
        assert!(!record_action(&mut store, &other, &OPEN, "x", 1_000).unwrap());
        drop(store);
        let _ = fs::remove_dir_all(&dir);
    }
}
