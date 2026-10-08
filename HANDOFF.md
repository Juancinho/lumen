# HANDOFF.md

> The live continuation only (roles: `docs/AGENT_PROTOCOL.md` §1). Rewrite every session;
> remove DONE tasks (their outcome lives in `PROJECT_STATE.md` and the ADRs).

## Branch

`main` on github.com/Juancinho/lumen (push pending from joao-pc). Last commits: T015 → T111 → T201
→ T014 (instrumentation) → T202 → T204 → T203 → T205 → T211 → T210.

## Active task

None claimed. T202, T014, T204, T203, T205, T211 and T210 are in REVIEW (Windows runs
below); next per `TASKS.md` → **Next**: T206 → T209 → T207.

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

6. T204 query lane (~10 min, T006 cache, PC idle and plugged in):
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\t204\run-windows-query-lane.ps1
   ```
   Commit `docs\benchmarks\t204\<date>-joao-pc\`. Pass if the `with_indexing_preempted`
   p95 with `-b1` is ≤ 80 ms (then ADR-030 → Accepted).

7. T203 ANN generations (optional timing, ~5 min with `-Large`, no model needed):
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\t203\run-windows-ann-gen.ps1 -Large
   ```
   Commit `docs\benchmarks\t203\<date>-joao-pc\`. With the app and a model (T202 run),
   `%APPDATA%\dev.lumen.desktop\vectors\` gets a `gen-*.usearch` file once ~2,000
   passages are embedded.

8. T205 relevance + the app with semantic search (needs the T006 model):
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\t205\run-windows-eval.ps1
   ```
   Commit `docs\benchmarks\t205\<date>-joao-pc\`. Then start the app with
   `LUMEN_EMBED_MODEL_DIR` / `LUMEN_ORT_DYLIB` set (as in the T202 check) and, once the tray
   shows passages embedded, type a sentence describing a document in your indexed folders:
   after a short pause rows from contents and meaning join the name results without the
   selection jumping; `LUMEN_DIAGNOSTICS=1` shows `lumen.content` / `lumen.semantic`.

9. T210 semantic search install (no environment variables this time):
   start `target\release\lumen.exe` normally → tray → Semantic search → shows "not
   installed (222 MB download)" → Download… → read the dialog (size, huggingface.co /
   files.pythonhosted.org, Apache-2.0 / MIT) → Download → the line counts up; Cancel
   download and Resume download… continue where they stopped. When it says "installed",
   tray → Content indexing starts counting passages without a restart, and a descriptive
   search shows `lumen.semantic` rows (`LUMEN_DIAGNOSTICS=1`). Files land in
   `%APPDATA%\dev.lumen.desktop\models\` and `runtime\`. Remove… deletes the model.
   If your network needs a proxy, curl only honours `HTTPS_PROXY` — report what happens.

## T210 — outcome (REVIEW, ADR-034)

- `lumen_provision::{EMBEDDING_MODEL, INFERENCE_RUNTIME, install, state, verify, remove,
  CurlFetch, DirFetch, Progress, State}`; network tests are `#[ignore]`
  (`cargo test -p lumen-provision -- --ignored`).
- Shell: `provisioning::{model_dir, runtime_library, ready, ask_download, cancel,
  ask_remove, setup_text}`, `tray::refresh_semantic`, `indexing::on_model_installed/
  removed`, `search::on_model_installed/removed`; install order provisioning → indexing →
  search.
- Not built: an About/licenses screen (notices are installed with the files), WinHTTP
  transport (proxy settings), bundling `onnxruntime.dll` in an installer.

## T211 — outcome (REVIEW, ADR-033)

- `scripts/eval/make_hard_set.py` regenerates `fixtures/eval-hard/` deterministically;
  `lumen-bench eval --fixture fixtures/eval-hard --sweep [--explain]`.
- Weights 1/1/2 in `search.rs`; the Windows eval script can take `-Fixture` later — for
  now run `lumen-bench eval` by hand with `--fixture fixtures\eval-hard` (same flags as
  `scripts\t205\run-windows-eval.ps1`).

## T205 — outcome (REVIEW, ADR-032)

- Core: `lumen_search::{fuse, RRF_K, DEFAULT_SETTLE}`, `Coordinator::{register_weighted,
  has_settled_providers}`, `SearchService::start_with_settle`;
  `lumen_catalog::{ContentProvider, CONTENT_PROVIDER_ID}`; `lumen_semantic::{
  SemanticProvider, SemanticConfig, SharedIndex, SEMANTIC_PROVIDER_ID}`,
  `QueryEmbedder::space_key`; `lumen_storage::{FtsQuery::{content, two_of}, STOPWORDS,
  ChunkRef, Store::chunk_refs}`.
- Shell: `search.rs` weights `WEIGHT_{NAME,CONTENT,SEMANTIC}` = 1, `QueryLane` (query
  embedder, idle unload 10 min), `warm_semantic` on overlay show; `indexing` installs
  first and shares `control()` / `shared_index()`.
- Harness: `lumen-bench eval --fixture fixtures/eval [--weights N,C,S] [--sweep]
  [--explain]`; `eval-mock` in `cargo xtask bench`.
- Next (T211): a harder set (hundreds of documents, near-duplicates, long files) so the
  weights and the semantic floor can be tuned for real.

## T203 — outcome (REVIEW, ADR-031)

- Storage: migration 0003 (`chunk_vectors.seq`, `generations.next_seq/activated_at`,
  `ann_files`); `lumen_storage::{GenerationInfo, GenerationState, AnnFileRecord,
  SeqVector}` + `Store::{generations, active_generation, promote_first,
  activate_generation, delete_retired_vectors, ann_file, set_ann_file, clear_ann_file,
  ann_file_names, vectors_through, vectors_after_seq, vector_seqs, vector_count,
  vector_count_through}`.
- `lumen_semantic::{SemanticIndex (open, reopen_file, refresh, search, status,
  maintenance), build_file, validate, cleanup_files, ann_config, IndexSettings}`.
- Shell `indexing::maintain_ann` after queue slices; `Indexing.ann: RwLock<Option<
  SemanticIndex>>` is what T205's settled-query lane reads (with a reader `Store`), only
  when that generation is `active`.
- For T205: embed the settled query with the `QueryEmbedder` (ADR-030), search
  `Indexing.ann`, map chunk ids → items, fuse with name + content FTS; evaluation harness.

## T204 — outcome (REVIEW, ADR-030 proposed)

- `lumen_semantic::{QueryEmbedder, QueryConfig, QueryError, QueryStats, MakeEmbedder}`:
  `start(make, indexing: Option<Control>, config)`, `embed(text, &cancel)`, `warm`,
  `unload`, `clear_cache`, `stats`.
- `lumen_content::Control::{mark_interactive, interactive_within}` + one-chunk batches for
  10 s after interactive use; shell `indexing::on_overlay_shown` (called from
  `overlay::show`).
- `lumen-bench query-lane --backend ort ... --query-threads N --index-threads M
  --index-batch B --queries Q`.
- For T205: create the `QueryEmbedder` in the shell with the indexing `Control`, call
  `warm()` on overlay show, embed only the settled query.

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
