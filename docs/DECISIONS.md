# DECISIONS.md — architecture decision log

> ADRs may be amended with evidence. Do not silently contradict them.

## ADR-001 — Local-first core

**Status:** Accepted

Core search/indexing/actions work without cloud services. Optional remote integrations are explicit later.

## ADR-002 — Rust core, Tauri/React shell

**Status:** Accepted provisionally

Use Rust for core/search/indexing and Tauri 2 + React/TypeScript for current UI shell.

Critical boundary: domain/core crates MUST NOT depend on Tauri/React. Shell migration must be possible without rewriting search/indexing.

FastFrame/egui remains an evidence-triggered comparison spike (`TX01`), not a current dependency.

## ADR-003 — SQLite + FTS5 canonical metadata store

**Status:** Accepted

SQLite is canonical for metadata/chunks/settings/history. ANN index is rebuildable derived data.

## ADR-004 — USearch/HNSW candidate ANN

**Status:** Accepted for benchmark path

Final parameters/scalar storage require T008 evidence.

## ADR-005 — Embedding runtime abstraction

**Status:** Accepted

EmbeddingGemma is accessed through `EmbeddingBackend`; no production Python requirement.

## ADR-006 — 256d default semantic index target

**Status:** Accepted provisionally

Use 256d normalized embeddings unless relevance testing justifies another profile.

## ADR-007 — Universal provider/result/action domain model

**Status:** Accepted

Lumen is a command center. Search sources return typed `ResultItem`s; results expose contextual actions. Workflows compose actions later.

This does NOT authorize building a public plugin SDK in M0–M3.

## ADR-008 — Single root search

**Status:** Accepted

Files/apps/commands/productivity features share one root surface. Modes may exist as explicit filters/prefixes but ordinary use must not require mode switching.

## ADR-009 — Progressive retrieval

**Status:** Accepted

Immediate lexical/provider results appear before semantic inference completes. Semantic results refine ranking with selection stability.

## ADR-010 — One-WebView target

**Status:** Accepted provisionally

Search, preview, settings/onboarding should share one WebView where feasible. Hidden state must minimize timers/render work. Split only with measured justification.

## ADR-011 — Multi-pass resumable indexing

**Status:** Accepted

Initial indexing is prioritized and resumable: metadata first, high-value text/code next, images then heavy media/refinement. Interactive work preempts background jobs.

## ADR-012 — Rewind is opt-in metadata/event memory first

**Status:** Accepted

Do not make continuous screen recording a default requirement. Rewind begins from local events/activity semantics with retention controls.

## ADR-013 — Wire DTOs are shell-owned projections; core types carry no serialization

**Status:** Accepted (T011)

Domain types in `crates/` (`ResultItem`, `ActionDescriptor`, ids, …) do not derive serde or any
wire format. The shell maps them into explicit camelCase DTOs (`apps/desktop/src-tauri/src/dto*`),
mirrored by hand-written TypeScript types in `apps/desktop/src/ipc/types.ts`, each guarded by a
Rust JSON-shape test.

Reasons:

- the wire contract is a *projection*, not the domain model: `Payload` (paths, provider keys) and
  provider confidence must never reach the UI; the UI refers to results only by `ResultId` and so
  cannot ask Lumen to act on arbitrary paths;
- keeps `lumen-core` dependency-free and shell-agnostic (ADR-002); another shell may need another
  encoding;
- explicit DTOs make breaking wire changes visible in review.

Revisit (generated TS bindings such as ts-rs/specta) when the DTO set grows beyond roughly ten
types or hand-mirroring causes a real defect. Generation must still run on shell DTOs, not core
types.

## ADR-014 — Synchronous embedding backend trait; shared correctness layer in `Embedder`

**Status:** Accepted (T005)

`EmbeddingBackend` (crate `lumen-embedding`) is a synchronous `Send + Sync` trait. Backends only
map fully formatted strings to raw native-dimension vectors (row-major `Vec<f32>`). Everything
that must be identical across runtimes lives in `Embedder`: versioned task prompts
(`PromptFormat`), batching to `max_batch`, cancellation between batches, output validation
(shape, NaN/inf, zero norm), Matryoshka truncation to the profile dimension with L2
renormalization (f64 accumulation), and the `EmbeddingSpace` key stored with index generations.

Reasons:

- inference is CPU/accelerator-bound; an async trait would add boxing/runtime coupling without
  concurrency gains, and most candidate runtimes expose blocking APIs;
- query-over-index prioritization (docs/ARCHITECTURE.md §13) needs dedicated worker lanes, which
  the query service (T204) and index queue (T202) own; the trait stays scheduling-free;
- one implementation of prompts/truncation/normalization prevents runtimes from silently
  producing incompatible spaces.

`EmbeddingSpace` = model id + weights revision (incl. quantization) + preprocessing version +
prompt id/version + dimension + normalization. It excludes backend and execution target (same
weights on CPU/NPU are one space). Prompt strings are the EmbeddingGemma v1 retrieval prompts and
MUST be verified against the EmbeddingGemma 2 model card in T006 (new `PromptFormat` version if
they differ). Image/audio/video entry points are added later as provided trait methods.

## ADR-015 — EmbeddingGemma 2 runs on ONNX Runtime, CPU, q4 weights by default

**Status:** Accepted (T006). Evidence: `docs/benchmarks/t006/2026-10-07-joao-pc/` (Ryzen 5 5600H
6C/12T AVX2, 15 GB, GTX 1650 4 GB + Radeon Vega iGPU, Windows 11, AC power).

**Decision**

1. **Runtime:** ONNX Runtime, loaded dynamically from `onnxruntime.dll` shipped next to `lumen.exe`
   (`ort` 2.0.0-rc.13, API level 24, crate `lumen-embedding-ort`). Model: text graph of
   `onnx-community/embeddinggemma-2-ONNX`, tokenizer from its `tokenizer.json`. Rust output
   matches the Python reference exactly (fp32 min cosine 1.00000).
2. **Default device: CPU. Default weights: `model_q4` (174 MB).** Short query p50/p95
   30.0/36.9 ms, ~128-token input 133 ms, 168 MiB resident, min cosine 0.980 vs fp32 with
   identical top-1 on the fidelity corpus. Index space key
   `embeddinggemma-2@onnx-community-q4/pre1/embeddinggemma-retrieval@1/d256/l2`.
   - `fp32` stays available as the quality reference / high-quality profile (33.1 ms, 627 MiB):
     not default because of the idle memory budget (docs/PERFORMANCE.md §5).
   - `q8` rejected on AVX2 CPUs: 220.8 ms (8-bit `MatMulNBits` kernels); re-test on AVX-VNNI.
3. **No GPU by default in v1.** DirectML on the GTX 1650 was 9–17× slower than CPU for queries
   (293–572 ms p50): the graph is dispatch-bound (1,100–1,700 small nodes per inference) and the
   fp32/q4 graphs leave 48 `Gelu` nodes on CPU (copies every layer). Indexing improved only
   2–3× (8.4–9.7 vs 3.2–3.9 chunks/s) for 1.1–2.9 GB VRAM. fp16 produced zero-norm vectors
   non-deterministically (the model card warns fp16 can yield NaNs); the integrated Radeon hung
   the device (DXGI 887A0006). DirectML itself is frozen at ORT 1.24 and "legacy" in Windows ML.
   DirectML support stays compiled behind the `directml` feature for diagnostics and T014.
4. **One weight variant per index generation.** Device choice (T013) may move inference between
   devices but never changes weights without a new generation (ADR-014 space key).
5. **Prompts verified** against EmbeddingGemma 2's `config_sentence_transformers.json`
   (`SearchQuery` = `task: search result | query: `, `Document` = `title: none | text: `):
   `PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1` stays.

**Consequences**

- The 60/120 ms warm-query budget is met with margin on a 2021 mainstream laptop CPU; ADR-006
  (256d) unchanged.
- **Indexing throughput is the main risk:** ~3–4 chunks/s of ~260 tokens using all cores
  (100k chunks ≈ 8 h of full CPU). T202 must chunk smaller (~128 tokens ≈ 2× throughput),
  cap background threads, prioritize by value (ADR-011); T014 looks for faster paths.
- Packaging: `onnxruntime.dll` ≈ 18 MB (CPU build) + model 174 MB + tokenizer 32 MB.
- **Evidence gaps:** one machine. Untested: Intel CPUs, AVX-VNNI/AVX-512, NPUs, RTX 30+,
  Windows ML EPs (TensorRT-RTX, OpenVINO, VitisAI, QNN), WebGPU EP, LiteRT-LM, llama.cpp.
  `scripts/t006/run-windows-bench.ps1 -Download` lets any Windows PC contribute a run.

**Revisit when** T014 finds a backend/device that beats CPU q4 on latency or indexing at
acceptable memory; or T205 relevance evaluation shows q4 losing meaningfully against fp32
(switch weights with a new index generation).

## ADR-016 — ANN: USearch HNSW, f16 storage, cosine, M=16, ef_search=256

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

## ADR-017 — SQLite store: bundled 3.53, WAL, user_version migrations, budgeted FTS5

**Status:** Accepted (T007), implements ADR-003. Evidence:
`docs/benchmarks/t007/2026-10-08-cloud-sandbox/storage-100k.json` and `lumen-storage` tests.

**Decision**

- **Bundled SQLite** (`rusqlite` 0.40 `bundled`, SQLite 3.53.2 with FTS5): identical engine on
  every machine; no dependency on a system SQLite.
- **One writer + N read-only readers, WAL, `synchronous=NORMAL`, `foreign_keys=ON`.** A reader
  is never blocked by an open write transaction (tested). NORMAL can lose the last
  transactions on power loss but never corrupts; the index is rebuildable.
- **Migrations:** embedded SQL files, forward-only, versions contiguous from 1 in
  `PRAGMA user_version`, one transaction per step (a failing step leaves no partial schema);
  a database newer than the binary is refused, readers refuse unmigrated databases.
  `0001_initial`: `items` (stable identity + case-insensitive unique path), `chunks`
  (`embedding_generation` for T203), `chunks_fts`, `settings` (JSON), `usage_events`.
- **FTS5:** external-content table over `chunks` kept in sync by triggers;
  `unicode61 remove_diacritics 2` (case/accent-insensitive: "reunion" finds "reunión");
  prefix indexes `3 4`; bm25 ranking; snippets marked with U+E000/U+E001.
- **User input is never raw FTS syntax:** `FtsQuery::from_user` quotes every term, keeps
  `"phrases"`, makes the last term a prefix only while typing and only from 3 characters
  (`MIN_PREFIX_CHARS`).
- **Interactive queries carry a `SearchBudget`** (deadline and/or `CancellationToken`)
  enforced by SQLite's progress handler; an exceeded budget returns `Interrupted`, never a
  late or partial answer.

**Evidence (100k chunks × 120 words, 2 vCPU sandbox, worst-case 50-word vocabulary)**

- Insert 8.8k chunks/s (16.9k without prefix indexes) — far above embedding speed.
- 156 MiB database; path lookup p50 2 µs.
- Per-keystroke FTS p50 0.05 ms but p95 73 ms / max 115 ms when common terms force bm25 over
  most rows; with a 20 ms budget p95 20.1 ms / max 20.4 ms (73 of 852 interrupted).

**Consequences**

- The coordinator (T107) gives FTS a per-keystroke budget and cancels stale queries;
  filename/app results (T102) cover keystrokes where FTS is interrupted or skipped.
  **Interruption affects one keystroke's query only, never the index:** when typing pauses,
  the final query is re-issued with a generous budget so complete lexical results always
  appear (T107 must test this).
- Very common terms remain the cost driver; T205 may drop high-document-frequency terms
  (fts5vocab) when selective terms exist. **Measured (bench schema v2, 100k chunks, sandbox):**
  final queries whose every term is in ~all chunks take p50 106 / p95 117 ms with 50 hits —
  the worst case (50-word synthetic vocabulary). It scales with matching rows, so at 1M chunks
  T205/T107 must either drop high-df terms or bound the final query too. Earlier "final query"
  numbers (0.05 ms) timed queries with no hits and were not evidence.
- `0001_initial.sql` may still change until the first release; afterwards only new migrations.

## ADR-018 — Inventory coverage guarantee and stable file identity

**Status:** Accepted (T009). Evidence: `crates/lumen-indexer` tests (Linux + native Windows),
`docs/benchmarks/t009/2026-10-08-joao-pc/` (Windows 11, NTFS C: and D:) and
`docs/benchmarks/t009/2026-10-08-cloud-sandbox/`.

**Decision**

- **No file is ever dropped silently.** Pass 0 (`lumen_indexer::scan`) emits every entry under
  an indexed root (at least path + name + kind), or records it as an exclusion with the rule
  that matched, or records a `ScanIssue` (stage + reason). Metadata, flag or identity failures
  degrade an entry but never suppress it. `ScanReport::is_complete()` is false whenever a
  directory could not be listed or the walk was cancelled; those paths must be retried.
- **Exclusions are visible rules:** system defaults (`$Recycle.Bin`, `System Volume
  Information`, `$WinREAgent`, `Config.Msi`, case-insensitive) plus user names/paths; each
  exclusion is reported with its rule. Hidden and system files are indexed (flagged), not
  excluded.
- **Iterative walk, links never followed:** symlinks, junctions and other name-surrogate
  reparse points are emitted as links but not traversed (no loops, no double counting);
  overlapping roots are merged. Cloud placeholders (OneDrive files on demand) are emitted from
  directory metadata only; identity is skipped for recall-on-open placeholders so scanning
  never triggers a download.
- **Identity = volume serial + 128-bit file id** (Windows, via `file-id`; handle opened with
  no data access, full sharing) or dev + inode (Unix). Names Win32 rewrites (trailing dot or
  space, reserved names like `aux.txt`) are retried through the `\\?\` verbatim path.
  Non-Unicode paths (unpaired UTF-16 surrogates) are emitted and counted.

**Evidence (Windows 11, Ryzen 5 5600H, user folders incl. OneDrive: 26.5k entries, 18.9 GiB)**

- Coverage COMPLETE, 0 issues; entry count equals an independent .NET walk (26,469 = 26,469).
  515 cloud placeholders inventoried without hydration; 3 junctions not followed.
- Without identity: 22k entries/s first pass, 122k/s warm. With identity (one handle open per
  entry): 7.3k/s first pass, 15.6k/s warm — identity is the dominant cost.
- Edge cases all reported correctly: 683-char path, junction loop, hidden+system file,
  ACL-denied folder (emitted + `ListDirectory/PermissionDenied`, coverage flagged incomplete),
  `$Recycle.Bin` excluded by rule, unpaired-surrogate name, trailing dot, `aux.txt`.
- identity-check on C: and D: (NTFS): all as expected; delete + recreate got a new id.

**Evidence (2 vCPU Linux sandbox, 240k entries, identity on)**

- First pass 33k entries/s (cold cache), second pass 349k entries/s; 0 issues; 9.7k links not
  followed; 3.6k hard-linked entries detected as shared identities.
- identity-check: rename, move, in-place edit keep identity; copy and save-by-replace get a
  new one; hard links share one; delete + recreate **reused** the inode on ext4.

**Consequences**

- T101 stores every emitted entry, including ones with failed metadata (`status = 'error'`
  with `error_code`), so they are still findable by name/path.
- `items UNIQUE(volume_id, file_id)` conflicts with hard links: T101 must store the second
  path as an alias of the same item (or relax the constraint in a new migration).
- T207: same path + new identity = update (editor save-by-replace); same identity + new path
  = rename; identity alone never proves same content (inode reuse, FAT/exFAT and some network
  shares synthesize ids) — always combine with size/mtime/fingerprint.
- Storage must keep non-Unicode paths losslessly (TEXT columns need an escape scheme, T101).
- Identity costs ~7× the walk when warm. T101/T207 should read ids in bulk per directory
  (`GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` on the directory handle) or defer
  identity to a background pass after names/paths are searchable.

## ADR-019 — Embedding device policy: CPU by default, accelerators only on measured proof

**Status:** Accepted (T013). Evidence: `docs/benchmarks/t013/2026-10-08-joao-pc/` (Ryzen 5 5600H,
GTX 1650 4 GB, driver 32.0.15.9227, ORT CPU 1.30 / DirectML 1.24.4, q4).
Refines ADR-015 §3–4. Code: `lumen_embedding::policy` (pure rules), `lumen_embedding::probe`
(measurement through the production `Embedder`), `lumen-bench probe` / `device-policy`.

**Decision**

- **CPU is always available and always the fallback**; a missing, failed or rejected
  accelerator never blocks search or indexing.
- **Same space only:** a device is considered only if its probe ran the index generation's
  exact `EmbeddingSpace` (weights, prompts, dimension). Device choice never changes weights.
- **Eligibility (all required):** successful probe; stable output (two runs ≥ 0.99999 cosine,
  no NaN/zero); vectors interchangeable with CPU ones (min cosine ≥ 0.999 on the same
  inputs); ≥ 90 % of graph nodes offloaded (placement known); device memory known and
  ≤ min(1.5 GiB, 50 % of the device); not an integrated GPU (opt-in only — T006 device hang);
  not quarantined; a successful CPU probe exists to compare with.
- **Query lane:** stays on CPU while CPU p95 ≤ 120 ms; otherwise the fastest eligible
  accelerator if ≥ 1.5× faster. Never paused.
- **Indexing lane:** the fastest eligible accelerator if ≥ 1.5× CPU throughput, only on AC,
  Balanced and idle (or Turbo); otherwise CPU with Eco 1 thread / Balanced n/2 (n/4 while the
  user is active, 1 on battery) / Turbo n−1. Paused below 20 % battery (not Turbo), on battery
  in Eco, and under 768 MiB available memory (any profile).
- **Quarantine:** a runtime `Backend`/`NonFinite`/`ZeroVector`/`OutputShape` error on an
  accelerator quarantines it for its `runtime_key` (runtime + driver versions) and the batch is
  re-run on CPU; a driver/runtime update lifts it. Cancellation/input errors never do.
- Probes run once per device per `runtime_key`, each in its own process; device memory comes
  from the platform layer (Windows: GPU Process Memory perf counters).

**Evidence (joao-pc)**

| probe | query p50/p95 ms | indexing chunks/s | cos vs CPU | offloaded | GPU memory |
|---|---:|---:|---:|---:|---:|
| cpu | 29.9 / 34.2 | 3.10 | 1 | — | — |
| dml:high (GTX 1650) | 417.8 / 513.8 | 7.28 (2.35×) | 0.9999995 | 94 % | 2,296 of 4,096 MiB |

The GPU passes stability, fidelity and placement and is fast enough for indexing, but needs
2.3 GB of a 4 GB card (limit min(1.5 GiB, 50 %)) → rejected; all 7 scenarios run on CPU
(Balanced AC idle 6 threads, active 3, battery 1, battery 15 % paused, Eco 1, Turbo 11, low
memory paused). Vectors from DirectML are interchangeable with CPU ones (cos ≥ 0.9999995), so a
future accelerator can join an existing index generation.

**Consequences**

- The probe compares against CPU at the runtime's default threads (all cores): conservative.
- `lumen-windows` (M1+) must supply power source, battery %, available memory, user activity
  and per-process GPU memory; the shell persists probes and the quarantine in settings.
