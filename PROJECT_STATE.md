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

- **T001 (REVIEW):** Cargo workspace + Tauri 2 shell + React/TS/Vite frontend exist and pass the
  full local gate on Linux. Windows `npm run tauri build` not yet run on real hardware.
- Crates: `crates/lumen-core` (domain, shell-agnostic), `apps/desktop/src-tauri` (`lumen-desktop`,
  binary `lumen`), `xtask` (repo tooling).
- Shell → core direction enforced by `cargo xtask arch` (Rust) and ESLint `no-restricted-imports`
  (only `src/ipc/` may import `@tauri-apps/*`).
- Commands, layout and toolchain policy: `docs/DEVELOPMENT.md`.
- No domain contracts (T011), overlay (T002), storage, ANN or embedding code yet.

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
