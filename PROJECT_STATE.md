# PROJECT_STATE.md

> Canonical short snapshot. Keep this under ~180 lines.

## Current milestone

**M1 — Instant launcher and universal surface** (M0 gate met 2026-10-08: overlay, boundary,
contracts, cold/warm measurements T012, runtime ADR-015, ANN ADR-016, schema ADR-017, CI T010)

## Product status

- Working codename: **Lumen**
- Target: Windows 11 first; graceful Windows 10 fallback where practical
- Product identity: **semantic command center**, not only file search
- Invocation: configurable global shortcut, keyboard-first overlay
- Core pillars: Find / Act / Remember / Extend
- Privacy: local-first; no cloud required for core features
- Model: EmbeddingGemma 2 behind interchangeable backend
- Default embedding target: 256d normalized; scalar type selected by benchmark

## Product scope accepted

Core destination includes:

- universal root search;
- exact filename/path/app search;
- FTS + semantic/multimodal retrieval;
- universal contextual actions;
- calculator/system/quicklink/snippet/clipboard providers;
- workflows;
- Semantic Drop + Find Similar;
- Smart Collections;
- Semantic Workspaces;
- temporal recent-work search;
- optional Rewind / Context Lens later;
- audio/video later;
- public extension SDK only after internal APIs stabilize.

## Architecture status

Accepted:

- Tauri 2 + React/TypeScript presentation shell
- Rust domain/core
- shell-agnostic core boundary
- internal `Provider`, `ResultItem`, `Action`, `Workflow` concepts
- SQLite WAL + FTS5
- USearch/HNSW
- progressive lexical/provider results → semantic refinement
- resident warm path where memory profile allows
- multi-pass, resumable, value-prioritized indexing
- one WebView target

Decided with evidence: runtime ONNX Runtime CPU q4 (ADR-015), f16 HNSW (ADR-016), device
policy (ADR-019), window material (ADR-024). Still open: faster indexing runtime (T014),
optional FastFrame/egui shell spike (TX01) only if M1 measurements miss targets.

## Implementation status

- **T001 DONE:** Cargo workspace + Tauri 2 shell + React/TS/Vite frontend; full gate passes on
  Linux and Windows (`target/release/lumen.exe` verified by the user).
- **T011 DONE:** universal command contracts in `crates/lumen-core` — ids, `ResultItem`,
  `CapabilitySet`, `ActionDescriptor` (risk class + panel group), `validate_result`,
  `ExecutionContext::authorize`, `CancellationToken`. ADR-013: shell owns wire DTOs.
- Crates: `crates/lumen-core`, `apps/desktop/src-tauri` (`lumen-desktop`, bin `lumen`), `xtask`.
- Shell → core direction enforced by `cargo xtask arch` and ESLint `no-restricted-imports`.
- Commands/layout: `docs/DEVELOPMENT.md`. Contract summary: `docs/COMMAND_MODEL.md` §0.
- **T002 DONE:** resident overlay — hidden borderless window, Alt+Space toggle, Escape/blur/
  Alt+F4 hide, cursor-monitor placement, tray (Show/Quit), single instance, focus-on-show.
  Verified on Windows by the user.
- **T005 DONE:** `crates/lumen-embedding` (sync `EmbeddingBackend`, `Embedder` with prompts,
  batching, cancellation, 768→256 + L2, `EmbeddingSpace` key, deterministic `MockBackend`) and
  `crates/lumen-bench` (`lumen-bench embed` JSON reports vs 60/120 ms budget). ADR-014.
- **T006 DONE (ADR-015):** EmbeddingGemma 2 on ONNX Runtime (`crates/lumen-embedding-ort`,
  dynamic `onnxruntime.dll`), default **CPU + q4**: 30 ms p50 query, 168 MiB, cos 0.98 vs fp32.
  DirectML measured and rejected as default (slower than CPU, unstable fp16/iGPU). Indexing
  throughput on CPU (~3–4 chunks/s) is the top risk → T013/T014 added.
- **T008 DONE (ADR-016):** `crates/lumen-vector` (USearch HNSW wrapper). f16 storage, cosine,
  M=16, ef_search=256: recall@10 1.000 at 100k / 0.988 at 1M, ≤2 ms queries, ~0.8 KB/vector.
  i8 rejected (recall 0.85). Read path via mmap; T203 adds a mutable delta.
- **T007 DONE (ADR-017):** `crates/lumen-storage` — bundled SQLite 3.53 + FTS5, WAL with
  one writer/N readers, user_version migrations (0001: items, chunks, chunks_fts, settings,
  usage_events), safe FTS query builder, budgeted/cancellable interactive search.
- **T009 DONE (ADR-018):** `crates/lumen-indexer` — Pass 0 inventory with a coverage
  guarantee (every entry emitted, excluded by a visible rule, or reported as an issue), links
  never followed, cloud placeholders metadata-only, stable identity (volume + file id).
  Windows: 26.5k user entries = independent .NET count, 0 issues, 22k entries/s cold
  (7.3k/s with identity); all edge cases reported. Also fixed usearch MSVC link (ADR-016).
- **T010 DONE:** GitHub Actions CI (frontend; Rust gate on Linux + Windows via
  `cargo xtask test`; quick release benches as artifacts) + `cargo xtask bench [--quick]`.
  Green on github.com/Juancinho/lumen.
- **T012 DONE (ADR-020):** WebView hidden-state modes; default trims to low memory after 30 s
  hidden: 72 → 7 MiB private WS, show→paint 22.6 ms p50 (27 ms after a trim), UI ready in
  ~0.4–0.5 s. Opt-in timing diagnostics (`LUMEN_DIAG_LOG`), `lumen.exe --show/--hide/--toggle/--quit`.
- **T013 DONE (ADR-019):** device policy — CPU default/fallback, accelerators only with a
  passing probe (same space, stable, cos ≥ 0.999 vs CPU, ≥ 90 % offloaded, memory budget,
  no iGPU), lane rules by power/profile/activity, quarantine per runtime key. joao-pc: GTX
  1650 indexes 2.35x faster but needs 2.3 GB VRAM -> rejected; CPU in every scenario.
- **T101 DONE (ADR-021):** `lumen-core::provider` + `builtin` actions; `lumen-catalog`
  (inventory → items with move detection and safe removal, Start-menu apps via
  `lumen-windows` AppsFolder, `CatalogProvider` exact/prefix, accent-insensitive); 245k
  entries synced in 6 s (sandbox); Windows: 26.5k entries + 330 AppsFolder apps, keystroke p95 5.3 ms.
- **T102 DONE (ADR-022):** code-aware name/folder tokens in FTS5 + Rust scoring (exact,
  stem, prefix, token-prefix, initials, folder+name, typos, priors); relevance set MRR@10
  1.000 over 40 queries; keystroke p95 7.9 ms at 247k entries (bounded best-effort stages).
- **T106 DONE (ADR-023):** local usage store (aggregates only): decayed frecency, learned
  query→item choices, pins, retention/clear; bounded ranking priors; empty-query suggestions.
- **T003 REVIEW:** configurable shortcut (tray submenu, 4 choices, "(in use)" probing,
  persisted in the app-data SQLite settings, first-free fallback only when nothing is saved).
- **T004 REVIEW (ADR-024 proposed):** window material — transparent window + DWM system
  backdrop: Automatic = Acrylic (Win11 22H2+), Mica, Solid; Solid when high contrast /
  transparency off / older Windows; native rounded corners + shadow; tinted surface tokens
  with a contrast floor enforced by tests; tray → Window material. Windows: ~22 ms
  show→paint for every material, Acrylic +3–4 % DWM GPU while visible, on-screen secondary
  contrast ≥ 5.3:1. Pending: user's visual verdict (Acrylic vs Mica default).
- **T103 REVIEW:** design tokens + premium root search (search bar, 52 px result rows with
  middle-truncated paths, no-results state, content-driven window height capped at 72 %,
  entrance fade, high-contrast/reduced-motion paths). No data source yet (T107).
- **T107 REVIEW (ADR-025):** `crates/lumen-search` (coordinator by latency class, merged
  updates, latest-wins search thread); shell `search` command + `lumen:results` events;
  background catalog sync (apps + standard folders, start-up + 30 min). Search now works in
  the app; Linux smoke 0.4–2 ms per query in-process.
- **T104 REVIEW:** keyboard model (`keymap.ts`: arrows, PageUp/Down, Enter / Ctrl+Enter /
  Alt+Enter / Ctrl+K claimed, Ctrl+L, Escape; text-editing keys and IME left alone) and a
  selection that follows its result id while results stream (`selection.ts`).
- **T108/T109 REVIEW (ADR-026):** Enter opens/launches, Ctrl+Enter reveals, click runs,
  Ctrl+K Action Panel (Open / Reveal in Explorer / Copy path); ids-only requests checked by
  the core policy against recently shown results; uses recorded for ranking.
- **T015/T016 DONE:** docs consolidated (ADR files in `docs/adr/`, §0 status per spec, one
  reading order, fixed file roles, throughput + memory budgets); storage bench with real hits.
- **T201 REVIEW (ADR-028):** `crates/lumen-extract` — kinds by extension, bounded decoding
  (BOM/UTF-8/UTF-16/Windows-1252, binary and size skips with reasons), 128-token chunks with
  offsets for prose/Markdown/code/data, symbol names for code; estimator calibrated against
  the EmbeddingGemma 2 tokenizer; `lumen-bench chunk`.
- **T202 REVIEW (ADR-029):** `crates/lumen-content` — incremental content pass and a
  persistent, pausable, throttled embedding queue whose state is the database (vectors per
  generation as f16 in `chunk_vectors`), run by the catalog thread under the device policy
  (power/memory/idle); tray progress + pause, per-location content toggle. Model via env
  until T210.
- **T014 REVIEW (Windows run pending):** throughput harness for ORT thread sweeps and
  llama.cpp builds (CPU/Vulkan/CUDA, GGUF) with CPU share per run; sandbox ORT q4 3.4
  chunks/s per busy core. The runtime/thread verdict becomes an ADR after the joao-pc run.
- **T111 REVIEW (ADR-027):** indexed locations + exclusions in one versioned setting; tray
  → Indexed locations / Exclusions (native folder picker), per-location state, developer
  noise and marker-based build folders excluded by default (visible, toggleable), Action
  Panel "Exclude folder from Lumen"; edits restart the pass. Sandbox whole-FS: 309k entries,
  keystroke p95 7.1 ms.
- **T110 REVIEW:** `LUMEN_DIAGNOSTICS=1` shows provider/match kind/confidence per row and
  logs per-query timings; absent from the wire otherwise.
- **T105 REVIEW:** Quick Look (Alt+Enter): window widens right, preview beside the list
  (or over it on narrow monitors) with metadata and a bounded text excerpt (16 KB read,
  4,000 chars, text extensions only, binaries rejected).
- Not yet: real icons, rich previews (images/PDF), pin/open-with UI.

## Immediate objective

1. Close M1 on Windows: the REVIEW checklists in `HANDOFF.md` (search, keys, actions, Quick
   Look, look and material, shortcut).
2. Start M2 (text/code semantic search) with T201, and settle the indexing runtime (T014) —
   ordered in `TASKS.md` → **Next**.

## M1 gate (instant launcher)

- type → name/app results every keystroke within budget ✔ (Windows: p95 5.3 ms provider);
- Enter/Ctrl+Enter/Ctrl+K actions, Quick Look, stable keyboard selection ✔ (REVIEW);
- premium surface + native material ✔ measured, visual verdict pending;
- catalog kept current without user action ✔ (sync at start-up + 30 min; watcher is T207).

## Top risks

- Content FTS per keystroke: 13/68 ms p50/p95 at 100k chunks with real hits (T016) — run it
  on the settled query, not every keystroke (ADR-017 note);

- CPU embedding throughput for initial indexing (~3–4 chunks/s measured, ADR-015);

- inference integration maturity;
- WebView lifecycle/RAM while resident;
- media/PDF extraction licensing/packaging;
- background indexing resource spikes;
- provider ranking complexity;
- semantic reranking focus instability;
- NTFS/non-NTFS identity edge cases (hard links, save-by-replace, id reuse: ADR-018);
- scope creep from launcher/productivity features;
- overengineering extension SDK too early.
