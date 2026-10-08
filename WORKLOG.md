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
