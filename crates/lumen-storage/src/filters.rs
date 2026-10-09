//! Parameterized metadata predicates shared by all retrieval lanes (T208).

use std::collections::HashSet;

use lumen_core::{QueryFilter, QueryFilters, QueryType};
use rusqlite::{OptionalExtension, params_from_iter, types::Value};

use crate::catalog::{ITEM_COLUMNS, item_from_row};
use crate::{CatalogItem, ChunkHit, FtsQuery, NameHit, Result, SearchBudget, Store};

/// Append predicates after the caller's parameters. User text is bound, never SQL.
pub(crate) fn predicates(filters: &QueryFilters, args: &mut Vec<Value>) -> String {
    let mut sql = String::new();
    for filter in &filters.0 {
        let param = args.len() + 1;
        match filter {
            QueryFilter::Type(kind) => {
                let value = match kind {
                    QueryType::File => "file",
                    QueryType::Folder => "folder",
                    QueryType::Application => "application",
                    _ => "file",
                };
                sql.push_str(&format!(" AND items.kind = ?{param}"));
                args.push(value.to_owned().into());
                if !kind.extensions().is_empty() {
                    let placeholders: Vec<_> = kind
                        .extensions()
                        .iter()
                        .map(|ext| {
                            args.push((*ext).to_owned().into());
                            format!("?{}", args.len())
                        })
                        .collect();
                    sql.push_str(&format!(
                        " AND lower(items.extension) IN ({})",
                        placeholders.join(",")
                    ));
                }
            }
            QueryFilter::Extension(ext) => {
                sql.push_str(&format!(
                    " AND items.kind = 'file' AND lower(items.extension) = ?{param}"
                ));
                args.push(ext.clone().into());
            }
            QueryFilter::In(path) => {
                let column = "lower(replace(items.canonical_path, char(92), '/'))";
                // Boundaries make docs different from docs-old, and C:/work from
                // C:/worker. Relative paths name directory components anywhere.
                if path == "/" {
                    sql.push_str(&format!(" AND substr({column}, 1, 1) = '/'"));
                } else if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
                    sql.push_str(&format!(
                        " AND ((items.kind = 'folder' AND {column} = ?{param}) OR instr({column}, ?{param} || '/') = 1)"
                    ));
                    args.push(path.clone().into());
                } else {
                    sql.push_str(&format!(
                        " AND instr('/' || {column} || CASE WHEN items.kind = 'folder' THEN '/' ELSE '' END, '/' || ?{param} || '/') > 0"
                    ));
                    args.push(path.clone().into());
                }
            }
            QueryFilter::Before(ms) | QueryFilter::After(ms) => {
                let op = if matches!(filter, QueryFilter::Before(_)) {
                    "<"
                } else {
                    ">="
                };
                sql.push_str(&format!(" AND items.modified_at {op} ?{param}"));
                args.push((*ms).into());
            }
        }
    }
    sql
}

impl Store {
    /// Filter before LIMIT so excluded leading candidates cannot hide valid ones.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn search_names_filtered(
        &self,
        key: &str,
        limit: usize,
        filters: &QueryFilters,
        budget: &SearchBudget,
    ) -> Result<Vec<NameHit>> {
        if filters.0.is_empty() {
            return self.search_names(key, limit, budget);
        }
        if key.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        self.bounded(budget, |store| {
            let mut args = vec![Value::from(key.to_owned()), Value::from(format!("{key}\u{10FFFF}")), Value::from(i64::try_from(limit).unwrap_or(i64::MAX))];
            let clause = predicates(filters, &mut args);
            let mut hits = Vec::new();
            for (test, exact) in [("name_key = ?1", true), ("name_key > ?1 AND name_key < ?2", false)] {
                let mut stmt = store.conn.prepare_cached(&format!("SELECT {ITEM_COLUMNS} FROM items WHERE {test}{clause} ORDER BY name_key LIMIT ?3"))?;
                for item in stmt.query_map(params_from_iter(&args), item_from_row)? { hits.push(NameHit { item: item?, exact }); }
            }
            Ok(hits)
        })
    }

    /// Filtered token search; the existing unfiltered query keeps its fast path.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn search_name_tokens_filtered(
        &self,
        matcher: &str,
        limit: usize,
        filters: &QueryFilters,
        budget: &SearchBudget,
    ) -> Result<Vec<CatalogItem>> {
        if filters.0.is_empty() {
            return self.search_name_tokens(matcher, limit, budget);
        }
        if matcher.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        self.bounded(budget, |store| {
            let mut args = vec![matcher.to_owned().into(), i64::try_from(limit).unwrap_or(i64::MAX).into()];
            let clause = predicates(filters, &mut args);
            let mut stmt = store.conn.prepare_cached(&format!("SELECT {ITEM_COLUMNS} FROM names_fts JOIN items ON items.id = names_fts.rowid WHERE names_fts MATCH ?1{clause} ORDER BY bm25(names_fts, 1.0, 0.25) LIMIT ?2"))?;
            Ok(stmt.query_map(params_from_iter(&args), item_from_row)?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Filtered typo candidates.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn name_key_range_filtered(
        &self,
        lo: &str,
        hi: &str,
        limit: usize,
        filters: &QueryFilters,
        budget: &SearchBudget,
    ) -> Result<Vec<CatalogItem>> {
        if filters.0.is_empty() {
            return self.name_key_range(lo, hi, limit, budget);
        }
        self.bounded(budget, |store| {
            let mut args = vec![lo.to_owned().into(), hi.to_owned().into(), i64::try_from(limit).unwrap_or(i64::MAX).into()];
            let clause = predicates(filters, &mut args);
            let mut stmt = store.conn.prepare_cached(&format!("SELECT {ITEM_COLUMNS} FROM items WHERE name_key >= ?1 AND name_key < ?2{clause} ORDER BY name_key LIMIT ?3"))?;
            Ok(stmt.query_map(params_from_iter(&args), item_from_row)?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Metadata-only query, ordered by name and stable ID, with a bounded scan.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn filtered_items(
        &self,
        filters: &QueryFilters,
        limit: usize,
        budget: &SearchBudget,
    ) -> Result<Vec<CatalogItem>> {
        self.bounded(budget, |store| {
            let mut args = vec![i64::try_from(limit).unwrap_or(i64::MAX).into()];
            let clause = predicates(filters, &mut args);
            let mut stmt = store.conn.prepare_cached(&format!("SELECT {ITEM_COLUMNS} FROM items WHERE 1{clause} ORDER BY name_key, items.id LIMIT ?1"))?;
            Ok(stmt.query_map(params_from_iter(&args), item_from_row)?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Learned choices obey the same hard constraints as indexed candidates.
    /// # Errors
    /// SQLite failure.
    pub fn catalog_item_filtered(
        &self,
        id: i64,
        filters: &QueryFilters,
    ) -> Result<Option<CatalogItem>> {
        if filters.0.is_empty() {
            return self.catalog_item(id);
        }
        let mut args = vec![id.into()];
        let clause = predicates(filters, &mut args);
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {ITEM_COLUMNS} FROM items WHERE items.id = ?1{clause}"
            ))?
            .query_row(params_from_iter(&args), item_from_row)
            .optional()?)
    }

    /// FTS metadata constraints apply before ranking and LIMIT.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn search_chunks_filtered(
        &self,
        query: &FtsQuery,
        limit: usize,
        filters: &QueryFilters,
        budget: &SearchBudget,
    ) -> Result<Vec<ChunkHit>> {
        if filters.0.is_empty() {
            return self.search_chunks(query, limit, budget);
        }
        self.bounded(budget, |store| {
            let mut args = vec![query.as_str().to_owned().into(), i64::try_from(limit).unwrap_or(i64::MAX).into()];
            let clause = predicates(filters, &mut args);
            let mut stmt = store.conn.prepare_cached(&format!("SELECT c.id, c.item_id, bm25(chunks_fts) AS rank, snippet(chunks_fts, 0, '{}', '{}', '…', 16) FROM chunks_fts JOIN chunks c ON c.id = chunks_fts.rowid JOIN items ON items.id = c.item_id WHERE chunks_fts MATCH ?1{clause} ORDER BY rank LIMIT ?2", crate::HIGHLIGHT_START, crate::HIGHLIGHT_END))?;
            Ok(stmt.query_map(params_from_iter(&args), |r| Ok(ChunkHit { chunk_id: r.get(0)?, item_id: r.get(1)?, rank: r.get(2)?, snippet: r.get(3)? }))?.collect::<std::result::Result<_, _>>()?)
        })
    }

    /// Check an ANN candidate batch against current canonical metadata.
    /// # Errors
    /// SQLite failure or exhausted/cancelled budget.
    pub fn matching_chunk_ids(
        &self,
        ids: &[i64],
        filters: &QueryFilters,
        budget: &SearchBudget,
    ) -> Result<HashSet<i64>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        self.bounded(budget, |store| {
            let mut out = HashSet::new();
            for batch in ids.chunks(500) {
                let mut args: Vec<Value> = batch.iter().copied().map(Value::from).collect();
                let placeholders: Vec<_> = (1..=args.len()).map(|i| format!("?{i}")).collect();
                let clause = predicates(filters, &mut args);
                let mut stmt = store.conn.prepare_cached(&format!("SELECT c.id FROM chunks c JOIN items ON items.id = c.item_id WHERE c.id IN ({}){clause}", placeholders.join(",")))?;
                out.extend(stmt.query_map(params_from_iter(&args), |r| r.get::<_, i64>(0))?.collect::<std::result::Result<Vec<_>, _>>()?);
            }
            Ok(out)
        })
    }
}
