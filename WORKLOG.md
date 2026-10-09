# WORKLOG.md

Append-only. Keep entries compact.

## 2026-10-07 — specification refinement

- Reframed Lumen from semantic file search to local semantic command center.
- Added Alfred/Raycast-inspired providers, universal actions, workflows, snippets/clipboard/quicklinks and Windows command surface.
- Added Lumen-specific Semantic Drop, Context Lens, Semantic Workspaces and Rewind direction.
- Strengthened shell-agnostic core, one-WebView/resource discipline, multi-pass resumable indexing and agent handoff rules.
- No application code implemented yet.

## 2026-10-07 — T001 workspace baseline (claude)

- Initialized git repo (`main`); committed spec pack as baseline, then T001.
- Cargo workspace: `lumen-core`, `lumen-desktop` (Tauri 2.12 shell), `xtask`; pinned Rust 1.97.0; shared lints/profiles (`release`, `profiling`).
- React 19 + TS 6.0 + Vite 8 frontend; ESLint 9 (type-aware strict + jsx-a11y strict), Prettier, Vitest + Testing Library.
- Boundary enforcement: `cargo xtask arch` (transitive + declared deps, all platforms/kinds) and ESLint restriction of `@tauri-apps/*` to `src/ipc/`. Negative tests confirmed both fail on violations.
- IPC smoke path `core_info`: React → `src/ipc` → async Tauri command → `lumen-core` → camelCase DTO; verified rendering under strict CSP in a Linux Xvfb run.
- Pending: Windows `npm run tauri build` on real hardware (T001 in REVIEW).

## 2026-10-07 — T011 universal domain contracts (claude)

- T001 confirmed on Windows by user → DONE.
- `lumen-core`: `ids` (namespaced `ProviderId`/`ActionId` with const validation, entity-stable `ResultId`, JS-safe `QueryId`), `result` (`ResultItem`, `ResultKind`, `IconRef`, normalized `Confidence` + `MatchKind`, Rust-only `Payload`), `capability` (bitset), `action` (`ActionSafety`, `ActionGroup`, descriptor consistency), `contract::validate_result`, `execution` (`ActionRequest` → `ExecutionContext::authorize`, `CancellationToken`).
- ADR-013 (shell-owned wire DTOs, no serde in core); COMMAND_MODEL §0 and DEVELOPMENT updated.
- 32 unit + 3 doc tests (incl. compile-fail for invalid built-in id); `ResultItem` = 224 B, guarded ≤ 256 B.

## 2026-10-07 — T002 overlay prototype (claude)

- Shell: `overlay/` (show/hide/toggle; pure `placement` + `policy` with tests), `shortcut.rs` (tauri-plugin-global-shortcut 2.4, fixed Alt+Space, non-fatal on conflict), `tray.rs` (tray-icon feature; Show/Quit), single-instance plugin 2.5, `hide_overlay`/`overlay_ready` commands, `lumen:overlay-shown` event; `--background` start flag.
- Window: hidden, undecorated, always-on-top, skip-taskbar, 800×64 logical, no resize; close hides.
- UI: minimal `SearchField`; focus + select on show; Escape hides except during IME composition. 10 frontend tests, 7 shell tests.
- Linux Xvfb smoke: placement (560,216 on 1920×1080), typing reaches input, Escape hides, Alt+Space conflict handled gracefully (openbox owns it). Native show path 0.3–3 ms. Windows interactive check pending → REVIEW.

## 2026-10-07 — T005 embedding backend interface + benchmark harness (claude)

- T002 confirmed on Windows by user → DONE.
- New `crates/lumen-embedding`: sync `EmbeddingBackend` trait, `Embedder` (EmbeddingGemma retrieval prompts v1, batching, cancellation, shape/NaN/zero checks, Matryoshka 768→256 + L2 in f64), `EmbeddingSpace` key, deterministic feature-hashing `MockBackend` with simulated latency. 25 tests.
- New `crates/lumen-bench`: `lumen-bench embed` (cold load, first/warm query p50/p95/p99 vs 60/120 ms, doc throughput per batch size, resident memory, machine metadata, JSON). 9 tests.
- Release run in sandbox (mock): Embedder overhead ≈ 2 µs/query p50; simulated 40 ms call measured 42.2 ms (harness accuracy check). ADR-014.

## 2026-10-07 — T006 EmbeddingGemma 2 runtime benchmark (claude)

- Verified EmbeddingGemma 2 (released 2026-10-06) prompts = our PromptFormat v1.
- `crates/lumen-embedding-ort`: ORT dynamic loading (API 24), CPU/DirectML (index or high/low preference), placement diagnostics via verbose session logger, fidelity test vs Python fp32 reference (`fixtures/embedding/`).
- `lumen-bench`: `--backend ort`, long-input probe, `--reference`, `--placement`. Windows matrix script `scripts/t006/run-windows-bench.ps1`.
- Windows results (Ryzen 5 5600H / GTX 1650 / Vega): CPU q4 30 ms p50, 168 MiB; fp32 33 ms, 627 MiB; q8 221 ms. DirectML 293–572 ms (dispatch-bound, Gelu on CPU for fp32/q4), fp16 zero-norm, iGPU device hang. First DML run failed by over-strict `disable_cpu_ep_fallback` (fixed: fallback allowed + placement report).
- ADR-015: ORT + CPU + q4 default; GPU deferred to T013/T014.

## 2026-10-08 — T008 ANN benchmark (claude)

- `crates/lumen-vector`: typed USearch 2.26.4 wrapper (Scalar f32/f16/bf16/i8, cosine/IP, HNSW params, add/search/remove/save/load/view).
- `lumen-bench ann`: embedding-like synthetic data calibrated on real EmbeddingGemma 2 vectors, exact ground truth, ef sweep, persistence, deletes; `--vectors` for real embeddings; `scripts/t008/run-windows-ann.ps1`.
- Sandbox: 100k f16 = f32 recall (1.000 @ef64), half memory; 1M needs ef 256 for 0.99 (1.3 ms); i8 caps at 0.85; bf16 0.995. ADR-016. Windows latency run and real-vector recall left as optional evidence.

## 2026-10-08 — T007 SQLite + migrations + WAL + FTS5 (claude)

- `crates/lumen-storage`: `Store::open_writer/open_reader`, migrations (0001_initial), items/chunks CRUD needed for proofs, `search_chunks` with bm25 + snippets, `FtsQuery::from_user`, `SearchBudget` (progress handler), checkpoint, query plans. 20 tests (FTS5 present, accents, prefix/phrase/symbol, hostile input, trigger sync, cascade, NOCASE unique path + index use, atomic batches, WAL reader isolation, budget interrupts).
- `lumen-bench storage`: 100k chunks → 8.8k inserts/s, keystroke FTS p95 73 ms unbounded → 20 ms with budget. ADR-017.

## 2026-10-08 — T009 file enumeration + stable identity (claude)

- User requirement: no file may go unindexed silently → coverage guarantee designed in (ADR-018).
- `crates/lumen-indexer`: iterative scan (emit / excluded-with-rule / issue-with-stage), links and junctions not followed, overlapping roots merged, hidden/system/cloud flags, recall-on-open placeholders never opened, identity via `file-id` with verbatim-path retry. 12 tests + 2 Windows-only (junction loop, trailing-dot/reserved names) and hidden attribute via `attrib`.
- `lumen-bench scan` / `identity-check`; `scripts/t009/run-windows-scan.ps1` (edge cases incl. >260-char path, junction loop, ACL-denied folder, unpaired surrogate; .NET oracle count; per-drive identity).
- Sandbox: 240k entries, 33k/s cold, 349k/s warm, 0 issues; identity-check as designed (inode reused on recreate). ADR-017 note: budget interruption never affects the index.
- Windows (joao-pc): first build failed to link usearch (numkong `dllimport` vs static lib) — T008 had never been built on MSVC; fixed via `CXXFLAGS_*_pc_windows_msvc=/DNK_DYNAMIC=` in `.cargo/config.toml`. Then: 12/12 tests, 7/7 edge cases, 26,469 entries = .NET oracle, 515 OneDrive placeholders not hydrated, identity 7.3k/s cold. T009 → DONE, ADR-018 Accepted.

## 2026-10-08 — T010 CI baseline + release-mode benchmark command (claude)

- `.github/workflows/ci.yml`: frontend job; rust matrix ubuntu-24.04/windows-2025 (UI build → fmt → clippy `--locked` → directml clippy → tests → arch); bench matrix running `cargo xtask bench --quick` with JSON artifacts, non-gating. actionlint clean.
- `cargo xtask bench [--quick] [--out DIR]`: every model-free lumen-bench subcommand in release mode. Sandbox quick run 31 s.
- REVIEW until the first GitHub run (no remote configured yet).

## 2026-10-08 — T013 embedding device selection + fallback policy (claude)

- `lumen_embedding::policy` (pure, 14 tests): CPU default/fallback; same-space rule; eligibility gates (probe ok, stable, cos >= 0.999 vs CPU, >= 90% offload, memory <= min(1.5 GiB, 50%), no iGPU by default, CPU baseline); query lane CPU while p95 <= 120 ms; indexing lane accelerator >= 1.5x only on AC+idle/Turbo; thread counts per profile; pauses (low battery, Eco on battery, memory pressure); quarantine per runtime key.
- `lumen_embedding::probe::measure` via the production Embedder; `EmbeddingBatch::try_from_flat`.
- `lumen-bench probe` / `device-policy`; `scripts/t013/run-windows-device-probe.ps1` (process per device, GPU memory from perf counters, iGPU only with -IncludeIntegrated).
- First CI run (repo pushed by the user): frontend + Linux green, Windows `cargo test` red (log needs repo admin).

## 2026-10-08 — CI fix (Windows doctests) + storage bench correction (claude)

- Windows CI `cargo test` failure reproduced by the user: lumen-core doctests fail to link (`__CxxFrameHandler3`, `memmove`, `mainCRTStartup`). Cause: tauri-build's static VC runtime writes an empty `msvcrt.lib` into lumen-desktop's OUT_DIR and cargo gives that search path to every doctest of the same invocation. Fix: `cargo xtask test` runs the workspace without the shell, then the shell; CI and the documented gate use it. Static VC runtime kept for releases.
- `lumen-bench storage` final queries never matched the synthetic corpus (0 hits, 0.05 ms = empty result sets). Schema v2 times vocabulary queries with hits (`fts_final`) and keeps the realistic no-hit ones (`fts_final_no_hits`). 100k worst case: p50 106 / p95 117 ms → ADR-017 consequence updated.
- First Windows `cargo xtask bench --quick` (joao-pc): ANN 20k f16 R@10 0.997, ef256 p95 0.31 ms (numkong haswell dispatch active); FTS keystroke p95 31.8 ms → 20.2 ms with budget.
- T013 Windows run: CPU q4 29.9/34.2 ms, 3.10 chunks/s; GTX 1650 DirectML 417.8/513.8 ms, 7.28 chunks/s, cos 0.9999995, 94% offloaded, 2296 MiB VRAM -> rejected by the memory cap; policy = CPU in all 7 scenarios. T013 DONE, ADR-019 Accepted.
- User decision: Turbo may use up to 60% of device memory (`turbo_max_device_memory_fraction`). Re-run on joao-pc probes: Turbo indexes on dml:high, others CPU. `device-policy` now takes the CPU count from the probed machine.

## 2026-10-08 — T012 WebView lifecycle/RAM spike (claude)

- First run produced nothing: the script built lumen.exe without `tauri/custom-protocol`, so the UI loaded the dev URL (documented in DEVELOPMENT; script fixed).
- joao-pc, 4 modes: keep 72 MiB hidden / 22.6 ms p50 show->paint; invisible no saving; low-memory 7.4 MiB / 27.1 ms; suspend worse (94-160 MiB). Start-up ~0.4-0.5 s to UI ready.
- Default `idle-low-memory` (trim after 30 s hidden), approved by the user. ADR-020. CI compiled the WebView2 COM code on windows-2025 (74d2bcd green).
- T010 DONE (CI green). M0 done for the M1 gate.

## 2026-10-08 — T101 app/file catalog provider (claude)

- Provider contract + built-in action ids in lumen-core; schema 0001 extended (pre-release); catalog storage API with move detection by identity+size+mtime and coverage-safe removal (not after cancel, not under unlistable dirs/missing roots).
- New crates: lumen-windows (AppsFolder enumeration, COM), lumen-catalog (fold, lossless paths, sync_files, sync_apps with Start-menu fallback, CatalogProvider exact/prefix, apps first). 15 + 6 + 3 new tests.
- Sandbox 245k entries: first sync 112 s -> found the planner using items_modified for the identity lookup; INDEXED BY items_identity -> 6.1 s (40k/s); writer cache_size tested and reverted (no gain). Keystroke lookup p50 0.086 ms.
- `scripts/t101/run-windows-catalog.ps1` for the Windows validation (user asleep; REVIEW).

## 2026-10-08 — T102 filename/path exact/prefix/fuzzy retrieval (claude)

- Code-aware tokenizer (camel/acronym/digit splits, words, stem initials), parent-folder tokens, `names_fts` (external content, change-only update trigger).
- Ranking module with bounded priors; typo matching by OSA distance with a length-based budget; candidate gathering in three stages, stages 2-3 best-effort with own time slices.
- Committed relevance fixture (40 queries): MRR@10 1.000. Sandbox 247k entries: first measurements p95 25 ms (FTS bm25 over 1-2 char prefixes, path tokens on single words, fuzzy DP allocations) -> fixes -> p95 7.9 ms / max 10.4 ms.

## 2026-10-08 — T106 recent/frequency/pin signals + local usage store (claude)

- Aggregate-only usage store (privacy): decayed frecency with time-independent rank key, learned choices per typed prefix, pins; retention + clear; cascade on item removal. 5 storage tests.
- Ranking: bounded usage priors; learned items surface without a name match; empty query suggestions. Provider test: Calculator becomes the top result for "s" after 5 picks; pins lead suggestions.
- Latency impact +0.3 ms p50 (sandbox, 247k entries). ADR-023.

## 2026-10-08 — T003 configurable global shortcut + conflict UX (claude)

- Tray submenu with 4 shortcut choices (checks, "(in use by another app)" via register/release probes); apply = register new, release old, rollback on failure; saved choice persisted in the new app-data SQLite settings (`lumen.db`), never silently replaced; first-free fallback only when unset.
- `lumen_storage` settings get/set/remove. Shell tests for labels/order/tooltips; Linux Xvfb smoke OK. REVIEW: Windows interactive check (conflicts are Windows-specific).

## 2026-10-08 — T004 Windows material/backdrop spike (claude)

- Plan in `lumen-windows::material` (pure, tested): Acrylic (transient system backdrop) by default, Mica optional, Solid for high contrast / transparency off / builds < 22621; documented DWM APIs only; native rounded corners from 22000.
- Shell applies it via Tauri window effects on a transparent window, re-checks system settings before each show, tray "Window material" submenu (saved setting, fallback reason shown), `LUMEN_MATERIAL` override, diag events.
- UI surface tokens (`src/design/material.css`) with a WCAG floor over any backdrop enforced by `material.test.ts` (tint 0.76: primary >= 7.4:1, secondary >= 3.4:1 worst case; solid secondary >= 6:1).
- 20 px radius not reachable with a system backdrop -> native 8 px (ADR-024 proposed). `scripts/t004/run-windows-material.ps1` measures show latency, DWM GPU, on-screen contrast and saves private screenshots. Windows-only code compiles only in Windows CI.

## 2026-10-08 — T101 Windows build fix (claude)

- First Windows build of the T101 AppsFolder code failed: `SIGDN_PARSINGNAME` does not exist (-> `SIGDN_PARENTRELATIVEPARSING`); `IEnumShellItems::Next` takes `Option<*mut u32>`. Windows-only code is unverifiable in the Linux sandbox: push before the Windows scripts so CI catches it first.

## 2026-10-08 — T103 premium root-search overlay (claude)

- Design tokens (spacing, radius, type, geometry, motion, interaction colours incl. forced-colors and reduced-motion) next to the T004 material tokens.
- Root search components: search bar with glyph/clear/combobox ARIA, result rows (icon tile, title, middle-truncated location keeping the last folder, kind label, "Open ↵" on selection), quiet no-results message that never flashes while searching.
- Deterministic window height from content (max 8 rows), shell `resize_overlay` capped at 72 % of the work area so the bar never moves; entrance = 150 ms content fade.
- Visual pass in headless Chromium (light/dark/acrylic/narrow): fixed text alignment (18 px optical gap), path tail priority and separator placement. 39 frontend tests.

## 2026-10-08 — Windows results + T107 progressive result stream (claude)

- User's Windows runs: T101 catalog (26.5k entries complete, 330 AppsFolder apps after the SIGDN fix, keystroke p95 5.3 ms; 92 % full-name top-10 because of widely duplicated names) -> T101/T102 DONE, ADR-021/022 Accepted. T004 dark run: ~22 ms show->paint for every material, Acrylic +3-4 % DWM GPU while visible, on-screen secondary contrast >= 5.3:1 -> ADR-024 accepted on measurements, default open to visual review.
- New `lumen-search` crate: coordinator (latency-class order, merged updates, provider failures reported) and latest-wins search thread (cancel running, drop stale ids, silent superseded queries). 9 tests.
- Shell: `search` command + `lumen:results` events, background catalog sync (apps + standard folders at start-up and every 30 min). UI `useResults` with stale-id filtering and no blank flash. Linux Xvfb smoke with the real app: results render, 0.4-2.2 ms per query in-process. ADR-025.

## 2026-10-08 — T104 keyboard navigation + stable selection (claude)

- Pure keymap (launcher keys claimed, text editing/IME untouched) and an id-following selection (top row until moved; follows its result through re-orders; keeps the position when it disappears). 51 frontend tests.

## 2026-10-08 — T108 Action Panel + T109 core actions (claude)

- Core: contextual action listing in panel order and request preparation over the existing `ExecutionContext::authorize`; recent-results window (4 queries) in the search service for id lookup; catalog usage recording from actions.
- Shell: executors (open/launch via ShellExecute, reveal via SHOpenFolderAndSelectItems, copy path via arboard), `list_actions`/`run_action`, usage recorded, overlay hides on success.
- UI: Enter/click primary, Ctrl+Enter reveal, Ctrl+K panel with keyboard, Escape closes the panel before dismissing, failure notice on the row. 58 frontend tests; Linux smoke of the panel. ADR-026.

## 2026-10-08 — T110 diagnostics mode (claude)

- Env-gated provider/match/confidence per row and per-query timing/failures; fields are omitted from IPC payloads when off. 59 frontend tests.

## 2026-10-08 — T105 Quick Look preview shell (claude)

- Bounded preview reads (16 KB, text extensions, binary/undecodable rejected), preview by result id; overlay grows to the right keeping the search bar in place, one-pane fallback on narrow monitors; preview follows the selection; Escape order panel > preview > overlay. 66 frontend tests; Linux smoke screenshot.

## 2026-10-08 — T016 storage bench corpus fix (claude)

- `lumen-bench storage` now inserts a Zipf(1) corpus over VOCAB + every realistic query term and reports hits for keystroke, vocabulary-final and realistic-final queries (schema v3), warning on zero-hit sets.
- Sandbox 100k chunks: keystroke FTS p50 13.3 / p95 67.6 ms with real hits, 44 % interrupted at the 20 ms budget -> ADR-017 corrected; content FTS moves to the settled-query lane in M2.


## 2026-10-08 — T111 specified: indexed locations + exclusions (claude)

- User request: index other drives besides the standard folders on C:, and exclude folders. Today roots are hard-coded in the shell (`catalog.rs`).
- Spec `docs/specs/T111-indexed-locations.md` (reuses `ScanOptions`/`Exclusions`/`sync_files`; tray UI + Action Panel "Exclude this folder"; versioned `index.locations` setting; marker-based build exclusions; whole-drive scale measurement). Added as T111 TODO in M1. Kept out of SEARCH_AND_INDEXING.md while T015 edits it.

## 2026-10-08 — T015 docs consolidation (claude)

- ADRs split into `docs/adr/adr-NNN-*.md`; `docs/DECISIONS.md` is now the index table.
- §0 implementation status in ARCHITECTURE, SEARCH_AND_INDEXING, PERFORMANCE (measured-vs-budget table), COMMAND_MODEL, PRIVACY_SECURITY, TESTING (DESIGN_SYSTEM already had one).
- Fixed sections contradicted by ADR-014..018/023/025: sync embedding trait, f16 decided, literal schema pointer, keystroke vs settled lanes, initial roots; crate layout only in DEVELOPMENT.md; Refinement appendices merged as numbered sections (code comments re-pointed).
- One reading order (AGENTS.md), fixed roles table (AGENT_PROTOCOL §1), ordered Next list (TASKS.md), HANDOFF trimmed to live REVIEW items; PERFORMANCE §9 throughput budget (>= 8 chunks/s @ <= 50 % CPU, proposed) and §5 single memory metric.

## 2026-10-08 — T111 indexed locations + exclusions (claude)

- Spec found uncommitted in `docs/specs/` (added outside this session); committed with T015 and implemented.
- Indexer: default directory names (developer noise, `venv` only with `pyvenv.cfg`) and build folders only next to project markers, reported by rule. Catalog: versioned `index.locations` model (unknown fields kept, newer versions read-only), per-location states, progress callback.
- Shell: live model with cancel-and-restart passes and progress events; tray submenus with native folder picker; `lumen.exclude-folder` action. ADR-027 (open questions resolved by default).
- Sandbox whole filesystem with defaults: 309k entries, first sync 38 s, resync 12.5 s, keystroke p95 7.1 ms.

## 2026-10-08 — T201 text/code extractor + chunker (claude)

- New `lumen-extract` crate: document kinds, bounded decoding with skip reasons, chunkers for prose (paragraph/sentence/word), Markdown (heading paths, intact fences, no heading-only chunks), code (top-level regions, class members, overlapping line windows, symbol names across 15+ languages) and data.
- `lumen-bench chunk` with an optional real-tokenizer check: estimator recalibrated (4 -> 5 chars per token unit) to estimate/real p50 ~1.13; 63 % of code chunks named. ADR-028.

## 2026-10-08 — T014 throughput spike instrumentation (claude)

- `lumen-bench embed` gains `--backend llama-server` (llama.cpp `llama-server --embedding` over localhost HTTP; GGUF variants are distinct spaces) and per-batch CPU accounting (`--cpu-pid` for the server), report schema v2. `lumen_windows::process::cpu_time` (GetProcessTimes); Linux reads `/proc/<pid>/stat`.
- `scripts/t014/run-windows-throughput.ps1`: ORT q4 thread sweep + q8 + DirectML fp16, llama.cpp latest release (cpu/vulkan/cuda 12) on Q8_0 and UD-Q4_K_XL GGUF, fidelity each run, summary with chunks/s per core and the §9 budget check.
- Sandbox ORT q4: 3.4 chunks/s per busy core, linear 1→2 threads, batch size irrelevant on CPU. Verdict waits for the joao-pc run.

## 2026-10-08 — T202 content pass + persistent embedding queue, core (claude)

- New `lumen-content` crate: content pass (text files new/changed/failed/stale → extract → chunk → replace chunks, 32 files per transaction, scope predicate for name-only locations) and `run_queue` (keyset over chunks without a vector in the generation, batches of 8, pause, interactive holds, duty cycle, time slices; device failures abort, other failures isolated per item).
- Storage migration 0002: per-item content state, `generations`, `chunk_vectors` (f16 LE; failed rows carry a code), `chunks.embedding_generation` dropped; v1 databases with chunks upgrade (test).
- `lumen-bench pipeline`: sandbox queue overhead 0.026 ms/chunk, ~600 B per vector, restart resumes with 0 pending; ORT q4 4.6 chunks/s at 2 threads, duty 0.5 → 50 % CPU at 2.3 chunks/s vs 1 thread → 50 % at 2.8. ADR-029. Added T210 (model/runtime provisioning).

## 2026-10-08 — T202 shell integration (claude)

- The catalog thread now runs catalog pass → content pass → 30 s embedding-queue slices (re-checking catalog work between slices); the plan comes from the device policy with live power / free memory / input idle (`lumen_windows::system`), the model unloads when the queue drains, and the model is configured by env until T210.
- Tray → Content indexing (status line, remembered Pause) and per-location "Index file contents"; `index.locations` v2 (v1 `names` upgrades to `names+content`).
- Linux smoke with the real app: 154 chunks embedded into generation 1; `scripts/t202/run-windows-indexing.ps1` for the Windows check. T202 → REVIEW.

## 2026-10-08 — T204 warm query embedding service (claude)

- New `lumen-semantic` crate: `QueryEmbedder` (own runtime session on one worker thread, latest-wins with `Superseded`, cancellation, 64-entry cache, warm/unload/idle unload, stats without query text) that holds the indexing queue while queries arrive (+1.5 s linger).
- `lumen_content::Control::{mark_interactive, interactive_within}`: the queue embeds one chunk per call for 10 s after interactive use; the shell marks it when the overlay is shown.
- `lumen-bench query-lane` (alone / with indexing / preempted): sandbox 2 vCPU q4 p95 60 ms alone, 189 ms next to indexing, 184 ms preempted with 8-chunk batches, 70 ms with 1-chunk batches. ADR-030 (proposed until the Windows run); `scripts/t204/run-windows-query-lane.ps1`.

## 2026-10-08 — T203 persistent ANN generations (claude)

- Migration 0003: per-generation write sequence numbers on `chunk_vectors`, `ann_files`; `lumen_storage::generations` (states, `promote_first`, atomic `activate_generation`, batched `delete_retired_vectors`, snapshot/delta/seq readers).
- `lumen_semantic::SemanticIndex`: mmap'd HNSW file (written `.tmp` → rename → recorded) + exact delta, candidates validated by seq so deleted, re-embedded and reused chunk ids never return old vectors; missing/mismatching files degrade to rebuild; `maintenance`, `build_file`, `validate` (drained, ≤ 1 % failed, self-recall), `cleanup_files`. `IndexConfig::fingerprint` in lumen-vector.
- Shell: the indexing thread promotes the first generation, refreshes/rebuilds the ANN after slices, activates validated later generations and deletes retired vectors. Linux smoke: v2 database migrated to v3, generation active, re-embedding with seqs.
- `lumen-bench ann-gen` (also in `cargo xtask bench`): sandbox 100k search p50 0.66 ms (2.3 ms with a 10k delta), recall@10 ≥ 0.998, build 25 s, open 7 ms. ADR-031.

## 2026-10-08 — T205 hybrid fusion + evaluation harness (claude)

- `lumen_search::fuse` (weighted RRF, exact/intent first, one row per entity with the strongest copy and any snippet) replaces the confidence merge; `SearchService` re-runs a typing query as settled after 80 ms so settled lanes answer.
- `lumen_catalog::ContentProvider` (settled FTS over chunks, one file per result, plain snippet; `FtsQuery::content` drops English/Spanish function words, `two_of` fallback) and `lumen_semantic::SemanticProvider` (query lane + active generation, space check, relative floor, passage excerpt; `Store::chunk_refs`); `QueryEmbedder::space_key`.
- Shell: three weighted lanes, query embedder with indexing preemption and 10-min idle unload, warmed on overlay show; ANN maintenance no longer holds the index lock while building.
- `fixtures/eval` (48 synthetic documents EN/ES, 56 judged queries) + `lumen-bench eval [--sweep] [--explain]`; sandbox q4: fused top-1 0.982 / NDCG 0.986 vs meaning 0.964, contents 0.536, names 0.179. An any-word content fallback cost 0.18 top-1 and was replaced. ADR-032; T211 (harder set) added.

## 2026-10-08 — T211 harder relevance set (claude)

- `scripts/eval/make_hard_set.py` → `fixtures/eval-hard/` (162 synthetic documents: monthly bill series, client × topic notes, report versions, long handbooks, one function in four languages, logs/CSV/config noise; EN/ES) with graded judgments; `lumen-bench eval` reads `related` (grade 1) and sweeps name 0.5–2 × content/semantic 0.5–3.
- Sandbox q4: meaning alone NDCG 0.964, fused 1/1/1 0.954, 1/1/2 0.957 (lexical 0.870, code 0.920). App and harness default to 1/1/2. ADR-033 with the two lexical-lane findings.

## 2026-10-08 — T210 model + runtime provisioning (claude)

- New `lumen-provision` crate: pinned components (EmbeddingGemma 2 q4 ONNX at a fixed Hugging Face commit; ONNX Runtime 1.30.0 win_amd64 wheel), `CurlFetch` (system curl, https only, resumable, cancellable) and `DirFetch` (local folder), staged/verified/atomic install with a minimal zip reader, `state`/`verify`/`remove`. Real downloads verified in the sandbox (207 MB in 15 s; 14 MB wheel extracted and hashed).
- Shell `provisioning.rs`: resolution env → beside the exe → installed; tray → Semantic search (status, Download… with consent dialog, progress, Cancel, Remove…); indexing and the query lane retry after an install (`QueryEmbedder::retry`). ADR-034; PRIVACY_SECURITY network statement updated.

## 2026-10-08 — T206 progressive refinement (claude)

- Coordinator holds back a settled run's intermediate lists for 150 ms (`SETTLED_BATCH`): one refinement burst with a warm model; slow lanes still let earlier ones show.
- UI `stabilize`: after the user moves the selection, refinements keep the selected result at its index. `ResultDto.snippet` (content/meaning matches only) replaces the location line; location in the tooltip. 68 UI tests; Linux smoke shows passages under semantic rows.

## 2026-10-09 — T209 code results and local actions (codex)

- Continued clean on-disk `main` at T206 (`1859f1e`); no task was claimed, so claimed the next implementation task T209. Preserved providers, file entity IDs, weights 1/1/2, keyboard selection and the ids-only action boundary.
- Migration 0004 adds derived code name/folder/language FTS context and repository metadata; the background content pass backfills it without re-extraction/re-embedding. Rename triggers clear stale repository/language immediately. Bounded `.git` discovery supports worktrees. Schema migration runs on a startup worker; first show waits for schema readiness.
- Typed boxed code target, display-only wire projection, symbol/file rows, capability-based Copy symbol / Reveal repository, matching-passage Quick Look; fusion retains code context/actions when the name copy wins. No guessed editor CLI, raw-offset line conversion, new parser, watcher or design tokens. ADR-036.
- Windows release hard set (162 documents/177 chunks/49 queries): content code top-1 0.50→1.00 and NDCG 0.617→0.902 (six code queries). Real q4/6 threads: fused NDCG 0.964, top-1 0.959; content p95 2.32 ms, semantic p95 65.67 ms (first-load max 1.8 s). Evidence in `docs/benchmarks/t209/2026-10-09-joao-pc/`. Synthetic 100k upgrade retained all vectors/sequences: migration 1.10 s, background backfill 1.60 s.
- Tests cover migration/backfill/moves, content/semantic code targets, authorized actions, fusion ties, wire privacy, matching-passage preview and accessible rows. Windows gate prerequisites: isolate colliding usage-test temp paths, close an mmap before the missing-file test deletes it, remove two unused Windows qualifications. Browser QA checked light/dark, 760/800 px, long names, unnamed code, Action Panel and preview; temporary fixture/server removed.
- T209 → REVIEW pending native default-handler/clipboard/Explorer checks; T207 is next implementation. Updated task/state/specs, condensed PROJECT_STATE and rewrote HANDOFF with exact continuation and all outstanding Windows reviews. Final gate commands/results are recorded in HANDOFF; 71 frontend tests.
- Final T209 check: an already-open Quick Look now refreshes on same-file code/passage enrichment (including exact filename rows with identical labels) and drops late answers (71 frontend tests); unchanged catalog metadata avoids redundant chunk-context updates. PROJECT_STATE is 178 lines.

## 2026-10-09 — T207 incremental file indexing (codex)

- Continued clean on-disk `main` at `a68d4d5` and claimed T207, the ordered next implementation. Kept the user's resident release process and app-data database untouched; all validation writes used temporary synthetic data. Preserved architecture, schema v4, extractor/model generations, provider weights, UI contracts and existing tests.
- Added shell-independent native watching (`notify 8.2.0`): 4,096 coalesced hints, 300 ms quiet / 2 s ordinary storm deadline, 5 s rate-limited loss recovery, parent/root registration and periodic fallback. Known hints survive recovery and run before the full inventory. Events preempt embedding at existing batch boundaries; inventories finish while new hints accumulate.
- Same-writer scoped reconciliation probes ordinary edits, scans new/moved subtrees and pages only scoped deletions. Offline/unverified roots and cancellation retain entries. Stable moves preserve IDs/chunks/vectors; hard links remain distinct and writes invalidate their aliases. Replacement/creation identity checks, known same-size/same-mtime writes, ancestor exclusions, marker changes and junction safety are covered. Case-only Windows moves verify actual spelling, including folder descendants.
- Native evidence exposed generic modify hints accompanying Windows renames: bounded extracted-chunk comparison within content consent preserves unchanged embeddings while rejecting rename+edit ambiguity. Code context refreshes via T209. Visible catalog/content/vector commits refresh the existing query; hidden commits do not wake JS search/query inference and the next show refreshes normally. ADR-037 and the T207 spec document contracts/limits.
- Windows release native probe: 10,001 synthetic catalog items, 20 mutations, lexical freshness 361/381 ms p50/p95 including debounce; reconciliation/content 49/70 ms. 29 entries emitted, zero full inventories, all five rename vectors retained. Two seconds parked: zero notifications and 0.0 s measured CPU (short/coarse sample). Synthetic vectors check preservation/queue state, not model quality. Evidence in `docs/benchmarks/t207/`.
- Full Rust format/lint/workspace+shell tests, DirectML lint and architecture guard passed. Frontend format/lint/types/build passed; 71 tests passed with one worker after parallel worker startup timed out under compilation/indexing load. No frontend configuration change. Rechecked late recovery/hidden-notification changes and junction/case-folder tests. Separate optimized task output: `target/t207-build/release/lumen.exe`; the original resident exe was not replaced.
- T207 → REVIEW for native visible query/selection/actions/preview, location edits and disconnected-volume checks. TASKS/PROJECT_STATE/specs updated (state 180 lines), HANDOFF rewritten with exact launch/reproduction and next task T208. No push or model download requested/performed.

## 2026-10-09 — T014 bounded GTX recheck / T212 optional GPU indexing (codex)

- Continued clean on-disk main at 69d741f. The user explicitly requested dedicated-GPU indexing with CPU queries and waived VRAM caps; claimed new T212 before implementation. Kept resident T207 PID 11356/start 15:59:27 and its live DB untouched. The broader T014 thread/llama.cpp matrix remains REVIEW; cached q4 recheck completed without downloads: CPU 2.71 vs GTX 1650 6.07 chunks/s, 2.24×, stable cosine 0.99999946, 94.43% offload, sampled peak 2296 MiB. Loaded machine, not idle evidence or ETA.
- Added off-by-default persisted native tray option, DXGI/D3D12 non-UMA discovery, explicit adapter, bounded synthetic child mode before Tauri/SQLite, model/runtime/driver/companion-DLL cache identity, quarantine and same-generation CPU fallback. Queries always use CPU; preserved 256d q4 weights/prompts, queue batching/preemption, single index writer, schema/ranking/UI contracts. ADR-038 explicitly allows available dedicated VRAM on AC while retaining ordinary battery/memory policy and Balanced CPU fallback. Probes defer while paused/on battery/unknown power/low memory, using existing queue retries.
- Optional pinned DirectML 1.24.4 runtime reuses T210 consent/resume/SHA-256/atomic install. One ORT library per process: runtime choice is pinned, installation asks for restart, enabled preference selects installed DirectML next launch, environment overrides retain precedence. Downloaded one fixed 25,111,930-byte runtime wheel for development hash/member verification; actual installer was validated from that local mirror without network. No model download, cloud inference, telemetry or Python runtime.
- Windows release real-backend synthetic check: native GTX adapter 0 (DXGI 3935 MiB), CPU prefix 2 + GPU bulk 16 + tail 8; same space/generation, original vectors preserved, 0 pending and 0 re-embedding on returning to CPU. Bulk GPU 6.80 chunks/s; actual CPU QueryEmbedder p50/p95 53.68/64.70 ms with queue preemption. Warm synthetic compatibility probe 3.84 CPU / 9.11 GPU chunks/s (2.37×), stable min cosine 0.99999982, 94.43% offload. An untimed document batch avoids first-use GPU shape setup distorting the bulk-speed gate. Context records the resident process and after-run CPU only; it does not measure per-run CPU delta/share.
- Delivered GUI exe's isolated probe with its beside-exe DLLs also passed: 3.81 CPU / 9.04 GPU chunks/s, cosine/offload gates satisfied, CPU query p95 56 ms. Malformed JSON and integrated adapter exit 1 before Tauri/DB; resident PID/start unchanged. Per-process DXGI usage ~906 MiB sampled after short 128-token inference is not a whole-run peak or comparable to T014's different corpus. Evidence: docs/benchmarks/t212/2026-10-09-joao-pc/; no native GPU reset/device-loss injection, visible tray QA or long driver soak claimed.
- Full Rust fmt/lint/workspace+shell tests, DirectML lint and architecture guard passed; latest changes rechecked with workspace lint and desktop/content/provision tests (34 shell, 14 content; pinned-wheel local test passed). Added policy/cache/resource tests and lost-device same-generation recovery checks. Frontend format/lint/types/build and 71 tests (one worker) passed. Optimized desktop/example build passed; usable target/t212-release/lumen.exe ships verified DirectML DLLs and license notices without replacing the active exe.
- T212 → REVIEW for native tray/keyboard/persistence/download/restart and longer GPU-pressure checks. TASKS/PROJECT_STATE/specs/ADRs updated (state remains 180 lines); HANDOFF rewritten with exact cached-model launch, retained Windows reviews and next implementation T208. No push performed.
- Post-commit user continuation: read-only OS/settings checks found new delivered-app PID 8884/start 17:29:01, replacing the old instance independently. GPU preference is saved true but no cached probe/quarantine; native module inspection shows the CPU development override still loaded. Pause=false, AC/100% battery. HANDOFF now records the exact current state and user-chosen restart with the bundled DirectML override. Agent did not stop either app or modify live settings/data.

### 2026-10-09 — codex — T208 query syntax while resident indexing continues

- Followed the canonical on-disk reading order, inspected clean main at cc416de and claimed the next ordered T208. Resident PID 7404/start 18:04:57 stayed running from target/t212-release/lumen.exe with beside-exe DirectML loaded. No agent restart, settings changes or live app-data DB writes; validation used synthetic temporary stores.
- Added a deterministic shell-independent parser for type/ext/in/before/after, strict Gregorian ISO mtime/UTC date boundaries, directory-component matching, parameterized hard AND constraints before lexical LIMIT and bounded filter-only inventory. Unknown operator-like text/URLs/drive letters stay literal; recognized invalid/incomplete operators fail closed. Completed quotes require lexical token phrases and disable semantic/two-of expansion. Category filters describe inventory and add no vision/OCR/PDF extraction.
- All three providers honor constraints, including learned choices and current canonical metadata checks; embeddings receive only remaining text. Filtered ANN progressively overfetches at most 1,024 candidates with cancellation/100 ms extra retrieval budget; no full vector scan, schema/generation/model/fusion-weight/UI/DTO changes. Usage keys omit operators; app/folder/filter-only/quoted requests avoid pointless model work.
- Added parser and real catalog/content tests for combined filters, date/null boundaries, metadata-only requests, quotes, learned exclusions and pre-LIMIT filtering; expanded semantic tests for embedding equivalence and narrow-filter recovery through both exact delta and persisted ANN. Existing relevance, actions and progressive selection tests pass. Full Rust workspace + 34 shell tests, workspace clippy, fmt and 14-crate architecture guard passed; frontend format/lint/types, 71 tests and build passed.
- Windows optimized synthetic 100k-item/chunk benchmark: ordinary/filtered names p95 13.429/13.086 ms; filtered content 0.372 ms, quoted content 1.343 ms, metadata-only 0.300 ms, 1,024 ANN-ID filter 1.678 ms; zero empty measured requests. Parser p95 0.001136 ms. Loaded-machine selective queries, not visible paint/common-term worst-case/full semantic inference; evidence and reproduction in docs/benchmarks/t208/2026-10-09-joao-pc/.
- Latest optimized desktop build passed; target/t208-release/lumen.exe contains T208/T212 and copied hash-verified DirectML assets/notices, separate from the live binary. Updated specs/ADR-022 amendment/task/state docs (state remains 180 lines), rewrote HANDOFF with exact launch and native review, and moved T208 to REVIEW. Native root/keyboard/filter/action inspection remains pending; next ordered implementation T301. No push performed.
