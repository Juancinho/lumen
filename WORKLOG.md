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
