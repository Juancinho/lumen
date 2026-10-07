# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main` (git initialized this session; two commits: spec-pack baseline, then T001).

## Active task

**T001 — REVIEW (owner: claude).** Implementation complete and validated on Linux. Only remaining
item: verify the Windows build on real hardware, then set T001 to `DONE` in `TASKS.md`.

T001's output (the workspace) exists, so T002/T005/T007/T008/T010/T011/T012 can start now; do the
Windows verification below first if you are on Windows.

## Implemented behavior

- Cargo workspace (`Cargo.toml`, resolver 3, edition 2024, shared deps/lints/profiles), toolchain
  pinned to Rust 1.97.0 (`rust-toolchain.toml`).
- `crates/lumen-core`: shell-agnostic core; currently only `CoreInfo`/`core_info()` (product name +
  version). `#![forbid(unsafe_code)]`, zero dependencies.
- `apps/desktop/src-tauri` (`lumen-desktop`, bin `lumen`): Tauri 2.12 shell; async command
  `core_info` returning `CoreInfoDto` (camelCase, JSON shape tested); strict CSP; capability
  `core:default` only; `withGlobalTauri: false`; installer bundling disabled (T807).
- `apps/desktop/src`: React 19 placeholder (`app/App.tsx`) showing core version via typed
  `src/ipc` wrapper; loading = `role="status"`, failure = `role="alert"`.
- `xtask`: `cargo xtask arch` fails if any member under `crates/` depends (transitively, any kind,
  any platform, or merely declared) on Tauri/WebView/GUI-toolkit crates or on anything under `apps/`.
- ESLint: only `src/ipc/**` may import `@tauri-apps/*`.

## Files changed

New: `.cargo/config.toml`, `.editorconfig`, `.gitattributes`, `.gitignore`,
`.vscode/extensions.json`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`,
`clippy.toml`, `crates/lumen-core/**`, `xtask/**`, `apps/desktop/**` (package.json + lockfile,
eslint/prettier/tsconfig/vite configs, `index.html`, `src/**`, `src-tauri/**` incl. placeholder
icons), `docs/DEVELOPMENT.md`.
Updated: `TASKS.md`, `PROJECT_STATE.md`, `README.md` (repo map), `WORKLOG.md`, this file.

## Validation (run in Linux sandbox, toolchain 1.97.0, Node 22)

All passed:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # core 2, shell 2, xtask 8
cargo xtask arch                  # OK - lumen-core
cd apps/desktop && npm run check  # prettier, eslint, tsc -b, vitest (3 tests)
cd apps/desktop && npx tauri build --debug --no-bundle   # then ran under Xvfb: "Core 0.1.0" rendered
cargo build --profile profiling -p lumen-desktop --features tauri/custom-protocol
```

Negative checks performed (then reverted): `tauri` added to `lumen-core` → `cargo xtask arch` exit 1;
dev-dependency on `apps/desktop/src-tauri` → exit 1; `@tauri-apps/api/core` imported from
`src/app/` → ESLint error.

## Exact next steps

1. On Windows 11 (PowerShell, repo root):
   `cd apps/desktop; npm ci; npm run check; npm run tauri build` → expect `target\release\lumen.exe`
   opening a window that shows "Core 0.1.0". Also run `cargo test --workspace` and `cargo xtask arch`.
   If all pass, set T001 to `DONE`.
2. Take T011 (domain contracts) or T002 (overlay). T011 must decide whether core types derive serde
   and whether TS wire types are generated (e.g. ts-rs/specta) or stay hand-written with Rust
   shape tests (current pattern in `src-tauri/src/dto.rs`).
3. T010 should wire the local gate above into CI (Linux needs `libwebkit2gtk-4.1-dev librsvg2-dev
   libxdo-dev libssl-dev`; a Windows runner is needed for real shell checks).

## Known issues / notes

- Plain `cargo build`/`cargo run` of `lumen-desktop` (without `tauri/custom-protocol`) loads the dev
  URL `http://localhost:1420`; use `npm run tauri dev|build`. Documented in `docs/DEVELOPMENT.md`.
- ESLint pinned to 9.x (jsx-a11y lacks ESLint 10 support); TypeScript 6.0.x (typescript-eslint `<6.1`).
- Icons are placeholders generated with `tauri icon`; branding is undecided.
- Repo was committed from the Cowork Linux VM with `core.fileMode=false`. If Windows git reports
  "dubious ownership", run `git config --global --add safe.directory D:/Proyectos/lumen`.

## Unresolved evidence-based decisions

- production EmbeddingGemma runtime (T006);
- exact native backdrop path (T004);
- vector scalar profile (T008);
- FastFrame/egui comparative shell spike timing (TX01; not before Tauri baseline);
- domain type serialization / TS binding generation (T011).
