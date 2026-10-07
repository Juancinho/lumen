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
