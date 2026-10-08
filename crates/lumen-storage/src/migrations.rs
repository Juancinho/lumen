//! Versioned, forward-only schema migrations tracked in `PRAGMA user_version`.
//!
//! Rules: migrations are append-only (never edit a released one), each runs in its own
//! transaction together with the version bump, and a database newer than this binary is
//! refused instead of being touched.

use rusqlite::Connection;

use crate::StorageError;

/// One schema step.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// All migrations, in order. Versions start at 1 and increase by exactly 1.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "content_and_vectors",
        sql: include_str!("../migrations/0002_content_and_vectors.sql"),
    },
];

/// Schema version this binary produces.
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

pub(crate) fn current_version(conn: &Connection) -> Result<u32, StorageError> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    u32::try_from(v).map_err(|_| StorageError::Corrupt(format!("user_version {v}")))
}

/// Applies pending `migrations`. Returns `(from, to)` versions.
pub(crate) fn apply(
    conn: &mut Connection,
    migrations: &[Migration],
) -> Result<(u32, u32), StorageError> {
    let from = current_version(conn)?;
    let latest = migrations.last().map_or(0, |m| m.version);
    if from > latest {
        return Err(StorageError::SchemaTooNew {
            found: from,
            supported: latest,
        });
    }
    for m in migrations.iter().filter(|m| m.version > from) {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)
            .map_err(|e| StorageError::Migration {
                version: m.version,
                name: m.name,
                message: e.to_string(),
            })?;
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
    }
    Ok((from, latest.max(from)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_contiguous_from_one() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                m.version as usize,
                i + 1,
                "migration {} out of order",
                m.name
            );
            assert!(!m.sql.trim().is_empty());
        }
    }

    #[test]
    fn fresh_database_reaches_latest_and_rerun_is_noop() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(apply(&mut conn, MIGRATIONS).unwrap(), (0, latest_version()));
        assert_eq!(current_version(&conn).unwrap(), latest_version());
        assert_eq!(
            apply(&mut conn, MIGRATIONS).unwrap(),
            (latest_version(), latest_version())
        );
    }

    #[test]
    fn failing_migration_rolls_back_completely() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = [
            Migration {
                version: 1,
                name: "ok",
                sql: "CREATE TABLE a (x INTEGER);",
            },
            Migration {
                version: 2,
                name: "broken",
                sql: "CREATE TABLE b (y INTEGER); THIS IS NOT SQL;",
            },
        ];
        let err = apply(&mut conn, &broken).unwrap_err();
        assert!(
            matches!(err, StorageError::Migration { version: 2, .. }),
            "{err}"
        );
        assert_eq!(current_version(&conn).unwrap(), 1);
        let b_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b_exists, 0, "partial migration leaked");
    }

    #[test]
    fn newer_schema_is_refused() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", latest_version() + 1)
            .unwrap();
        assert!(matches!(
            apply(&mut conn, MIGRATIONS),
            Err(StorageError::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn v1_database_with_chunks_upgrades_to_v2() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply(&mut conn, &MIGRATIONS[..1]).unwrap();
        conn.execute_batch(
            "INSERT INTO items (kind, canonical_path, display_name) VALUES ('file', '/a', 'a');
             INSERT INTO chunks (item_id, ordinal, chunk_kind, text, embedding_generation)
             VALUES (1, 0, 'text', 'hola mundo', NULL);",
        )
        .unwrap();
        assert_eq!(apply(&mut conn, MIGRATIONS).unwrap(), (1, 2));
        let (text, state): (String, Option<String>) = conn
            .query_row(
                "SELECT c.text, i.content_state FROM chunks c JOIN items i ON i.id = c.item_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((text.as_str(), state), ("hola mundo", None));
        let fts: i64 = conn
            .query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH 'mundo'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts, 1);
    }
}
