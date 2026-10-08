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
.cargo/config.toml         `cargo xtask` alias; MSVC CXXFLAGS workaround for usearch (ADR-016)
.github/workflows/ci.yml   CI: frontend checks, Rust gate on Linux + Windows, quick benches (T010)
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
  lumen-indexer/           Pass 0 inventory: scan with coverage guarantee, stable FileIdentity (ADR-018)
  lumen-catalog/           app/file catalog: inventory -> items, Start-menu apps, CatalogProvider (ADR-021)
  lumen-search/            root-search Coordinator + latest-wins SearchService (ADR-025)
  lumen-extract/           text/code extraction + retrieval chunking (ADR-028)
  lumen-content/           content pass (extract -> chunks) + persistent embedding queue
                           (pause, duty cycle, interactive holds; ADR-029)
  lumen-windows/           Windows OS adapters (AppsFolder apps, window material/DWM plan,
                           process CPU time);
                           no GUI framework/WebView types
  lumen-bench/             benchmark harness binary `lumen-bench` (release-mode, JSON reports)
apps/desktop/              presentation shell (Tauri 2 + React/TS + Vite)
  src/                     React UI
    app/                   overlay root (App.tsx), appearance.ts, placeholder styles
    design/                tokens.css (T103) + material.css (T004, ADR-024); CSS-reading tests
    features/root-search/  RootSearch, SearchField, ResultList/ResultRow, icons, model (view
                           model, path split), layout (window height), useResults (T107)
    ipc/                   ONLY place allowed to import @tauri-apps/* (typed wrappers + wire types)
    test/                  Vitest setup
  src-tauri/               Rust shell crate `lumen-desktop` (binary `lumen`)
    src/commands/          Tauri commands, one module per feature area
    src/overlay/           overlay window lifecycle; placement.rs + policy.rs are pure/tested
    src/shortcut.rs        configurable global shortcut + conflict handling (T003)
    src/settings.rs        app-data SQLite settings (lumen.db, `settings` table)
    src/lifecycle.rs       hidden-WebView modes (ADR-020); src/diag.rs timing diagnostics
    src/material.rs        window material (ADR-024): apply plan, re-check before show
    src/search.rs          search thread + catalog provider -> `lumen:results` (ADR-025)
    src/catalog.rs         catalog sync over the indexed locations (start-up, edits, 30 min)
    src/indexing.rs        content pass + embedding-queue slices on the catalog thread,
                           device policy from power/memory/idle (ADR-029); model via env
    src/actions.rs         action executors behind the core policy (ADR-026)
    src/preview.rs         Quick Look data: metadata + bounded text excerpt (T105)
    src/instance.rs        second-launch commands (--show/--hide/--toggle/--quit)
    src/tray.rs            tray icon + menu (Show, Keyboard shortcut, Window material, Quit)
    src/dto.rs             wire DTOs mapped from core types
    capabilities/          Tauri permission sets (minimal: core:default)
    tauri.conf.json        window, CSP, build hooks
xtask/                     repo tooling (`cargo xtask arch|test|bench`)
docs/                      specs (each starts with §0 status); DECISIONS.md = ADR index,
                           one file per ADR in docs/adr/; benchmarks/ = JSON evidence
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
| Rust tests | `cargo xtask test` (two cargo calls; plain `cargo test --workspace` breaks doctests on Windows) |
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
| File inventory coverage/speed (counts only) | `cargo run --release -p lumen-bench -- scan --root DIR --identity --json target/bench/scan.json` |
| Stable identity semantics on a volume | `cargo run --release -p lumen-bench -- identity-check --dir DIR` |
| Catalog sync + keystroke name lookup | `cargo run --release -p lumen-bench -- catalog --root DIR [--apps] [--show QUERY]` |
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
cargo xtask test            # = cargo test, split in two calls (Windows doctest link issue, see xtask/src/test.rs)
cargo xtask arch
cd apps/desktop && npm run check
```

Release-mode benchmark suite (every model-free `lumen-bench` subcommand, one JSON per bench):

```sh
cargo xtask bench --quick          # ~1 min: CI size, reports in target/bench/quick/
cargo xtask bench                  # full size (100k ANN/storage), target/bench/full/
cargo xtask bench --out DIR        # custom output directory
```

## 4.1 Continuous integration (T010)

`.github/workflows/ci.yml` runs on every push to `main`, every pull request and on demand:

- **frontend** (Ubuntu 24.04): `npm ci`, `npm run check`, `npm run build`;
- **rust** (Ubuntu 24.04 + Windows Server 2025): builds the UI first (the shell embeds
  `apps/desktop/dist`), then `cargo fmt --check` (Linux), `cargo clippy --workspace
  --all-targets --locked -D warnings`, clippy for `lumen-bench --features directml`,
  `cargo xtask test --locked`, `cargo xtask arch`;
- **bench** (both OSes, after rust): `cargo xtask bench --quick`; reports uploaded as the
  `bench-quick-<os>` artifact (90 days). Numbers are not gating yet (PERFORMANCE.md §12): add
  thresholds only once run-to-run noise on hosted runners is known.

The Windows job exists because Linux-only checks missed a Windows link failure (usearch/MSVC,
found in T009). Model-dependent tests (`--test fidelity`, ORT benches) stay manual: they need
the ~2 GB model download (`scripts/t006/`). Validate workflow edits with
`actionlint .github/workflows/ci.yml`.

## 5. Overlay runtime behaviour (T002)

- Single resident process (`tauri-plugin-single-instance`; a second launch shows the running
  overlay). The overlay window is created hidden, never destroyed.
- First show waits for the UI's `overlay_ready` call (no blank first frame). `lumen --background`
  starts resident in the tray without showing.
- The toggle shortcut (default `Alt+Space`) shows → focuses if visible but unfocused → hides.
  Tray → "Keyboard shortcut" offers Alt+Space, Ctrl+Space, Alt+Shift+Space, Ctrl+Alt+Space;
  choices another app owns are labelled "(in use by another app)". A chosen shortcut is saved
  (`settings` key `shortcut.toggle` in `%APPDATA%\dev.lumen.desktop\lumen.db`) and never
  silently replaced; with nothing saved and the default taken, the first free choice is used
  for the session only. With none free Lumen keeps running and the tooltip says so (T003).
- Placement: monitor under the cursor, horizontally centered, top edge at 20% of the work area,
  clamped inside it; logical width 800 (1200 with Quick Look), compact height 64
  (`tauri.conf.json`). The UI asks for its content size (`resize_overlay`, from `features/root-search/layout.ts`); the shell caps
  it at 72% of the work area so the top edge never moves, and re-applies it on every show.
- Dismiss: Escape (ignored during IME composition), focus loss, Alt+F4. Quit: tray → Quit Lumen.
- Keys (T104/T108, `features/root-search/keymap.ts`): ↑/↓, PageUp/PageDown move the
  selection; Enter primary action, Ctrl+Enter reveal, Ctrl+K Action Panel, Ctrl+L query,
  Alt+Enter Quick Look (T105). Click runs the primary action. Escape closes the Action
  Panel, then Quick Look, then the overlay.
- On every show the shell emits `lumen:overlay-shown`; the UI focuses and selects the query.
- Diagnostics mode (T110): `LUMEN_DIAGNOSTICS=1` adds provider · match kind · confidence
  under every result row and logs per-query timing/failed providers to the WebView console
  (`console.debug`). Off by default; never shown in normal UI.
- Indexed locations (T111, ADR-027): tray → Indexed locations / Exclusions; stored as
  `index.locations` in `lumen.db` settings; any edit cancels and restarts the catalog pass.
- Content indexing (T202, ADR-029): after each catalog pass the same thread runs the
  content pass (text/code → `chunks`) and, when `LUMEN_EMBED_MODEL_DIR` (EmbeddingGemma 2
  ONNX copy, e.g. `.cache\t006\embeddinggemma-2-ONNX`) and `LUMEN_ORT_DYLIB`
  (`onnxruntime.dll`) are set, embedding-queue slices into `chunk_vectors`
  (`LUMEN_EMBED_VARIANT=q4|q8|fp32`, `LUMEN_EMBED_THREADS=N` override the defaults). Tray →
  Content indexing shows progress and "Pause indexing"; tray → Indexed locations → a
  location → "Index file contents". `scripts/t202/run-windows-indexing.ps1 [-Launch]`.
- Root search (T107, ADR-025): the UI calls `search(queryId, text)` per query change, on
  show and on `lumen:catalog-changed`; results stream as `lumen:results`. The catalog lives
  in the same `lumen.db`; the first sync starts 2 s after launch.
- Window material (T004, ADR-024): transparent window + DWM system backdrop. Tray → "Window
  material" = Automatic (Acrylic) / Acrylic / Mica / Solid, saved as `appearance.material`;
  `LUMEN_MATERIAL=auto|acrylic|mica|solid` overrides for one run. Backdrops need Windows 11
  22H2+; high contrast or Transparency effects off force Solid (re-checked on every show).
  The UI asks `overlay_appearance` before `overlay_ready` and follows `lumen:appearance`;
  CSS keys off `<html data-material data-corners>`. Compare materials with
  `scripts/t004/run-windows-material.ps1` (screenshots stay in `target/t004/`).
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
