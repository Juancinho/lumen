//! Index generations and their ANN files (T203, ADR-031, docs/SEARCH_AND_INDEXING.md §16).
//!
//! - A generation is `building` while its vectors are written, `active` when search uses
//!   it (at most one), `retired` once replaced; retired generations are deleted in batches.
//! - The first generation of a database is promoted at once ([`Store::promote_first`]):
//!   there is nothing older to keep searchable, so partial results beat none.
//! - Every result row has a write sequence number (`seq`); an ANN file records the highest
//!   one it contains, so readers can tell file hits that are still current from rows
//!   deleted or rewritten since.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use crate::content::decode_f16;
use crate::{Result, StorageError, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationState {
    Building,
    Active,
    Retired,
}

impl GenerationState {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "building" => Ok(Self::Building),
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            other => Err(StorageError::Corrupt(format!("generation state `{other}`"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationInfo {
    pub id: i64,
    pub space_key: String,
    pub chunker_version: u32,
    pub dim: usize,
    pub state: GenerationState,
    pub created_at: i64,
    pub activated_at: Option<i64>,
    /// Highest write sequence number used so far (0 = none).
    pub max_seq: i64,
}

/// The current ANN file of a generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnFileRecord {
    pub generation: i64,
    /// Relative to the app's vector folder.
    pub file_name: String,
    /// The file holds exactly the vectors with `seq <= built_through_seq` that existed
    /// when it was built.
    pub built_through_seq: i64,
    pub vectors: u64,
    pub index_config: String,
    pub built_at: i64,
}

/// One stored vector with its write sequence number.
#[derive(Debug, Clone, PartialEq)]
pub struct SeqVector {
    pub chunk_id: i64,
    pub seq: i64,
    pub vector: Vec<f32>,
}

const GENERATION_COLUMNS: &str =
    "id, space_key, chunker_version, dim, state, created_at, activated_at, next_seq - 1";

fn generation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(GenerationInfo, String)> {
    let chunker: i64 = r.get(2)?;
    let dim: i64 = r.get(3)?;
    Ok((
        GenerationInfo {
            id: r.get(0)?,
            space_key: r.get(1)?,
            chunker_version: u32::try_from(chunker).unwrap_or(0),
            dim: usize::try_from(dim).unwrap_or(0),
            state: GenerationState::Building,
            created_at: r.get(5)?,
            activated_at: r.get(6)?,
            max_seq: r.get(7)?,
        },
        r.get(4)?,
    ))
}

fn with_state((mut g, state): (GenerationInfo, String)) -> Result<GenerationInfo> {
    g.state = GenerationState::parse(&state)?;
    Ok(g)
}

impl Store {
    /// Every generation, oldest first.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn generations(&self) -> Result<Vec<GenerationInfo>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {GENERATION_COLUMNS} FROM generations ORDER BY id"
        ))?;
        let rows = stmt.query_map([], generation_row)?;
        rows.map(|r| with_state(r?)).collect()
    }

    /// The generation search uses, if any.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn active_generation(&self) -> Result<Option<GenerationInfo>> {
        self.conn
            .prepare_cached(&format!(
                "SELECT {GENERATION_COLUMNS} FROM generations WHERE state = 'active'"
            ))?
            .query_row([], generation_row)
            .optional()?
            .map(with_state)
            .transpose()
    }

    /// Makes `generation` active when no generation is: the first index of a database is
    /// searchable while it fills. Returns whether it is active now.
    ///
    /// # Errors
    /// Unknown generation; SQLite failure.
    pub fn promote_first(&mut self, generation: i64, now_ms: i64) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let active: Option<i64> = tx
            .prepare_cached("SELECT id FROM generations WHERE state = 'active'")?
            .query_row([], |r| r.get(0))
            .optional()?;
        let now_active = match active {
            Some(id) => id == generation,
            None => {
                let n = tx
                    .prepare_cached(
                        "UPDATE generations SET state = 'active', activated_at = ?2
                         WHERE id = ?1 AND state = 'building'",
                    )?
                    .execute(params![generation, now_ms])?;
                if n == 0 {
                    return Err(StorageError::Corrupt(format!(
                        "generation {generation} cannot be promoted"
                    )));
                }
                true
            }
        };
        tx.commit()?;
        Ok(now_active)
    }

    /// Atomically makes `generation` the active one; the previous active generation is
    /// retired (still on disk until [`Store::delete_retired_vectors`]).
    ///
    /// # Errors
    /// Unknown or retired generation; SQLite failure.
    pub fn activate_generation(&mut self, generation: i64, now_ms: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        let state: Option<String> = tx
            .prepare_cached("SELECT state FROM generations WHERE id = ?1")?
            .query_row([generation], |r| r.get(0))
            .optional()?;
        match state.as_deref() {
            Some("active") => return Ok(()),
            Some("building") => {}
            other => {
                return Err(StorageError::Corrupt(format!(
                    "generation {generation} cannot be activated (state {other:?})"
                )));
            }
        }
        tx.prepare_cached("UPDATE generations SET state = 'retired' WHERE state = 'active'")?
            .execute([])?;
        tx.prepare_cached(
            "UPDATE generations SET state = 'active', activated_at = ?2 WHERE id = ?1",
        )?
        .execute(params![generation, now_ms])?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes up to `limit` vector rows of retired generations, then the generations
    /// that have none left (their ANN file rows go with them). Returns rows deleted; call
    /// again until 0 (small transactions keep the writer responsive).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn delete_retired_vectors(&mut self, limit: usize) -> Result<usize> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let tx = self.conn.transaction()?;
        // chunk_vectors is WITHOUT ROWID: delete by primary key.
        let deleted = tx
            .prepare_cached(
                "DELETE FROM chunk_vectors WHERE (generation, chunk_id) IN (
                     SELECT v.generation, v.chunk_id FROM chunk_vectors v
                     JOIN generations g ON g.id = v.generation
                     WHERE g.state = 'retired' LIMIT ?1)",
            )?
            .execute([limit])?;
        if deleted == 0 {
            tx.prepare_cached(
                "DELETE FROM generations WHERE state = 'retired' AND NOT EXISTS
                     (SELECT 1 FROM chunk_vectors v WHERE v.generation = generations.id)",
            )?
            .execute([])?;
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// The generation's current ANN file record.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn ann_file(&self, generation: i64) -> Result<Option<AnnFileRecord>> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT generation, file_name, built_through_seq, vectors, index_config, built_at
                 FROM ann_files WHERE generation = ?1",
            )?
            .query_row([generation], |r| {
                let vectors: i64 = r.get(3)?;
                Ok(AnnFileRecord {
                    generation: r.get(0)?,
                    file_name: r.get(1)?,
                    built_through_seq: r.get(2)?,
                    vectors: u64::try_from(vectors).unwrap_or(0),
                    index_config: r.get(4)?,
                    built_at: r.get(5)?,
                })
            })
            .optional()?)
    }

    /// Every recorded ANN file name (cleanup keeps these, deletes other files).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn ann_file_names(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT file_name FROM ann_files")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Records (replaces) the generation's ANN file. The file must already be in place.
    ///
    /// # Errors
    /// SQLite failure (e.g. the generation was deleted meanwhile).
    pub fn set_ann_file(&self, record: &AnnFileRecord) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR REPLACE INTO ann_files
                     (generation, file_name, built_through_seq, vectors, index_config, built_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                record.generation,
                record.file_name,
                record.built_through_seq,
                i64::try_from(record.vectors).unwrap_or(i64::MAX),
                record.index_config,
                record.built_at
            ])?;
        Ok(())
    }

    /// Forgets the generation's ANN file (corrupt or unreadable: it will be rebuilt).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn clear_ann_file(&self, generation: i64) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM ann_files WHERE generation = ?1")?
            .execute([generation])?;
        Ok(())
    }

    /// Up to `limit` vectors with `chunk_id > after_chunk_id` and `seq <= through_seq`, by
    /// chunk id: one consistent snapshot for an ANN build while the queue keeps writing.
    ///
    /// # Errors
    /// SQLite failure or a malformed blob.
    pub fn vectors_through(
        &self,
        generation: i64,
        after_chunk_id: i64,
        through_seq: i64,
        limit: usize,
    ) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT chunk_id, vector FROM chunk_vectors
             WHERE generation = ?1 AND chunk_id > ?2 AND seq <= ?3 AND vector IS NOT NULL
             ORDER BY chunk_id LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                generation,
                after_chunk_id,
                through_seq,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            out.push((id, decode(id, &blob)?));
        }
        Ok(out)
    }

    /// Up to `limit` vectors written after `after_seq`, in write order: the in-memory
    /// delta on top of an ANN file.
    ///
    /// # Errors
    /// SQLite failure or a malformed blob.
    pub fn vectors_after_seq(
        &self,
        generation: i64,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<SeqVector>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT chunk_id, seq, vector FROM chunk_vectors
             WHERE generation = ?1 AND seq > ?2 AND vector IS NOT NULL
             ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![
                generation,
                after_seq,
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (chunk_id, seq, blob) = row?;
            out.push(SeqVector {
                chunk_id,
                seq,
                vector: decode(chunk_id, &blob)?,
            });
        }
        Ok(out)
    }

    /// Current `seq` of the given chunks' vectors (absent: deleted or failed). Validates
    /// ANN candidates against the canonical rows.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn vector_seqs(&self, generation: i64, chunk_ids: &[i64]) -> Result<HashMap<i64, i64>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT seq FROM chunk_vectors
             WHERE generation = ?1 AND chunk_id = ?2 AND vector IS NOT NULL",
        )?;
        let mut out = HashMap::with_capacity(chunk_ids.len());
        for &id in chunk_ids {
            if let Some(seq) = stmt
                .query_row(params![generation, id], |r| r.get::<_, i64>(0))
                .optional()?
            {
                out.insert(id, seq);
            }
        }
        Ok(out)
    }

    /// Stored vectors with `seq <= through_seq`: what of an ANN file is still current (the
    /// rest of the file are deleted or rewritten rows: compaction signal).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn vector_count_through(&self, generation: i64, through_seq: i64) -> Result<u64> {
        let n: i64 = self
            .conn
            .prepare_cached(
                "SELECT count(*) FROM chunk_vectors
                 WHERE generation = ?1 AND seq <= ?2 AND vector IS NOT NULL",
            )?
            .query_row(params![generation, through_seq], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Stored vectors (not failures) of `generation`.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn vector_count(&self, generation: i64) -> Result<u64> {
        let n: i64 = self
            .conn
            .prepare_cached(
                "SELECT count(*) FROM chunk_vectors WHERE generation = ?1 AND vector IS NOT NULL",
            )?
            .query_row([generation], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

fn decode(chunk_id: i64, blob: &[u8]) -> Result<Vec<f32>> {
    decode_f16(blob)
        .ok_or_else(|| StorageError::Corrupt(format!("vector blob of chunk {chunk_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{GenerationSpec, VectorWrite};
    use crate::tests::TempDb;
    use crate::{NewChunk, NewItem};

    fn chunk(item_id: i64, ordinal: i64) -> NewChunk<'static> {
        NewChunk {
            item_id,
            ordinal,
            chunk_kind: "text",
            text: "texto",
            symbol_name: None,
            page_number: None,
            start_offset: None,
            end_offset: None,
        }
    }

    fn spec(key: &str) -> GenerationSpec<'_> {
        GenerationSpec {
            space_key: key,
            chunker_version: 1,
            dim: 2,
        }
    }

    #[test]
    fn generations_promote_activate_and_retire() {
        let db = TempDb::new("gen-lifecycle");
        let mut store = Store::open_writer(&db.path()).unwrap();
        let a = store.ensure_generation(spec("a"), 1).unwrap();
        assert!(store.active_generation().unwrap().is_none());
        assert!(store.promote_first(a, 2).unwrap());
        let b = store.ensure_generation(spec("b"), 3).unwrap();
        // A second generation does not take over by itself.
        assert!(!store.promote_first(b, 4).unwrap());
        assert_eq!(store.active_generation().unwrap().unwrap().id, a);

        let item = store.insert_item(&NewItem::file("/x", "x")).unwrap();
        let ids = store
            .insert_chunks(&[chunk(item, 0), chunk(item, 1)])
            .unwrap();
        let v = [0.6_f32, 0.8];
        let writes: Vec<_> = ids
            .iter()
            .map(|&chunk_id| VectorWrite {
                chunk_id,
                result: Ok(&v[..]),
            })
            .collect();
        store.write_vectors(a, &writes, 5).unwrap();
        store.write_vectors(b, &writes, 5).unwrap();

        store.activate_generation(b, 6).unwrap();
        let gens = store.generations().unwrap();
        assert_eq!(
            gens.iter().map(|g| g.state).collect::<Vec<_>>(),
            [GenerationState::Retired, GenerationState::Active]
        );
        assert_eq!(gens[1].activated_at, Some(6));
        assert!(
            store.activate_generation(a, 7).is_err(),
            "retired stays retired"
        );

        assert_eq!(store.delete_retired_vectors(1).unwrap(), 1);
        assert_eq!(store.delete_retired_vectors(10).unwrap(), 1);
        assert_eq!(store.delete_retired_vectors(10).unwrap(), 0);
        assert_eq!(store.generations().unwrap().len(), 1);
        assert_eq!(store.vector_count(b).unwrap(), 2);
    }

    #[test]
    fn sequence_numbers_separate_file_contents_from_newer_rows() {
        let db = TempDb::new("gen-seq");
        let mut store = Store::open_writer(&db.path()).unwrap();
        let g = store.ensure_generation(spec("a"), 1).unwrap();
        let item = store.insert_item(&NewItem::file("/x", "x")).unwrap();
        let ids = store
            .insert_chunks(&[chunk(item, 0), chunk(item, 1), chunk(item, 2)])
            .unwrap();
        let v = [1.0_f32, 0.0];
        let w = |chunk_id| VectorWrite {
            chunk_id,
            result: Ok(&v[..]),
        };
        store.write_vectors(g, &[w(ids[0]), w(ids[1])], 1).unwrap();
        store
            .write_vectors(
                g,
                &[VectorWrite {
                    chunk_id: ids[2],
                    result: Err("too_long"),
                }],
                1,
            )
            .unwrap();
        let snapshot = store.generations().unwrap()[0].max_seq;
        assert_eq!(snapshot, 3);
        // Re-embedding a chunk after the snapshot moves it past the file.
        store.write_vectors(g, &[w(ids[0])], 2).unwrap();
        let through = store.vectors_through(g, 0, snapshot, 10).unwrap();
        assert_eq!(through.iter().map(|r| r.0).collect::<Vec<_>>(), [ids[1]]);
        let delta = store.vectors_after_seq(g, snapshot, 10).unwrap();
        assert_eq!(
            (delta.len(), delta[0].chunk_id, delta[0].seq),
            (1, ids[0], 4)
        );
        let seqs = store.vector_seqs(g, &ids).unwrap();
        assert_eq!(seqs.get(&ids[0]), Some(&4));
        assert_eq!(seqs.get(&ids[1]), Some(&2));
        assert!(!seqs.contains_key(&ids[2]), "failures carry no vector");
        assert_eq!(store.vector_count_through(g, snapshot).unwrap(), 1);
        let refs = store.chunk_refs(&[ids[1], 9_999], 3).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!((refs[0].item_id, refs[0].excerpt.as_str()), (item, "tex"));
        assert_eq!(store.vector_count(g).unwrap(), 2);

        let record = AnnFileRecord {
            generation: g,
            file_name: "gen-1-3.usearch".into(),
            built_through_seq: snapshot,
            vectors: 1,
            index_config: "f16".into(),
            built_at: 9,
        };
        store.set_ann_file(&record).unwrap();
        assert_eq!(store.ann_file(g).unwrap(), Some(record));
        assert_eq!(store.ann_file_names().unwrap(), ["gen-1-3.usearch"]);
        store.clear_ann_file(g).unwrap();
        assert!(store.ann_file(g).unwrap().is_none());
    }
}
