# HANDOFF.md

> The live continuation only (roles: `docs/AGENT_PROTOCOL.md` §1). Rewrite every session;
> remove DONE tasks (their outcome lives in `PROJECT_STATE.md` and the ADRs).

## Branch

`main` on github.com/Juancinho/lumen (push pending from joao-pc). Last commits: T015 → T111 → T201
→ T014 (instrumentation).

## Active task

None claimed. T202 and T014 are in REVIEW waiting for the Windows runs below; next per
`TASKS.md` → **Next**: T204 (warm query embedding service) → T203 (ANN generations).

## Pending human checks (Windows, joao-pc)

1. `git push` (CI compiles everything new on windows-2025).
2. Build and try the app:
   ```powershell
   cd apps\desktop; npm run build; cd ..\..
   cargo build --release -p lumen-desktop --features tauri/custom-protocol
   target\release\lumen.exe
   ```
   Wait a few seconds for the first catalog sync, then the checks in the REVIEW sections
   below (T107 search, T104 keys, T108/T109 actions, T105 Alt+Enter, T103/T004 look, T003
   shortcut). `LUMEN_DIAGNOSTICS=1` shows ranking evidence (T110).
3. Verdict on the default window material (Acrylic vs Mica, tray → Window material).
4. T014 throughput run (~3 GB first download, 20–40 min, plugged in, PC idle):
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\t014\run-windows-throughput.ps1 -Download
   ```
   Then commit `docs\benchmarks\t014\<date>-joao-pc\` (counts and timings only). Failed
   rows (e.g. CUDA without a recent driver) are fine — they are recorded.
5. T202 content indexing (needs the T006 model in `.cache\t006`; quit Lumen first — a
   second launch only focuses the running one):
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Root D:\Proyectos\lumen
   powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Launch -SkipBench
   ```
   Check: tray → Content indexing counts files, then "semantic N% (x of y passages)" rising;
   Task Manager CPU ≈ a quarter of the machine while you use the PC, about half when idle;
   "Pause indexing" stops it within a second or two and survives a restart; unplugging a
   laptop drops to 1 thread; tray → Indexed locations → a location → "Index file contents"
   off stops new content there. Commit `docs\benchmarks\t202\<date>-joao-pc\`.

## T202 — outcome (REVIEW, ADR-029)

- `lumen_content::{run_content_pass, run_queue, QueueJob, Control, Hold}`;
  `lumen_storage::content` (candidates by keyset, `write_content`, generations,
  `pending_chunks`, `write_vectors` f16, `queue_counts`, `vectors`); migration 0002 (drops
  `chunks.embedding_generation`, adds content state, `generations`, `chunk_vectors`);
  `NewChunk` now has `start_offset`/`end_offset`; `lumen_extract::{TEXT_EXTENSIONS,
  EXTRACTOR_VERSION}`.
- `lumen-bench pipeline --root DIR [--backend ort ...] [--duty F] [--max-seconds S]`.
  Sandbox: queue overhead 0.026 ms/chunk, ~600 B/vector, duty 0.5 → exactly 50 % CPU, but
  1 thread beats 2 threads × duty 0.5 by ~20 % per CPU.
- Existing databases migrate to v2 on the next app start (tested v1-with-chunks → v2).
- Shell: `indexing.rs` (content pass + 30 s queue slices on the catalog thread, plan from
  `lumen_windows::system` power/memory/idle via `policy::plan`, unload on drain), tray →
  Content indexing (status + Pause, `indexing.paused`), per-location "Index file contents"
  (`index.locations` v2; v1 upgrades to content on). Linux smoke: 154 chunks embedded.

## T014 — outcome so far (REVIEW, Windows run pending)

- `lumen-bench embed --backend llama-server --server HOST:PORT --server-target cpu|gpu
  --variant gguf-q8_0` measures llama.cpp builds over localhost HTTP (`/v1/embeddings`) with
  the same latency / throughput / fidelity harness; `--cpu-pid PID` reports that server's
  CPU. Every throughput row now carries `cpu {cpu_s, cores, machine_percent}` (report
  schema v2); Windows CPU time via `lumen_windows::process::cpu_time` (GetProcessTimes).
- Sandbox (2 vCPU, ORT q4, ~128-token chunks): 3.4 chunks/s per busy core, linear 1→2
  threads, batching irrelevant on CPU; fidelity min cos 0.980, recall@1 1.0.
- After the Windows run: pick runtime + thread cap for the Balanced profile (or revise the
  §9 budget with evidence) in a new ADR, amend ADR-015, then T202 uses it.

## T201 — outcome (REVIEW, ADR-028)

- `lumen_extract::{kind_for_extension, extract_file, decode, chunk, ChunkConfig, Chunk,
  TokenCount, EstimateTokens}`; 13 tests.
- `lumen-bench chunk --root DIR [--target N] [--tokenizer tokenizer.json]` (feature
  `tokenizer` for real counts). Evidence in `docs/benchmarks/t201/`.
- For T202: `NewChunk` still lacks `start_offset`/`end_offset` (columns exist) — add them when
  writing chunks; embed with `PromptFormat` document prompt (`title: <name> | text: …`).

## T111 — outcome (REVIEW, ADR-027)

- Code: `lumen_catalog::locations::{IndexLocations, location_states, LocationState}`
  (setting `index.locations`), `lumen_indexer::Exclusions::{default_names,
  build_dirs_next_to_markers}` + `DEV_NOISE_NAMES`, `BUILD_DIR_NAMES`, `PROJECT_MARKERS`,
  `sync_files_with_progress`; shell `catalog.rs` (model, edits, cancel+restart, progress
  events, states), `tray.rs` (Indexed locations / Exclusions submenus, folder picker via
  tauri-plugin-dialog), `actions.rs` (`lumen.exclude-folder`), `settings::{get_raw,set_raw}`.
- Windows acceptance (spec §Acceptance):
  1. tray → Indexed locations → Add folder or drive… → `D:\Proyectos` → its files appear
     within seconds (no restart); Remove → they disappear.
  2. A USB drive as location → unplug → "(not available)", results kept; replug → ok.
  3. `node_modules` hidden by default; tray → Exclusions → untick → its files appear.
  4. A `build` folder without a project marker is searchable; next to `Cargo.toml` it is not.
  5. Folder result → Ctrl+K → "Exclude folder from Lumen" → gone after the pass; tray →
     Exclusions → the folder → Include again.
  6. Restart → the list is kept.
  7. `powershell -ExecutionPolicy Bypass -File scripts\t111\run-windows-locations.ps1 -Drive D:\`
     (and optionally `-Drive C:\`): first sync time, DB size, keystroke p95.

## T105 — outcome (REVIEW)

- Shell: `preview.rs` (`preview(item)`: metadata, `excerpt` with binary/encoding guards),
  command `preview_result(queryId, resultId)`; `overlay::resize(width, height)` with
  `placement::{clamp_width, expand_x}` (grow right, shift left only on overflow);
  `resize_overlay` returns `{width, height}`.
- UI: `usePreview` (follows selection, drops late answers), `PreviewPane`, `format.ts`,
  `layout::overlaySize`; docked when the applied width ≥ 1100.
- Windows check: Alt+Enter on a .txt/.md shows its start; on an app/photo shows metadata;
  the search bar does not move; Escape closes the preview first.

## T110 — outcome (REVIEW)

- `LUMEN_DIAGNOSTICS=1`: `ResultDto.diagnostics` + `ResultsDto.diagnostics` (skipped from the
  JSON otherwise), mono line under rows, `console.debug` per update.

## T108 + T109 — outcome (REVIEW, ADR-026)

- Core: `lumen_search::{available, prepare, ActionError}`, `SearchService::lookup`;
  `lumen_catalog::usage::{record_action, item_id, use_kind}`.
- Shell: `actions.rs` (REGISTRY = built-ins, executors via tauri-plugin-opener + arboard),
  commands `list_actions`, `run_action`; `ResultDto.primaryAction`, `ActionDto`.
- UI: `useActions` (run, panel state, notice), `ActionPanel`, layout grows for the panel.
- Windows check: Enter on an app launches it; on a file opens it; Ctrl+Enter selects it in
  Explorer; Ctrl+K → Copy path → paste somewhere; picking the same item a few times makes it
  rise for that query.

## T104 — outcome (REVIEW)

- `features/root-search/keymap.ts` (pure `commandFor`), `selection.ts` (`Selection`,
  `selectedIndex`, `moveSelection`, `selectIndex`); handled on the query input's keydown;
  ResultList scrolls the selection into view. No wrap-around; PageUp/Down = 8 rows.
- Enter/Ctrl+Enter/Alt+Enter/Ctrl+K are claimed but do nothing until T108/T109.
- Windows check: arrow keys feel instant; selection does not jump when results refresh.

## T107 — outcome (REVIEW, ADR-025)

- `crates/lumen-search`: `Coordinator::{register, run}` (latency-class order, merged
  updates, failed providers reported), `merge`, `SearchService::{start, submit}` (one
  thread, latest wins, stale ids dropped, superseded queries silent).
- Shell: `search.rs` (service + CatalogProvider reader, `lumen:results`), command
  `search(queryId, text) -> accepted`, `catalog.rs` (background sync: apps, then Desktop/
  Documents/Downloads/Pictures/Music/Videos; start-up +2 s and every 30 min;
  `lumen:catalog-changed`), DTOs `ResultDto`/`ResultsDto`.
- UI: `useResults` (ids, keeps rows until the next answer, re-runs on show/catalog change),
  `ipc` `search/onResults/onCatalogChanged`, `lib/subscribe.ts`.
- Diag (`LUMEN_DIAG_LOG`): `search_done_ms`, `search_partial_ms`, `catalog_apps_ms`,
  `catalog_pass_ms`.
- Enter/click do nothing yet (T109); arrows are T104.

## T103 — outcome (REVIEW)

- `src/design/tokens.css`; `src/features/root-search/{RootSearch, SearchField, ResultList,
  ResultRow, icons, model, layout, useResults}`; command `resize_overlay(height) -> applied`
  (`overlay::resize`, `placement::clamp_height`, 72 % cap, re-applied on show).
- `useResults` is a placeholder (always idle) until T107 connects the provider stream.
- Review on Windows once T107 shows results: 100/125/150 % scaling, light/dark, Acrylic and
  Solid, long names/paths, high contrast. Previewed in Chromium (light/dark/acrylic/narrow).

## T004 — outcome (REVIEW, ADR-024 accepted on measurements)

- Run: `powershell -ExecutionPolicy Bypass -File scripts\t004\run-windows-material.ps1`
  (put a bright, busy window behind the top-centre of the screen first; ideally run once in
  light and once in dark mode). Prints per material: what was applied and why, show→paint
  p50/p95, per-show check cost, DWM GPU %, real contrast from screen samples. Screenshots in
  `target\t004\` (private). JSON in `docs\benchmarks\t004\<date>-<pc>\`.
- Visual check: tray → Window material → try each; corners rounded, shadow visible, no
  white flash on show, text legible over a white page and over a dark one; Settings →
  Accessibility → Visual effects → Transparency effects off → next show is Solid (tray says
  "using Solid: transparency effects are off").
- Measured on joao-pc (dark): ~22 ms show→paint for all materials, Acrylic +3–4 % DWM GPU
  while visible, on-screen secondary contrast ≥ 5.3:1. Remaining: the user's visual verdict
  (keep `auto` = Acrylic or switch to Mica); optional light-mode run. Then T004 DONE.
- Code: `lumen_windows::material::{plan, system_appearance, round_corners}` (pure plan, unit
  tested; WinRT UISettings/AccessibilitySettings; DWM corner preference — the Windows-only
  parts are compiled only by Windows CI); shell `material.rs` (install, `before_show`,
  `choose`, `current`; command `overlay_appearance`, event `lumen:appearance`); UI
  `src/design/material.css` + `material.test.ts` (contrast floor), `src/app/appearance.ts`.
- T012 script now builds with `--features tauri/custom-protocol` (it did not).

## T003 — outcome (REVIEW)

- Windows check: build/run `lumen.exe` (`npm run tauri build` or the T012 script's build);
  right-click tray → Keyboard shortcut → pick Ctrl+Space → it toggles; restart → still
  Ctrl+Space; a combination owned by another app shows "(in use by another app)" and picking
  it keeps the previous one. Then DONE.
- Shell now opens `lumen.db` in app data at start-up (`settings.rs`); `lumen_storage::Store::
  {setting, set_setting, remove_setting}`.
- Linux Xvfb smoke: starts `--background`, creates the DB, UI ready; no panics.

## Embedding runtime notes (T006, for T014/T202/T204)

- `crates/lumen-embedding-ort`: `init_runtime(dylib)` once per process; `OrtBackend::new(OrtConfig)`
  (`model_dir`, `ModelVariant` fp32/fp16/q8/q4/q4f16, `Device` cpu/dml:N/dml:high/dml:low,
  `threads`, `max_batch`=16, `max_tokens`=2048, `cpu_fallback`=true); `placement()` parses ORT
  verbose node placement. Graph inputs: input_ids/attention_mask + empty [0,512] media features;
  output `sentence_embedding` (mean-pooled, unit norm, 768d).
- `crates/lumen-bench`: features `ort`, `directml`; `--backend ort --ort-dylib --model-dir --variant
  --device --threads --placement --no-cpu-fallback --reference --corpus --long-words`.
- `fixtures/embedding/`: corpus (24 queries / 36 docs, EN+ES) + fp32 reference (256d);
  regenerate with `scripts/embedding/make_reference.py` (dev-only Python).
- `scripts/t006/run-windows-bench.ps1`: Windows matrix (process per config, DLLs next to exe).
- Local assets (git-ignored): `.cache/t006/{ort-cpu,ort-dml,embeddinggemma-2-ONNX}`.

## Validation (all green in the Linux sandbox for every commit above)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p lumen-bench --features directml --all-targets -- -D warnings
cargo xtask test
cargo xtask arch            # 10 core crates
cd apps/desktop && npm run check
```

Windows-only code (`#[cfg(windows)]`: lumen-windows apps/material, shell lifecycle/material)
cannot be compiled in the sandbox — push before running Windows scripts so CI catches it.

## Known issues / notes

- Sandbox → device sync: code is built in the cloud copy and copied to `D:\Proyectos\lumen`;
  git in that folder leaves `.lock` files (no delete permission) — moved to
  `.git/stale-locks/`, safe to delete.
- Linux WebKitGTK enforces a ~200 px minimum window height (smoke runs only).
- `.cache/` holds ~2.3 GB of models/DLLs (T006); `-Download` restores it.
- Packaging must ship `onnxruntime.dll` next to the exe; model download UX undecided (T807).

## Unresolved evidence-based decisions

- Default material Acrylic vs Mica (T004, user); indexing runtime/device (T014);
  q4 vs fp32 relevance at scale (T205); content-FTS settle delay (T205/T206);
  FastFrame/egui spike timing (TX01); TS bindings (ADR-013).
