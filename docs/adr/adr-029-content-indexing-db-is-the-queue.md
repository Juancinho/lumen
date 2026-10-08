# ADR-029 — Content indexing: the database is the queue, vectors live in SQLite, thread cap before duty cycle

**Status:** Accepted (T202, core). Code: `crates/lumen-content` (`run_content_pass`,
`run_queue`, `Control`), `lumen_storage::content`, migration `0002_content_and_vectors.sql`.
Evidence: `docs/benchmarks/t202/2026-10-08-cloud-sandbox-pipeline-*.json`
(`lumen-bench pipeline`, 2 vCPU sandbox, this repository's sources: 128 files → 3,052 chunks).

**Decision**

- **Content pass (Pass 1).** Text files (the `lumen-extract` extension list) whose content
  is new, changed (`content_fingerprint` = `size:mtime` ≠ the catalog's), failed last time or
  produced by an older `EXTRACTOR_VERSION` are extracted, chunked and their chunks replaced in
  one transaction per 32 files. Content state lives in its own columns
  (`items.content_state` indexed/skipped/failed + `content_error`), separate from the
  inventory `status`. Cloud placeholders and items with failed metadata are not read;
  locations catalogued by name only are left alone (scope predicate, ADR-027 `content`).
- **The database is the queue.** A chunk is pending for a generation while it has no
  `chunk_vectors` row there. Paging is keyset by chunk id (one rescan from the start before
  "drained", for reused top ids). Nothing is held in memory but one batch — that is the
  backpressure bound — and a restart resumes exactly (measured: 0 pending after reopening a
  drained queue).
- **Vectors are stored durably in SQLite** (`chunk_vectors`: generation, chunk, 256 × f16
  little-endian = 512 B, ~600 B per vector with row + index pages). Embedding is the
  expensive part of the index (hours of CPU, ADR-015); the ANN file (T203) is derived data
  rebuilt from this table without re-embedding. A chunk is embedded once per generation, so
  a new generation can be built while the old one stays searchable
  (`chunks.embedding_generation` was dropped). Generations are keyed by
  `EmbeddingSpace::key()` × chunker version and created `building`; T203 validates and
  switches them.
- **One indexing thread** runs catalog sync, content pass and queue in turn on the single
  SQLite writer (ADR-025's rule for a third writer): `run_queue` returns after a time slice
  (30 s), on pause, cancellation or when drained.
- **Control:** pause/resume; `hold()` guards for interactive embedding (the queue waits at
  the next batch boundary — batches of 8 keep that wait at one short batch); a duty cycle
  that sleeps `busy × (1 − d)/d` after each batch.
- **Failures:** a device/runtime failure aborts the run with nothing written for that batch
  (the device policy, ADR-019, decides); any other batch failure is retried item by item and
  items that still fail are recorded failed for the generation (code only, never content).
- **Document prompt** `title: <file name> | text: <chunk>` (ADR-015/028); Markdown heading
  paths are not in the prompt yet (T205 measures whether they help).

**Evidence (sandbox, 2 vCPU)**

| run | chunks/s | CPU (of machine) | notes |
|---|---:|---:|---|
| mock backend | 27,700 | — | queue overhead **0.026 ms/chunk** (reads + f16 writes) |
| ORT q4, 2 threads | 4.6 | 97 % | real code/Markdown chunks |
| ORT q4, 2 threads, duty 0.5 | 2.3 | 50 % | duty cycle holds the share exactly |
| ORT q4, 1 thread | 2.8 | 50 % | **~20 % more chunks per CPU than the duty cycle** |

Content pass: 545 files/s, 3.5 MiB/s of text; a re-run with nothing changed takes < 1 ms.

**Consequences**

- The Balanced profile caps CPU with the **embedding session's thread count first** (fewer
  threads scale better per core) and uses the duty cycle only for shares a thread count
  cannot express (e.g. 1 thread on a 2-core machine is already 50 %). Turbo = all physical
  cores, duty 1. Concrete numbers come from T014's Windows sweep.
- Queue overhead is ~10⁻⁴ of embedding time: storage is never the bottleneck.
- 100k chunks ≈ 60 MB of vectors in SQLite on top of the ANN file — accepted for the
  ability to rebuild the ANN and switch generations without re-embedding.
- Open: the shell integration (indexing thread, tray pause/resume, progress), model and
  runtime provisioning in the app (T210), per-location content toggle, prioritisation
  beyond insertion order (docs/SEARCH_AND_INDEXING.md §17).
