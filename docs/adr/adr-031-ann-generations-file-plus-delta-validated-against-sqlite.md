# ADR-031 — ANN generations: derived HNSW file + exact delta, every hit validated against SQLite

**Status:** Accepted (T203; Windows latencies pending, recall is hardware-independent).
Code: `lumen_semantic::{SemanticIndex, build_file, validate, cleanup_files}`,
`lumen_storage::generations`, migration `0003_ann_generations.sql`, shell
`indexing::maintain_ann`. Benchmark: `lumen-bench ann-gen`. Evidence:
`docs/benchmarks/t203/2026-10-08-cloud-sandbox-ann-gen-100k.json` (2 vCPU sandbox,
synthetic embedding-like 256-d vectors, ADR-016 geometry).

**Context.** ADR-029 made `chunk_vectors` (SQLite) the durable embedding results; ADR-016
chose USearch HNSW f16 and noted that a memory-mapped index is read-only. Search needs an ANN
structure that survives restarts, follows a queue that keeps writing, never returns a vector
that no longer belongs to its chunk, and can be replaced by a new generation (model or
chunker change) without an hour of broken search (SEARCH_AND_INDEXING.md §16).

**Decision**

- **Write sequence numbers.** Every `chunk_vectors` row gets a per-generation `seq`
  (`generations.next_seq`), assigned in `write_vectors`. An ANN file is built from one
  consistent snapshot, "every row with `seq <= built_through_seq`", recorded in
  `ann_files` (generation, file name, snapshot seq, vector count, lumen-vector config
  fingerprint). Re-embedded rows get a new seq; SQLite may reuse a deleted chunk id, and the
  seq is what tells the old vector from the new one.
- **File + delta.** The file (`<app data>/vectors/gen-<generation>-<seq>.usearch`, written
  to `.tmp`, renamed, then recorded) is opened memory-mapped. Rows after its snapshot are
  loaded into an in-memory delta (≤ 50,000 rows, ~1 KiB each) searched exactly.
- **Validation of every hit.** Candidates from the file (over-fetched 1.5k + 8, retried
  ×4 when too many are stale) and the delta are checked against the canonical rows in one
  lookup by primary key: a file hit is valid only if its row exists with
  `seq <= built_through_seq`, a delta hit only if its seq is still current. Deleted or
  rewritten chunks therefore never surface, without tombstones in the file.
- **Rebuild, not mutate.** The indexing thread rebuilds the file when the delta reaches
  max(2,000, 10 % of the file), when 20 % of the file's rows are gone or rewritten, or when
  the delta overflowed; each build gets a new file name (a mapped file is never
  overwritten — Windows cannot replace it), and unrecorded files are deleted afterwards
  (retried later if still mapped).
- **Degrade, never fail.** A missing, unreadable, mismatching (config fingerprint or
  vector count) file means "rebuild needed"; meanwhile the delta serves the rows it holds
  (all of them for indexes ≤ 50k vectors) and search reports `incomplete` beyond that.
- **Generations.** The first generation of a database is promoted to `active` at once
  (nothing older to keep searchable; partial semantic results beat none). A later
  generation stays `building` until its queue is drained, at most 1 % of its chunks failed
  and 64 sampled stored vectors find themselves first (identical-vector duplicates count);
  then `activate_generation` switches atomically and the retired generation's vectors are
  deleted in 5,000-row transactions.

**Evidence (sandbox, 2 vCPU, 100k × 256-d, k = 10, 300 queries)**

| scenario | file | delta | search p50 / p95 | recall@10 |
|---|---:|---:|---:|---:|
| fresh file | 100,000 | 0 | 0.66 / 1.07 ms | 0.999 |
| + 5,000 new vectors | 100,000 | 5,000 | 1.68 / 2.23 ms | 0.999 |
| + 5 % of chunks re-embedded | 100,000 | 10,000 | 2.30 / 2.78 ms | 0.998 |
| rebuilt | 105,000 | 0 | 0.70 / 1.09 ms | 0.998 |

Write path 39.5k vectors/s into SQLite; build 25 s for 100k (4.0k vectors/s, one thread);
file 63 MiB, database 63 MiB; open (mmap + empty delta) 7 ms; loading a 5,000-row delta
14 ms. Every latency includes the per-hit SQLite validation; the exact delta adds
~0.2 ms per 1,000 rows (1.0 ms for 5,000).

**Consequences**

- Semantic latency stays dominated by query embedding (~30 ms, ADR-015); ANN + validation
  is ≤ 3 ms at 100k even with a 10 % delta.
- A rebuild blocks the indexing thread (and with it catalog passes) for its duration:
  ~25 s per 100k vectors on the sandbox, about once per 10 % growth. Acceptable for now;
  a multi-threaded or off-thread build is the fix if Windows numbers are worse.
- Search must read the generation the query embedder can embed for: with a model change,
  the active (old) generation needs the old model until the new one is validated. T205 wires
  search to the active generation only; T210 decides whether old models are kept during a
  switch.
- 1M vectors (~630 MB file; build extrapolated to ~4 min single-threaded on the sandbox)
  is the next scale check
  (`scripts/t203/run-windows-ann-gen.ps1 -Large`).
