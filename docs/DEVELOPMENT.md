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
    src/ids.rs             ProviderId / ActionId / ResultId / QueryId
    src/result.rs          ResultItem, ResultKind, IconRef, Score, Payload
    src/capability.rs      CapabilitySet (what a result's target supports)
    src/action.rs          ActionDescriptor, ActionSafety, ActionGroup, ActionLookup
    src/contract.rs        validate_result — run it in every provider test
    src/execution.rs       ActionRequest -> ExecutionContext::authorize, CancellationToken
  lumen-embedding/         EmbeddingBackend trait (sync), Embedder (prompts, batching,
                           cancellation, 768->256 truncation + L2), EmbeddingSpace key, MockBackend
  lumen-embedding-ort/     EmbeddingGemma 2 on ONNX Runtime (ADR-015): dynamic onnxruntime.dll,
                           CPU (default) / DirectML (feature `directml`), placement diagnostics
  lumen-storage/           SQLite (ADR-017): migrations/, WAL writer+readers, FTS5, SearchBudget
  lumen-vector/            ANN index (USearch HNSW, ADR-016): f16, cosine, add/search/remove/save/view
  lumen-bench/             benchmark harness binary `lumen-bench` (release-mode, JSON reports)
apps/desktop/              presentation shell (Tauri 2 + React/TS + Vite)
  src/                     React UI
    app/                   overlay root (App.tsx) + placeholder styles
    features/root-search/  SearchField (T002 minimal; premium surface is T103)
    ipc/                   ONLY place allowed to import @tauri-apps/* (typed wrappers + wire types)
    test/                  Vitest setup
  src-tauri/               Rust shell crate `lumen-desktop` (binary `lumen`)
    src/commands/          Tauri commands, one module per feature area
    src/overlay/           overlay window lifecycle; placement.rs + policy.rs are pure/tested
    src/shortcut.rs        global shortcut (fixed Alt+Space until T003)
    src/tray.rs            tray icon + menu (Show / Quit)
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
  `src/ipc/types.ts`, with a Rust test guarding the JSON shape. Core types never derive serde
  (ADR-013); `Payload` and provider confidence are never sent to the UI.

Provider/result conventions (T011, see `docs/COMMAND_MODEL.md` §0):

- built-in ids are `const`s via `ProviderId::from_static` / `ActionId::from_static` (invalid names
  fail compilation); `lumen.` is reserved for built-ins;
- every provider test asserts `validate_result(&item, &actions).is_empty()` for each result;
- actions run only through `ExecutionContext::authorize`; the UI sends ids, never paths.

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

Benchmarks (release builds only; debug runs are flagged in the report):

| Purpose | Command |
|---|---|
| Embedding backend latency/throughput/memory | `cargo run --release -p lumen-bench -- embed --json target/bench/embed-<backend>.json` |
| ANN index build/recall/latency/persistence | `cargo run --release -p lumen-bench -- ann --sizes 100000 --json target/bench/ann.json` |
| SQLite/FTS5 insert + per-keystroke latency | `cargo run --release -p lumen-bench -- storage --json target/bench/storage.json` |
| Options | `cargo run --release -p lumen-bench -- --help` (`--backend`, `--dim`, `--iterations`, `--batch-sizes`, `--label`, …) |

Real model (ADR-015): `cargo run --release -p lumen-bench --features ort -- embed --backend ort
--ort-dylib <onnxruntime.dll> --model-dir <embeddinggemma-2-ONNX copy> --variant q4
--reference fixtures/embedding/reference-eg2-onnx-fp32-d256.json`. Full Windows matrix:
`scripts/t006/run-windows-bench.ps1` (see `docs/benchmarks/t006/README.md`). Fidelity test:
`LUMEN_EG2_MODEL_DIR=… LUMEN_ORT_DYLIB=… cargo test -p lumen-embedding-ort --release --test fidelity`
(skipped when unset).

Reports carry `schema_version`, machine/build metadata, the `EmbeddingSpace` key, cold load,
first and warm query latency (p50/p95/p99 vs the 60/120 ms budget), per-batch-size document
throughput and resident memory before/after load. Committed baselines belong under
`docs/benchmarks/` with the hardware described in `--label`.

Full local gate before committing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask arch
cd apps/desktop && npm run check
```

CI wiring of this gate and the release-mode benchmark command are T010.

## 5. Overlay runtime behaviour (T002)

- Single resident process (`tauri-plugin-single-instance`; a second launch shows the running
  overlay). The overlay window is created hidden, never destroyed.
- First show waits for the UI's `overlay_ready` call (no blank first frame). `lumen --background`
  starts resident in the tray without showing.
- `Alt+Space` toggles (show → focus if visible but unfocused → hide). If registration fails
  (another launcher owns it) Lumen keeps running; the tray tooltip says the shortcut is unavailable.
- Placement: monitor under the cursor, horizontally centered, top edge at 20% of the work area,
  clamped inside it; logical size 800×64 (`overlay::LOGICAL_SIZE` = `tauri.conf.json`).
- Dismiss: Escape (ignored during IME composition), focus loss, Alt+F4. Quit: tray → Quit Lumen.
- On every show the shell emits `lumen:overlay-shown`; the UI focuses and selects the query.
- Linux dev note: WebKitGTK enforces a ~200px minimum window height and single-instance needs a
  D-Bus session; both are Linux-only artefacts.

## 6. Build profiles

- `release`: LTO, `codegen-units = 1`, stripped. Use for all performance evidence
  (docs/PERFORMANCE.md: dev timings are not acceptance evidence).
- `profiling`: release optimisations with debug symbols, for flamegraphs/WPR:
  `cd apps/desktop && npm run build && cd ../.. && cargo build --profile profiling -p lumen-desktop --features tauri/custom-protocol`
  → `target/profiling/lumen(.exe)`.

Note: a plain `cargo build`/`cargo run` of `lumen-desktop` (without `tauri/custom-protocol`, which
the Tauri CLI adds for `tauri build`) produces a binary that loads the **dev server URL**
(`http://localhost:1420`). Use `npm run tauri dev|build` unless you pass that feature yourself.

Installer bundling is disabled (`bundle.active = false`); packaging/signing is T807.

## 7. Security baseline

- Strict CSP in `tauri.conf.json` (`default-src 'self'`, no inline scripts/styles in production;
  dev CSP only adds Vite HMR websocket + inline styles).
- `withGlobalTauri: false`; IPC only through `@tauri-apps/api` inside `src/ipc/`.
- Capability set is `core:default` for the `main` window. Adding a permission requires a task
  reason in the capability file description or commit message.

## 8. Toolchain policy

- Rust toolchain is pinned; bump it in a dedicated commit after `cargo clippy` is clean.
- npm dependencies are pinned exactly (`--save-exact`). ESLint is held at 9.x because
  `eslint-plugin-jsx-a11y` does not yet support ESLint 10; TypeScript at 6.0.x because
  `typescript-eslint` supports `<6.1`.
- Tauri stays on 2.x (ADR-002). Tauri 3 is pre-release; do not upgrade without an ADR.
