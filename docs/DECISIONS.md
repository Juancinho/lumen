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
  ≤ min(1.5 GiB, 50 % of the device) — in **Turbo** (user decision 2026-10-08) ≤ 60 % of the
  device, total memory required; not an integrated GPU (opt-in only — T006 device hang);
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
With the Turbo cap (60 % = 2,458 MiB) the same probes give `turbo → indexing on dml:high`
(2.35× faster), every other scenario unchanged (`policy-v2.json`).

**Consequences**

- The probe compares against CPU at the runtime's default threads (all cores): conservative.
- `lumen-windows` (M1+) must supply power source, battery %, available memory, user activity
  and per-process GPU memory; the shell persists probes and the quarantine in settings.

## ADR-020 — Hidden WebView: trim to low memory after 30 s hidden

**Status:** Accepted (T012), refines ADR-010. Evidence:
`docs/benchmarks/t012/2026-10-08-joao-pc/webview-lifecycle-4modes.json` (Ryzen 5 5600H,
Windows 11, WebView2 154.0.4258.62, release `lumen.exe`, 20 quick show/hide cycles + 3 shows
after 8 s hidden per mode). Code: `apps/desktop/src-tauri/src/lifecycle.rs`.

| mode | show → painted p50/p95 | after 8 s hidden | private WS hidden | commit |
|---|---:|---:|---:|---:|
| keep (window hide only) | 22.6 / 26.0 ms | 11.7 ms | 72 MiB | 146 MiB |
| invisible (`SetIsVisible(false)`) | 17.6 / 21.5 ms | 26.5 ms | 73 MiB | 146 MiB |
| low-memory (+ `MemoryUsageTargetLevel=Low`) | 27.1 / 32.0 ms | 26.7 ms | **7.4 MiB** | 149 MiB |
| suspend (+ `TrySuspend` after 5 s) | 28.6 / 31.0 ms | 30.5 ms | 94–160 MiB | 276–283 MiB |

Start-up to UI ready: 380–523 ms wall (in-process 367–408 ms). Shell process alone: 3–4 MiB.

**Decision**

- Default `idle-low-memory`: nothing on hide; after 30 s hidden, `SetIsVisible(false)` +
  `MemoryUsageTargetLevel=Low`; on show, Normal + visible. Quick re-opens get `keep` latency,
  an idle resident Lumen shows ~7 MiB in Task Manager. User-approved 2026-10-08.
- `suspend` rejected (more memory, an extra process); `invisible` gives nothing.
- Delayed trims re-check on the UI thread that no show happened since (no trim while visible).
- `LUMEN_WEBVIEW_HIDDEN` overrides the mode; `LUMEN_DIAG_LOG` records timings; second launches
  accept `--show/--hide/--toggle/--quit`.

**Consequences**

- Memory budget (<400 MB idle) met with a wide margin; commit (~150 MiB) is the real reserve.
- Low-memory trims page out, they do not free commit; re-show after a trim costs ~27 ms.
  T103 (real result list) must re-run `scripts/t012/run-windows-webview.ps1`; if show→paint
  after a trim exceeds the 35 ms p50 budget, raise `IDLE_TRIM_AFTER` or use `keep`.
- The composed `idle-low-memory` mode was not measured as such yet:
  `run-windows-webview.ps1 -Modes idle-low-memory` validates it.
- Show latency excludes hotkey delivery (measured from the shell receiving the request).

## ADR-021 — Catalog: one `items` table for files and apps, inventory sync, instant name provider

**Status:** Proposed (T101) until `scripts/t101/run-windows-catalog.ps1` runs on Windows
(AppsFolder COM enumeration is compiled by CI but not yet exercised). Evidence:
`docs/benchmarks/t101/2026-10-08-cloud-sandbox/catalog-usr-home.json`, `lumen-catalog` tests.

**Decision**

- **Provider contract** in `lumen-core::provider`: synchronous `Provider::search(&ProviderQuery,
  &CancellationToken) -> Vec<ResultItem>` with a `LatencyClass` (Instant/Fast/Semantic/Deferred);
  the coordinator (T107) owns threads. Built-in actions in `lumen-core::builtin`:
  `lumen.open`, `lumen.launch`, `lumen.reveal`, `lumen.copy-path` (executors: T109).
- **Schema (0001, pre-release edit allowed by ADR-017):** `items` gains `source`
  (files|apps), `raw_path` (exact OS path for non-Unicode names), `name_key` (folded search
  key), `launch_target`, `attributes`, `seen_scan`; table `scans`. The path is unique
  **exactly** (case-variant entries on case-sensitive dirs stay distinct, coverage) with a
  NOCASE lookup index; identity `(volume_id, file_id)` is indexed but **not unique** (hard links).
- **Inventory sync** (`lumen_catalog::sync_files`): every emitted entry is upserted (2,000 per
  transaction; metadata failures as `status='error'`, still searchable). Same path → refresh;
  unseen item with same identity + size + mtime → **move** (keeps the item id, later its
  chunks/vectors); else insert. Afterwards unseen items are removed — never after a cancelled
  pass, never under a directory that failed to list or a root that did not open.
- **Apps** (`sync_apps`): Windows AppsFolder (`lumen-windows`, Get-StartApps equivalent:
  packaged + desktop apps, launched as `shell:AppsFolder\<parsing name>`); fallback Start-menu
  shortcuts (.lnk/.url/.appref-ms). An empty discovery never wipes the app list.
- **Names**: `name_key` = NFKD, combining marks removed, lowercase, whitespace collapsed
  (`unicode-normalization`); "reunion" finds "Reunión". `CatalogProvider` (`lumen.catalog`,
  Instant): exact then prefix on `name_key`, apps first; `ResultId` = `item:<id>`.

**Evidence (2 vCPU sandbox, 245,623 entries with identity)**

- First sync 6.1 s (40k entries/s), resync 7.4 s (33k/s), 121 MiB database.
- Keystroke lookups (n=2,318, 1–8-char prefixes of sampled names): p50 0.086 / p95 0.163 ms.
- Full name returns that item in the top 10 for 88 % of sampled names (misses are names shared
  by >10 items — `python`, `__init__.py` — ranking is T102).
- Regression found and fixed: without statistics SQLite used `items_modified` for the move
  lookup (many files share an mtime) → 2.2k entries/s; `INDEXED BY items_identity` + a plan test.

**Consequences**

- T102 adds token-prefix ("code" → "Visual Studio Code"), fuzzy and path matching, and ranking
  signals; T106 recency/frequency; T207 incremental updates reuse `upsert_entries`/moves.
- `sync_files` must receive the complete root set (roots no longer configured are removed).
- Query-plan stability: catalog statements that matter keep explicit index hints or plan tests.

## ADR-022 — Name/path matching: tokenized names in FTS5, Rust scoring, bounded stages

**Status:** Proposed (T102) — validated on Linux; the Windows run of
`scripts/t101/run-windows-catalog.ps1` (apps + user folders) accepts it together with ADR-021.
Evidence: `crates/lumen-catalog/tests/relevance.rs` over `fixtures/search/catalog-relevance.json`,
`docs/benchmarks/t102/2026-10-08-cloud-sandbox/catalog-usr-home.json`.

**Decision**

- **Tokens:** names split code-aware (`MyComponentName` → my component name, `HTTPServer` →
  http server, `q3budget2025` → q 3 budget 2025), folded (case, accents). `items.name_parts` =
  tokens + original words + initials of the stem (`vsc`, `upc`); `items.path_parts` = tokens of
  the 3 nearest parent folders. Both indexed by `names_fts` (FTS5 external content, prefix
  indexes 1–3; update trigger only fires when the token text changed).
- **Candidates (bounded):** (1) exact + prefix on `name_key` (index range, always);
  (2) FTS5 token-prefix with terms of 3+ characters, single term restricted to `name_parts`,
  best bm25 first, 300 rows, own 8 ms slice; (3) typo candidates with the same first two
  characters, 1,000 rows, own 4 ms slice, only while results are scarce. Stages 2–3 degrade to
  stage-1 results when their slice runs out.
- **Scoring (`lumen_catalog::rank`):** exact 1.0 > exact stem 0.97 > name prefix 0.80–0.92 >
  all tokens prefix name tokens in order 0.78–0.86 / any order 0.70–0.78 (initials included) >
  name + folder tokens 0.50–0.60 > typo (OSA distance ≤1 at 4–7 chars, ≤2 at 8+) 0.29–0.37.
  Priors: app +0.06, modified <3 days +0.04 / <30 days +0.02, hidden/system −0.15, depth
  beyond 4 levels −0.004/level (max −0.04). Ties: shorter name, then id.

**Evidence**

- Relevance set (12 apps, 30 files, 40 queries incl. typos, initials, code names, Spanish,
  folder+name, app-vs-file collisions): MRR@10 1.000, R@1 1.000 (CI test asserts MRR ≥ 0.95).
- 2 vCPU sandbox, 246,912 entries (/usr + home, worst case: 100k `lib*` names): keystroke
  p50 0.76 ms, p95 7.9 ms, max 10.4 ms (p95 by prefix length 1–2: 0.3 ms; 3–8: ~8 ms — the
  FTS slice). First sync 19.2 s (12.9k entries/s; FTS maintenance ~3× the T101 cost), resync
  10.2 s, 173 MiB database.

**Consequences**

- The relevance fixture is the regression gate for any ranking change (add cases, never
  lower the threshold silently). Usage/recency signals (T106) plug in as priors.
- Inventory throughput halved by the token index; acceptable for Pass 0 (1M entries ≈ 80 s in
  the background). Revisit if T207 incremental updates show it matters.
- Query syntax filters (`ext:`, `in:`, `type:`) are T208; extension tokens already match.

## ADR-023 — Usage signals: aggregates only, decayed frecency, learned query choices, pins

**Status:** Accepted (T106). Code: `lumen_storage::usage`, `lumen_catalog::rank::usage_prior`.

**Decision**

- **No raw event log** (docs/PRIVACY_SECURITY.md §2, least retention): the `usage_events`
  table is replaced (pre-release 0001 edit) by aggregates — `usage_stats` (uses, last use,
  decayed score), `query_choices` (folded query prefix ≤ 32 chars → item, uses, last use),
  `pins` (never expire). `prune_usage(cutoff)` = retention, `clear_usage()` = delete history
  (pins stay); all cascade when an item disappears.
- **Frecency:** exponential decay, half-life 14 days; stored as `rank_key = ln(score) + λt`
  (log-sum-exp updates), so ordering by recent use is an index scan with no periodic rewrite.
  Weights: primary action 1.0, reveal 0.5, copy path 0.3.
- **Learned results:** a primary action after typing `q` records every prefix of `q`; the
  provider adds those items as candidates (even without a name match, base 0.6).
- **Priors (bounded):** pinned +0.05; frecency +min(0.08, 0.025·ln(1+f)); learned choice for
  the exact current query +min(0.35, 0.12 + 0.08·ln(uses)) — a habit (≈8 picks) can lift
  "Visual Studio Code" above a folder literally named `code`; one pick cannot.
- **Empty query** returns suggestions: pins (oldest first), then highest frecency.

**Consequences**

- Recording happens when actions execute (T109: `Store::record_use(item, kind, query_key, now)`).
- Settings (T8xx) must expose retention and "clear history"; default retention TBD (90 days
  proposed).
- Sandbox: usage lookups add ~0.3 ms p50 per keystroke (head of 50 candidates).

## ADR-024 — Window material: system Acrylic by default, Solid fallback, native corners

**Status:** Proposed (T004) — pending the Windows run (`scripts/t004/run-windows-material.ps1`)
and a visual review. Code: `lumen_windows::material` (pure plan + DWM/WinRT probes),
`apps/desktop/src-tauri/src/material.rs`, `apps/desktop/src/design/material.css`.

**Decision**

- **Materials:** `acrylic` = DWM `DWMSBT_TRANSIENTWINDOW` (Microsoft's material for
  transient, light-dismiss surfaces), `mica` = `DWMSBT_MAINWINDOW` (wallpaper-only), `solid` =
  no backdrop. Applied through Tauri window effects on a transparent window (one system blur;
  the UI never adds CSS blur). `auto` (default) = Acrylic.
- **Only documented APIs:** backdrops from Windows 11 22H2 (build 22621). Older builds get
  `solid`, never the undocumented `SetWindowCompositionAttribute` (Windows 10 acrylic) or
  `DWMWA_MICA_EFFECT` (21H2) paths. Windows 10 = solid, square corners.
- **Accessibility wins:** high contrast or "Transparency effects" off → `solid` whatever was
  chosen (CSS `forced-colors` then uses system colours). Re-read before every show (two WinRT
  property reads); re-applied only when the plan changes. The tray says when a choice fell
  back and why.
- **Corners and shadow are native:** `DWMWCP_ROUND` (8 px at 100 %, DPI-scaled) with the DWM
  shadow and 1 px system border. The 20 px target in DESIGN_SYSTEM §3 is not reachable with a
  system backdrop (DWM clips the backdrop to its own radius; a CSS-rounded surface inside a
  transparent window would show square blurred corners and needs a click-through margin for a
  CSS shadow). `radius-xl` therefore follows the system radius; nested radii stay below it.
- **Legibility floor in tokens, not borders:** the UI paints the surface colour at
  `--lumen-tint-alpha` 0.76 over the backdrop. With the tint alone (ignoring the system
  material's luminosity layer) primary text stays ≥ 7:1 and secondary ≥ 3:1 over a pure
  black or white backdrop; on solid, secondary ≥ 4.5:1. `material.test.ts` enforces this.
- **Choice:** tray → Window material (Automatic / Acrylic / Mica / Solid), saved as
  `appearance.material`; `LUMEN_MATERIAL` overrides it for one run (benchmarks).

**Consequences**

- T103 builds on `--lumen-background`, `--lumen-text-primary/secondary` and the corner
  attribute (`<html data-material data-corners>`); it must not introduce a second blur layer.
- The Windows run decides: show→paint cost per material (must stay within the 35 ms p50
  budget), DWM GPU load while visible, real composited contrast over bright/dark windows, and
  whether `auto` should be Mica instead (calmer, no busy-window bleed). Then Accepted.
- Battery saver and inactive windows are handled by Windows (the backdrop turns solid).
