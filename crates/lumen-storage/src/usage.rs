//! Local usage signals (T106): frecency, learned query → item choices, pins.
//!
//! Aggregates only (docs/PRIVACY_SECURITY.md §2): no per-event log is kept. Query keys are
//! folded and truncated; [`Store::prune_usage`] and [`Store::clear_usage`] implement
//! retention and "delete history". Pins are separate and never expire.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use crate::catalog::CatalogItem;
use crate::{Result, Store};

/// Frecency half-life: a use two weeks ago counts half as much as one today.
pub const HALF_LIFE_MS: f64 = 14.0 * 86_400_000.0;
/// Longest stored query prefix (characters).
pub const QUERY_KEY_MAX_CHARS: usize = 32;

fn lambda() -> f64 {
    std::f64::consts::LN_2 / HALF_LIFE_MS
}

/// `ln(e^a + e^b)` without overflow.
fn log_add_exp(a: f64, b: f64) -> f64 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    hi + (lo - hi).exp().ln_1p()
}

/// Frecency at `now_ms` from a stored rank key.
#[allow(clippy::cast_precision_loss)]
fn decayed(rank_key: f64, now_ms: i64) -> f64 {
    (rank_key - lambda() * now_ms as f64).exp()
}

/// How much one use counts, by action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    /// Opened a file/folder or launched an app (the primary action).
    Primary,
    /// Revealed in Explorer.
    Reveal,
    /// Copied its path.
    Copy,
}

impl UseKind {
    #[must_use]
    pub const fn weight(self) -> f64 {
        match self {
            Self::Primary => 1.0,
            Self::Reveal => 0.5,
            Self::Copy => 0.3,
        }
    }
}

/// Usage evidence for one item.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UsageSignal {
    /// Decayed use score at query time (1.0 ≈ one primary use just now).
    pub frecency: f64,
    pub uses: u64,
    pub last_used_at: Option<i64>,
    pub pinned: bool,
    /// Times this item was chosen for exactly the current query key.
    pub query_uses: u64,
}

/// Prefixes of `key` (1..=[`QUERY_KEY_MAX_CHARS`] characters) recorded for a choice.
fn query_prefixes(key: &str) -> Vec<&str> {
    key.char_indices()
        .skip(1)
        .map(|(i, _)| i)
        .chain(std::iter::once(key.len()))
        .take(QUERY_KEY_MAX_CHARS)
        .map(|end| key[..end].trim_end())
        .filter(|p| !p.is_empty())
        .fold(Vec::new(), |mut acc: Vec<&str>, p| {
            if acc.last() != Some(&p) {
                acc.push(p);
            }
            acc
        })
}

fn truncate(key: &str) -> &str {
    key.char_indices()
        .nth(QUERY_KEY_MAX_CHARS)
        .map_or(key, |(i, _)| &key[..i])
}

impl Store {
    /// Records that the user acted on `item_id` (optionally after typing `query_key`, the
    /// folded query). One transaction.
    ///
    /// # Errors
    /// SQLite failure (e.g. unknown item).
    #[allow(clippy::cast_precision_loss)]
    pub fn record_use(
        &mut self,
        item_id: i64,
        kind: UseKind,
        query_key: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        let add = kind.weight().ln() + lambda() * now_ms as f64;
        let tx = self.conn.transaction()?;
        let old: Option<f64> = tx
            .prepare_cached("SELECT rank_key FROM usage_stats WHERE item_id = ?1")?
            .query_row([item_id], |r| r.get(0))
            .optional()?;
        let key = old.map_or(add, |k| log_add_exp(k, add));
        tx.prepare_cached(
            "INSERT INTO usage_stats (item_id, uses, last_used_at, rank_key) VALUES (?1, 1, ?2, ?3)
             ON CONFLICT (item_id) DO UPDATE SET uses = uses + 1, last_used_at = ?2, rank_key = ?3",
        )?
        .execute(params![item_id, now_ms, key])?;
        if kind == UseKind::Primary
            && let Some(q) = query_key
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO query_choices (query_key, item_id, uses, last_used_at)
                 VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT (query_key, item_id) DO UPDATE SET uses = uses + 1, last_used_at = ?3",
            )?;
            for prefix in query_prefixes(truncate(q.trim())) {
                stmt.execute(params![prefix, item_id, now_ms])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Usage signals for `ids` with respect to the current folded query key.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn usage_for(
        &self,
        ids: &[i64],
        query_key: &str,
        now_ms: i64,
    ) -> Result<HashMap<i64, UsageSignal>> {
        let mut out: HashMap<i64, UsageSignal> = HashMap::new();
        if ids.is_empty() {
            return Ok(out);
        }
        let mut stats = self.conn.prepare_cached(
            "SELECT uses, last_used_at, rank_key FROM usage_stats WHERE item_id = ?1",
        )?;
        let mut pin = self
            .conn
            .prepare_cached("SELECT 1 FROM pins WHERE item_id = ?1")?;
        let mut choice = self.conn.prepare_cached(
            "SELECT uses FROM query_choices WHERE query_key = ?1 AND item_id = ?2",
        )?;
        let key = truncate(query_key.trim());
        for &id in ids {
            let mut signal = UsageSignal::default();
            if let Some((uses, last, rank)) = stats
                .query_row([id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f64>(2)?,
                    ))
                })
                .optional()?
            {
                signal.uses = u64::try_from(uses).unwrap_or(0);
                signal.last_used_at = Some(last);
                signal.frecency = decayed(rank, now_ms);
            }
            signal.pinned = pin.query_row([id], |_| Ok(())).optional()?.is_some();
            if !key.is_empty()
                && let Some(n) = choice
                    .query_row(params![key, id], |r| r.get::<_, i64>(0))
                    .optional()?
            {
                signal.query_uses = u64::try_from(n).unwrap_or(0);
            }
            if signal != UsageSignal::default() {
                out.insert(id, signal);
            }
        }
        Ok(out)
    }

    /// Items most often chosen for exactly `query_key`, most uses first.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn learned_choices(&self, query_key: &str, limit: usize) -> Result<Vec<i64>> {
        let key = truncate(query_key.trim());
        let mut stmt = self.conn.prepare_cached(
            "SELECT item_id FROM query_choices WHERE query_key = ?1
             ORDER BY uses DESC, last_used_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![key, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| r.get(0),
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Suggestions for an empty query: pinned items first (oldest pin first), then the
    /// highest frecency, at most `limit`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn suggestions(&self, limit: usize) -> Result<Vec<(CatalogItem, bool)>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let cols = crate::catalog::ITEM_COLUMNS;
        let mut out = Vec::new();
        let mut pinned = self.conn.prepare_cached(&format!(
            "SELECT {cols} FROM pins JOIN items ON items.id = pins.item_id
             ORDER BY pins.pinned_at LIMIT ?1"
        ))?;
        for item in pinned.query_map([limit], crate::catalog::item_from_row)? {
            out.push((item?, true));
        }
        let mut used = self.conn.prepare_cached(&format!(
            "SELECT {cols} FROM usage_stats JOIN items ON items.id = usage_stats.item_id
             WHERE usage_stats.item_id NOT IN (SELECT item_id FROM pins)
             ORDER BY usage_stats.rank_key DESC LIMIT ?1"
        ))?;
        for item in used.query_map([limit], crate::catalog::item_from_row)? {
            out.push((item?, false));
        }
        out.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(out)
    }

    /// Pins an item (idempotent).
    ///
    /// # Errors
    /// SQLite failure (e.g. unknown item).
    pub fn pin(&self, item_id: i64, now_ms: i64) -> Result<()> {
        self.conn
            .prepare_cached("INSERT OR IGNORE INTO pins (item_id, pinned_at) VALUES (?1, ?2)")?
            .execute(params![item_id, now_ms])?;
        Ok(())
    }

    /// Removes a pin; `false` if it was not pinned.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn unpin(&self, item_id: i64) -> Result<bool> {
        Ok(self
            .conn
            .prepare_cached("DELETE FROM pins WHERE item_id = ?1")?
            .execute([item_id])?
            > 0)
    }

    /// Retention: forgets usage and learned choices last touched before `cutoff_ms`
    /// (pins stay). Returns rows removed.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn prune_usage(&mut self, cutoff_ms: i64) -> Result<u64> {
        let tx = self.conn.transaction()?;
        let a = tx
            .prepare_cached("DELETE FROM query_choices WHERE last_used_at < ?1")?
            .execute([cutoff_ms])?;
        let b = tx
            .prepare_cached("DELETE FROM usage_stats WHERE last_used_at < ?1")?
            .execute([cutoff_ms])?;
        tx.commit()?;
        Ok((a + b) as u64)
    }

    /// "Delete history": all usage and learned choices (pins stay).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn clear_usage(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM query_choices; DELETE FROM usage_stats;")?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ItemKind;
    use crate::catalog::{CatalogEntry, Source};

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_800_000_000_000;

    struct TempDb(PathBuf);

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store_with(names: &[&str]) -> (TempDb, Store, Vec<i64>) {
        let dir = std::env::temp_dir().join(format!(
            "lumen-usage-{}-{}",
            names.join("-").len(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open_writer(&dir.join("u.db")).unwrap();
        let scan = store.begin_scan(Source::Files).unwrap();
        let paths: Vec<String> = names.iter().map(|n| format!("/r/{n}")).collect();
        let entries: Vec<CatalogEntry<'_>> = names
            .iter()
            .zip(&paths)
            .map(|(n, p)| CatalogEntry {
                kind: ItemKind::File,
                source: Source::Files,
                path: p,
                raw_path: None,
                name: n,
                name_key: n,
                name_parts: n,
                path_parts: "",
                extension: None,
                volume_id: None,
                file_id: None,
                launch_target: None,
                attributes: 0,
                size_bytes: None,
                modified_at: None,
                created_at: None,
                error: None,
            })
            .collect();
        store.upsert_entries(scan, &entries).unwrap();
        let ids = paths
            .iter()
            .map(|p| store.item_id_by_path(p).unwrap().unwrap())
            .collect();
        (TempDb(dir), store, ids)
    }

    #[test]
    fn frecency_decays_with_the_half_life_and_accumulates() {
        let (_t, mut s, ids) = store_with(&["a", "b"]);
        s.record_use(ids[0], UseKind::Primary, None, NOW).unwrap();
        let f = s.usage_for(&ids, "", NOW).unwrap()[&ids[0]].frecency;
        assert!((f - 1.0).abs() < 1e-9);
        let later = s.usage_for(&ids, "", NOW + 14 * DAY).unwrap()[&ids[0]].frecency;
        assert!((later - 0.5).abs() < 1e-9, "{later}");
        s.record_use(ids[0], UseKind::Primary, None, NOW + 14 * DAY)
            .unwrap();
        let sig = s.usage_for(&ids, "", NOW + 14 * DAY).unwrap()[&ids[0]];
        assert!((sig.frecency - 1.5).abs() < 1e-9);
        assert_eq!(sig.uses, 2);
        assert!(!s.usage_for(&ids, "", NOW).unwrap().contains_key(&ids[1]));
    }

    #[test]
    fn learned_choices_cover_every_typed_prefix() {
        let (_t, mut s, ids) = store_with(&["spotify", "spot.txt"]);
        s.record_use(ids[0], UseKind::Primary, Some("spo"), NOW)
            .unwrap();
        s.record_use(ids[0], UseKind::Primary, Some("spo"), NOW)
            .unwrap();
        s.record_use(ids[1], UseKind::Primary, Some("s"), NOW)
            .unwrap();
        assert_eq!(s.learned_choices("s", 5).unwrap(), [ids[0], ids[1]]);
        assert_eq!(s.learned_choices("sp", 5).unwrap(), [ids[0]]);
        assert!(s.learned_choices("spot", 5).unwrap().is_empty());
        assert_eq!(
            s.usage_for(&ids, "spo", NOW).unwrap()[&ids[0]].query_uses,
            2
        );
        // Reveal/copy are not "choices" for the query.
        s.record_use(ids[1], UseKind::Copy, Some("spo"), NOW)
            .unwrap();
        assert_eq!(s.learned_choices("spo", 5).unwrap(), [ids[0]]);
    }

    #[test]
    fn suggestions_pins_first_then_frecency() {
        let (_t, mut s, ids) = store_with(&["a", "b", "c"]);
        s.record_use(ids[1], UseKind::Primary, None, NOW - 30 * DAY)
            .unwrap();
        s.record_use(ids[2], UseKind::Primary, None, NOW).unwrap();
        s.pin(ids[0], NOW).unwrap();
        s.pin(ids[0], NOW).unwrap();
        let names: Vec<_> = s
            .suggestions(10)
            .unwrap()
            .into_iter()
            .map(|(i, pinned)| (i.name, pinned))
            .collect();
        assert_eq!(
            names,
            [("a".into(), true), ("c".into(), false), ("b".into(), false)]
        );
        assert!(s.unpin(ids[0]).unwrap());
        assert!(!s.unpin(ids[0]).unwrap());
        assert_eq!(s.suggestions(1).unwrap().len(), 1);
    }

    #[test]
    fn retention_and_clear_keep_pins() {
        let (_t, mut s, ids) = store_with(&["a", "b"]);
        s.record_use(ids[0], UseKind::Primary, Some("a"), NOW - 100 * DAY)
            .unwrap();
        s.record_use(ids[1], UseKind::Primary, Some("b"), NOW)
            .unwrap();
        s.pin(ids[0], NOW).unwrap();
        assert_eq!(
            s.prune_usage(NOW - 90 * DAY).unwrap(),
            2,
            "a's stats + its choice"
        );
        assert!(s.learned_choices("a", 5).unwrap().is_empty());
        s.clear_usage().unwrap();
        assert!(s.learned_choices("b", 5).unwrap().is_empty());
        let sug = s.suggestions(5).unwrap();
        assert_eq!(sug.len(), 1);
        assert!(sug[0].1, "pin survives clear");
        // Deleting the item removes its usage (cascade).
        s.record_use(ids[1], UseKind::Primary, Some("b"), NOW)
            .unwrap();
        s.delete_items(&[ids[1]]).unwrap();
        assert!(s.learned_choices("b", 5).unwrap().is_empty());
    }

    #[test]
    fn query_keys_are_truncated() {
        let long = "x".repeat(50);
        assert_eq!(truncate(&long).len(), QUERY_KEY_MAX_CHARS);
        assert_eq!(query_prefixes("abc"), ["a", "ab", "abc"]);
        assert_eq!(query_prefixes("ab c"), ["a", "ab", "ab c"]);
        assert!(log_add_exp(1000.0, 1000.0).is_finite());
    }
}
