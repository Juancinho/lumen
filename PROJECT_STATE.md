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

Benchmark/ADR required:

- Windows EmbeddingGemma runtime
- f16/f32/i8 vector profile
- Mica/Acrylic implementation
- optional FastFrame/egui shell spike after baseline exists; no migration by intuition

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
- Not yet: provider trait/registry, design tokens/material (T004/T103).

## Immediate objective

Complete M0 without overbuilding:

1. workspace baseline;
2. hotkey → overlay → focus/hide;
3. shell-agnostic core contracts;
4. embedding runtime benchmark;
5. ANN benchmark;
6. SQLite/FTS proof;
7. file identity/enumeration proof;
8. performance baseline.

## Success gate to M1

- overlay reliable and focus-correct;
- shell/domain dependency boundary enforced;
- universal result/action contracts compiled/tested;
- cold/warm measurements captured;
- embedding runtime direction selected;
- ANN target validated;
- schema migration baseline;
- CI baseline.

## Top risks

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
