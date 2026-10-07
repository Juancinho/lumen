# PROJECT_STATE.md

> Canonical short snapshot. Keep this under ~180 lines.

## Current milestone

**M0 — Technical spikes and repository foundation**

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
- Not yet: real inference runtime (T006), provider trait/registry, storage, ANN, design tokens/
  material (T004/T103).

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

- inference integration maturity;
- WebView lifecycle/RAM while resident;
- media/PDF extraction licensing/packaging;
- background indexing resource spikes;
- provider ranking complexity;
- semantic reranking focus instability;
- NTFS/non-NTFS identity edge cases;
- scope creep from launcher/productivity features;
- overengineering extension SDK too early.
