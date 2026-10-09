//! T209 synthetic v3 -> v4 upgrade measurement; no model or personal files.
//! cargo run --release -p lumen-storage --example code_upgrade -- 100000 report.json

#![allow(clippy::print_stdout)] // Standalone benchmark report.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lumen_storage::{MIGRATIONS, Store};
use rusqlite::{Connection, params};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let chunks: i64 = args.first().map_or(Ok(100_000), |n| n.parse())?;
    if !(1..=1_000_000).contains(&chunks) {
        return Err("chunks must be 1..1000000".into());
    }
    let output = args.get(1).map(PathBuf::from);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let dir =
        std::env::temp_dir().join(format!("lumen-code-upgrade-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&dir)?;
    let db = dir.join("lumen.db");
    let mut conn = Connection::open(&db)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    for migration in &MIGRATIONS[..3] {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "INSERT INTO items (kind, canonical_path, display_name, name_parts, path_parts)
            VALUES ('file', '/synthetic/src/retry.py', 'retry.py', 'retry py', 'synthetic src');
            INSERT INTO generations (space_key, chunker_version, dim, scalar, created_at)
            VALUES ('synthetic', 1, 256, 'f16', 0);",
        )?;
        let mut insert = tx.prepare("INSERT INTO chunks (item_id, ordinal, chunk_kind, text, symbol_name)
            VALUES (1, ?1, 'code', 'def retry_request(url): exponential backoff for failed http requests', 'retry_request')")?;
        let mut vector = tx.prepare(
            "INSERT INTO chunk_vectors (chunk_id, generation, vector, embedded_at, seq)
            VALUES (?1, 1, ?2, 0, ?1)",
        )?;
        let mut unit = [0u8; 512];
        unit[1] = 60;
        for ordinal in 0..chunks {
            insert.execute([ordinal])?;
            vector.execute(params![ordinal + 1, unit.as_slice()])?;
        }
        drop(insert);
        drop(vector);
        tx.commit()?;
    }
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    drop(conn);
    let started = Instant::now();
    let store = Store::open_writer(&db)?;
    let migration_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (kept, sequences): (i64, i64) = store.connection().query_row(
        "SELECT count(*), count(DISTINCT seq) FROM chunk_vectors",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!((kept, sequences), (chunks, chunks));
    let started = Instant::now();
    store.set_code_context(1, "/synthetic/src/retry.py", "python", Some("/synthetic"))?;
    let backfill_ms = started.elapsed().as_secs_f64() * 1000.0;
    let hits: i64 = store.connection().query_row(
        "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH 'python backoff'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(hits, chunks);
    store.connection().execute(
        "INSERT INTO chunks_fts (chunks_fts, rank) VALUES ('integrity-check', 1)",
        [],
    )?;
    let report = format!(
        "{{\"schema_version\":1,\"kind\":\"code-upgrade\",\"os\":\"{}\",\"arch\":\"{}\",\"release\":{},\"logical_cpus\":{},\"chunks\":{chunks},\"preserved_vectors\":{kept},\"migration_ms\":{migration_ms},\"context_backfill_ms\":{backfill_ms}}}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        !cfg!(debug_assertions),
        std::thread::available_parallelism()?.get()
    );
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, &report)?;
    }
    println!("{report}");
    drop(store);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
