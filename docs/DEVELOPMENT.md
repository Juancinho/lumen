# DEVELOPMENT.md — workspace, commands and boundaries

Established by T001. Keep this file accurate when commands or layout change.

## 1. Prerequisites (Windows 11, primary target)

- **Rust** via [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml`
  (currently `1.97.0` + rustfmt + clippy); rustup installs it automatically on first `cargo` call.
- **MSVC build tools**: Visual Studio Build Tools with the "Desktop development with C++" workload.
- **WebView2 runtime**: preinstalled on Windows 11 (Evergreen).
- **Node.js** `^20.19.0 || >=22.12.0` with npm (npm is the package manager; lockfile is
  `apps/desktop/package-lock.json`).

Linux (CI/agents) additionally needs the Tauri system packages, e.g. on Ubuntu 24.04:
`libwebkit2gtk-4.1-dev librsvg2-dev libxdo-dev libssl-dev`.

## 2. Layout

```text
Cargo.toml                 Cargo workspace (shared versions, lints, profiles)
rust-toolchain.toml        pinned toolchain
.cargo/config.toml         `cargo xtask` alias
crates/
  lumen-core/              domain core — shell-agnostic, no Tauri/React/WebView deps
apps/desktop/              presentation shell (Tauri 2 + React/TS + Vite)
  src/                     React UI
    app/                   root component + placeholder styles
    ipc/                   ONLY place allowed to import @tauri-apps/* (typed wrappers + wire types)
    test/                  Vitest setup
  src-tauri/               Rust shell crate `lumen-desktop` (binary `lumen`)
    src/commands/          Tauri commands, one module per feature area
    src/dto.rs             wire DTOs mapped from core types
    capabilities/          Tauri permission sets (minimal: core:default)
    tauri.conf.json        window, CSP, build hooks
xtask/                     repo tooling (`cargo xtask arch`)
```

New domain crates (`lumen-storage`, `lumen-search`, `lumen-embedding`, …) go under `crates/`
and are created by the task that needs them, not ahead of time. Add each to
`[workspace] members` in the root `Cargo.toml`.

## 3. Dependency direction (ADR-002) — enforced

```text
apps/desktop (Tauri + React)  ──depends on──►  crates/*  (never the reverse)
```

Two automated guards:

1. **Rust — `cargo xtask arch`.** Reads `cargo metadata` (all platforms, all dependency kinds) and
   fails if any workspace member under `crates/` depends, directly or transitively, on a
   presentation-shell crate (`tauri`, `tauri-*`, `wry`, `tao`, `webview2-com*`, `webkit2gtk*`,
   `gtk*`, `muda`, `tray-icon`, `egui`, `eframe`, `winit`) or on anything under `apps/`. It also
   inspects declared dependencies, because cargo silently drops some invalid edges from the
   resolve graph. It refuses to pass if it finds zero core crates.
2. **TypeScript — ESLint `no-restricted-imports`.** Only `src/ipc/**` may import `@tauri-apps/*`.
   UI code imports typed functions from `src/ipc`, which keeps components native-bridge-agnostic
   and trivially mockable.

Shell conventions:

- Tauri commands are thin: call core/application code, map to a DTO, return.
- Commands are `async` so they run off the main thread. No blocking disk/DB/inference work in a
  sync command.
- Wire DTOs are explicit camelCase structs in `src-tauri/src/dto.rs`, mirrored by
  `src/ipc/types.ts`, with a Rust test guarding the JSON shape. Whether domain types gain serde
  derives (or generated TS bindings) is decided in T011.

## 4. Commands

Run Rust commands from the repository root, frontend commands from `apps/desktop/`.

| Purpose | Command |
|---|---|
| Install frontend deps | `cd apps/desktop && npm ci` |
| Run app (dev, hot reload) | `cd apps/desktop && npm run tauri dev` |
| Build app (release exe, no installer) | `cd apps/desktop && npm run tauri build` → `target/release/lumen.exe` |
| Rust format | `cargo fmt --all` / check: `cargo fmt --all -- --check` |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust tests | `cargo test --workspace` |
| Architecture check | `cargo xtask arch` |
| Frontend format | `npm run format` / check: `npm run format:check` |
| Frontend lint (type-aware, a11y strict) | `npm run lint` |
| Frontend typecheck | `npm run typecheck` |
| Frontend tests (Vitest + Testing Library) | `npm test` |
| All frontend checks | `npm run check` |

Full local gate before committing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask arch
cd apps/desktop && npm run check
```

CI wiring of this gate and the release-mode benchmark command are T010.

## 5. Build profiles

- `release`: LTO, `codegen-units = 1`, stripped. Use for all performance evidence
  (docs/PERFORMANCE.md: dev timings are not acceptance evidence).
- `profiling`: release optimisations with debug symbols, for flamegraphs/WPR:
  `cd apps/desktop && npm run build && cd ../.. && cargo build --profile profiling -p lumen-desktop --features tauri/custom-protocol`
  → `target/profiling/lumen(.exe)`.

Note: a plain `cargo build`/`cargo run` of `lumen-desktop` (without `tauri/custom-protocol`, which
the Tauri CLI adds for `tauri build`) produces a binary that loads the **dev server URL**
(`http://localhost:1420`). Use `npm run tauri dev|build` unless you pass that feature yourself.

Installer bundling is disabled (`bundle.active = false`); packaging/signing is T807.

## 6. Security baseline

- Strict CSP in `tauri.conf.json` (`default-src 'self'`, no inline scripts/styles in production;
  dev CSP only adds Vite HMR websocket + inline styles).
- `withGlobalTauri: false`; IPC only through `@tauri-apps/api` inside `src/ipc/`.
- Capability set is `core:default` for the `main` window. Adding a permission requires a task
  reason in the capability file description or commit message.

## 7. Toolchain policy

- Rust toolchain is pinned; bump it in a dedicated commit after `cargo clippy` is clean.
- npm dependencies are pinned exactly (`--save-exact`). ESLint is held at 9.x because
  `eslint-plugin-jsx-a11y` does not yet support ESLint 10; TypeScript at 6.0.x because
  `typescript-eslint` supports `<6.1`.
- Tauri stays on 2.x (ADR-002). Tauri 3 is pre-release; do not upgrade without an ADR.
