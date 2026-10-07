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
