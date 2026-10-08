# ADR-016 — ANN: USearch HNSW, f16 storage, cosine, M=16, ef_search=256


**Status:** Accepted (T008), amends ADR-004. Evidence: `docs/benchmarks/t008/2026-10-08-cloud-sandbox/`
(2 vCPU sandbox; recall is hardware-independent, absolute latencies are pessimistic).
Data: synthetic 256d vectors calibrated on real EmbeddingGemma 2 geometry (mean pairwise cosine
0.58, strong shared direction), 500/300 queries, exact brute-force ground truth.

**Decision**

- **Storage `f16`** (crate `lumen-vector`). Recall equals f32 (100k: R@10 1.000 at ef 64 for
  both; 1M: 0.988 vs 0.990 at ef 256) at ~55% of the memory/disk (100k: 63 MB file / 81 MiB
  in RAM; 1M: 630 MB / 760 MiB) and faster build and search.
- **Rejected:** `i8` — recall plateaus at 0.85 even at ef 256 (the strong shared component of
  real embeddings leaves little resolution for 8-bit cosine); `bf16` — plateaus at 0.995 at
  100k. i8 is only reconsidered with f16 re-scoring of candidates.
- **Metric cosine, M = 16, ef_construction = 128, ef_search = 256.** ef 64 is enough at 100k
  but recall falls to 0.90 at 1M; ef 256 keeps ≥ 0.99 for ~1.3 ms p50 at 1M f16 on the
  sandbox — negligible next to query embedding (~30 ms, ADR-015).
- **Deletes** are tombstones: removed keys were never returned (10,000 removed at 1M,
  ~0.6 µs/key) and can be re-added; space is reclaimed by rebuild/compaction in T203.
- **Read path memory-maps** (`VectorIndex::view`: 5 ms at 100k, ~90 ms at 1M) so a large
  index does not count fully against the resident budget; a viewed index is read-only, so T203
  needs a mutable in-memory delta + periodic merge/rebuild into a new generation file.

**Consequences**

- ANN is not a latency risk: ≤ 2 ms at 1M even with high ef.
- Memory: ~0.8 KB/vector in RAM for f16 (vectors + graph). 100k chunks ≈ 80 MiB fits the idle
  budget; 1M chunks (~760 MiB) must rely on mmap paging, not full load.
- Build speed (2.5–5.5k vectors/s on 2 vCPU) is irrelevant next to embedding (~3–4 chunks/s).
- Uniform random vectors give meaningless recall (no neighbour structure); kept only as a
  documented degenerate case.
- Evidence gaps: real-embedding recall at scale (`lumen-bench ann --vectors` +
  `scripts/embedding/embed_corpus.py` exist for it), Windows latencies
  (`scripts/t008/run-windows-ann.ps1`), filtered search (T208).
- **Windows/MSVC build:** usearch 2.26.4 + numkong 7.8.5 fail to link (`__imp_nk_*`,
  LNK2019: numkong headers declare `dllimport` but the crate builds a static library).
  Workaround in `.cargo/config.toml` `[env]`: `CXXFLAGS_<msvc target> = "/DNK_DYNAMIC="`.
  Found during T009 (T008 had only been built on Linux); remove when fixed upstream.
