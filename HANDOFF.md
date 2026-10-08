# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main` (no remote yet). Commits: spec baseline → T001 → T011 → T002 → T005 → T006 → T008 → T007 → T009 → T010.

## Active task

**Pending Windows runs (user):** T004 material, T101+T102 catalog, T003 shortcut check —
details per task below. Next: T107 (progressive result stream), then T104.

## T103 — outcome (REVIEW)

- `src/design/tokens.css`; `src/features/root-search/{RootSearch, SearchField, ResultList,
  ResultRow, icons, model, layout, useResults}`; command `resize_overlay(height) -> applied`
  (`overlay::resize`, `placement::clamp_height`, 72 % cap, re-applied on show).
- `useResults` is a placeholder (always idle) until T107 connects the provider stream.
- Review on Windows once T107 shows results: 100/125/150 % scaling, light/dark, Acrylic and
  Solid, long names/paths, high contrast. Previewed in Chromium (light/dark/acrylic/narrow).

## T004 — outcome (REVIEW, ADR-024 proposed)

- Run: `powershell -ExecutionPolicy Bypass -File scripts\t004\run-windows-material.ps1`
  (put a bright, busy window behind the top-centre of the screen first; ideally run once in
  light and once in dark mode). Prints per material: what was applied and why, show→paint
  p50/p95, per-show check cost, DWM GPU %, real contrast from screen samples. Screenshots in
  `target\t004\` (private). JSON in `docs\benchmarks\t004\<date>-<pc>\`.
- Visual check: tray → Window material → try each; corners rounded, shadow visible, no
  white flash on show, text legible over a white page and over a dark one; Settings →
  Accessibility → Visual effects → Transparency effects off → next show is Solid (tray says
  "using Solid: transparency effects are off").
- Decide with the user: keep `auto` = Acrylic or switch to Mica. Then ADR-024 Accepted,
  T004 DONE.
- Code: `lumen_windows::material::{plan, system_appearance, round_corners}` (pure plan, unit
  tested; WinRT UISettings/AccessibilitySettings; DWM corner preference — the Windows-only
  parts are compiled only by Windows CI); shell `material.rs` (install, `before_show`,
  `choose`, `current`; command `overlay_appearance`, event `lumen:appearance`); UI
  `src/design/material.css` + `material.test.ts` (contrast floor), `src/app/appearance.ts`.
- T012 script now builds with `--features tauri/custom-protocol` (it did not).

## T101 + T102 — REVIEW — one Windows run validates both:
`powershell -ExecutionPolicy Bypass -File scripts\t101\run-windows-catalog.ps1`
(native tests incl. AppsFolder COM; catalog bench over user folders + apps with the T102
ranking; prints top results for calc/spotify/visual/config/notas). Then both DONE, ADR-021/022
Accepted. Next after that: T103 (premium overlay) or T106/T107 (signals, result stream).
DONE: T001, T002, T005–T013, T106. M0 gate met; M1 in progress.

## T003 — outcome (REVIEW)

- Windows check: build/run `lumen.exe` (`npm run tauri build` or the T012 script's build);
  right-click tray → Keyboard shortcut → pick Ctrl+Space → it toggles; restart → still
  Ctrl+Space; a combination owned by another app shows "(in use by another app)" and picking
  it keeps the previous one. Then DONE.
- Shell now opens `lumen.db` in app data at start-up (`settings.rs`); `lumen_storage::Store::
  {setting, set_setting, remove_setting}`.
- Linux Xvfb smoke: starts `--background`, creates the DB, UI ready; no panics.

## T106 — outcome (ADR-023)

- `lumen_storage::usage`: `Store::{record_use, usage_for, learned_choices, suggestions, pin,
  unpin, prune_usage, clear_usage}`, `UseKind`, `UsageSignal`; tables `usage_stats`,
  `query_choices`, `pins` (replace `usage_events`).
- `lumen_catalog::rank::{usage_prior, score_learned}`; provider adds learned candidates,
  usage priors on the top 50, empty query → suggestions.
- To wire in T109: call `record_use` after an action runs (query key = folded query).

## T102 — outcome (ADR-022)

- `lumen_catalog::text::{tokens, name_parts, path_parts}`; `lumen_catalog::rank::{ParsedQuery,
  score, edit_distance, typo_budget}`; provider `gather` = 3 bounded stages.
- Storage: `items.name_parts/path_parts`, `names_fts` + triggers; `Store::{search_name_tokens,
  name_key_range}`; `SearchBudget::is_cancelled`.
- Relevance gate: `cargo test -p lumen-catalog --test relevance` (fixture in `fixtures/search/`).
- Bench `catalog` now reports p95 by prefix length.

## T101 — outcome (ADR-021)

- `lumen_core::{Provider, ProviderQuery, LatencyClass, ProviderError}`; `lumen_core::builtin`
  (OPEN, LAUNCH, REVEAL, COPY_PATH + DESCRIPTORS).
- `lumen_storage`: schema 0001 edited (source, raw_path, name_key, launch_target, attributes,
  seen_scan, `scans`; exact-unique path, non-unique identity); `Store::{begin_scan,
  upsert_entries, unseen_items, delete_items, finish_scan, search_names, catalog_item,
  count_items}`; `bounded()` budget helper.
- `crates/lumen-windows`: `start_apps()` (AppsFolder via COM; `deny(unsafe_code)` + module allow).
- `crates/lumen-catalog`: `text::fold`, `path::{encode, decode}` (lossless non-Unicode),
  `sync_files`, `sync_apps`/`discover`/`write_apps`, `CatalogProvider`.
- Bench: `lumen-bench catalog --root DIR --apps [--show Q]` (counts only in JSON).

## T012 — outcome (ADR-020)

- `apps/desktop/src-tauri/src/lifecycle.rs`: `HiddenMode` keep | invisible | low-memory |
  suspend | **idle-low-memory (default, 30 s)**; WebView2 COM in a `#[allow(unsafe_code)]`
  module (crate is `deny(unsafe_code)`); delayed work re-checks a generation counter on the UI
  thread. `diag.rs` (LUMEN_DIAG_LOG), `instance.rs` (second-launch commands),
  `overlay_painted(seq)` command + UI double-rAF report.
- Release exe for measurements must be built with `--features tauri/custom-protocol`.
- Pending validation: `scripts/t012/run-windows-webview.ps1 -Modes idle-low-memory`.

## T010 — outcome

- CI green on Linux + Windows since 94913df (`cargo xtask test` splits the shell's tests:
  tauri-build's empty msvcrt.lib broke other crates' doctests in one cargo call).

## T013 — outcome (ADR-019)

- `lumen_embedding::policy::plan(space_key, &[DeviceProbe], &Quarantine, &SystemState,
  &PolicyConfig) -> DevicePlan { query_device, indexing: Run{device,threads}|Paused(reason),
  rejected }`; `Quarantine::record_failure`, `is_device_failure`.
- `lumen_embedding::probe::measure(&Embedder, &ProbeCorpus, Option<&ProbeVectors>, &ProbeConfig)`.
- `lumen-bench probe` (embed options + `--device-id --integrated --runtime-key --save-vectors
  --cpu-vectors --device-memory-mib --device-memory-total-mib`) and `lumen-bench device-policy
  --probe F...` (7 scenarios). Sandbox q4 CPU probe: p50 48 ms, 2.8 chunks/s (2 vCPU).

## T010 — outcome

- CI jobs: frontend (Ubuntu), rust gate on Ubuntu 24.04 + Windows 2025 (UI built first for
  the Tauri context; fmt, clippy `--locked -D warnings`, directml clippy, tests, arch), quick
  bench suite on both OSes uploaded as artifacts (non-gating).
- `xtask/src/bench.rs`: builds `lumen-bench` release `--locked`, runs embed-mock, ann, storage,
  scan-repo (repo minus target/node_modules/.git/.cache), identity-check → JSON per bench.
  Sandbox `--quick`: 31 s including the release build cache hit.

## T009 — outcome (ADR-018)

- `crates/lumen-indexer`: `scan(&ScanOptions { roots, exclusions, identity }, on_entry, cancel)`
  → `ScanReport` (counts, `excluded` with rule, `issues` with `IssueStage`/`IssueKind`,
  `is_complete()`, `blocking_issues()`, `non_unicode_paths`, `identity_skipped`).
  `ScanEntry { path, kind, size, modified_ms, created_ms, flags, identity }`.
  `identity_of(path)` → `FileIdentity { volume, file }` (`volume_key()/file_key()` hex for
  `items`). Windows-only `winpath::verbatim`.
- Windows run (`scripts/t009/run-windows-scan.ps1`, results in
  `docs/benchmarks/t009/2026-10-08-joao-pc/`): 12/12 native tests, 7/7 edge-case checks,
  coverage COMPLETE and count = .NET walk, identity-check OK on C: and D:.
- Identity is ~7× the walk cost warm → bulk per-directory ids or a deferred pass (T101/T207).
- `.cargo/config.toml [env]` carries the usearch/numkong MSVC link workaround (ADR-016).
- Bench: `lumen-bench scan --root DIR [--identity --repeat N --exclude-name X]` (counts only,
  no paths in JSON), `lumen-bench identity-check [--dir DIR]`.
- Findings for T101/T207: hard links vs `UNIQUE(volume_id, file_id)`; save-by-replace gives a
  new id at the same path; inode reuse; non-Unicode paths need lossless storage.

## T007 — outcome (ADR-017)

- `crates/lumen-storage`: `Store::open_writer(path)` (WAL, pragmas, migrations) /
  `Store::open_reader(path)` (read-only, query_only); `insert_item`, `item_id_by_path`,
  `delete_item` (cascade), `insert_chunks` (one tx), `update_chunk_text`,
  `search_chunks(&FtsQuery, limit, &SearchBudget)` → `ChunkHit { chunk_id, item_id, rank,
  snippet }`, `checkpoint`, `query_plan`. Migrations in `crates/lumen-storage/migrations/`.
- `FtsQuery::from_user(input, typing)`: quoted terms, phrases kept, prefix only ≥3 chars.
- `SearchBudget::within(d).with_cancel(token)` → `StorageError::Interrupted`.
- Bench: `lumen-bench storage`; results `docs/benchmarks/t007/2026-10-08-cloud-sandbox/`.

## T008 — outcome (ADR-016)

- `crates/lumen-vector`: `VectorIndex::{new, reserve, add, search, remove, save, load, view}`,
  `IndexConfig { dim, metric, scalar, params }`, `Scalar::{F32,F16,BF16,I8}`. Keys = VectorId u64.
- Decision: f16, cosine, M=16, ef_construction=128, **ef_search=256** (0.99 recall at 1M,
  ~1.3 ms). i8 rejected (0.85 recall), bf16 0.995.
- `lumen-bench ann [--sizes --scalars --efs --dataset --vectors D.f32,Q.f32 ...]`; results in
  `docs/benchmarks/t008/2026-10-08-cloud-sandbox/`. Optional: `scripts/t008/run-windows-ann.ps1`
  (Windows latencies), real-embedding recall via `scripts/embedding/embed_corpus.py`.
- T203 must add a mutable delta index next to the mmap'ed (read-only) generation file.

## T006 — outcome (ADR-015)

- Default: ONNX Runtime **CPU**, weights **q4** (`model_q4`): 30.0/36.9 ms p50/p95 query,
  133 ms @~128 tokens, 168 MiB, min cos 0.980 vs fp32, top-1 identical. fp32 = 33 ms / 627 MiB
  (quality profile). q8 = 221 ms on AVX2 (rejected).
- DirectML (GTX 1650 / Vega iGPU): 293–572 ms queries, 2–3× indexing at 1.1–2.9 GB VRAM, fp16
  zero-norm, iGPU device hang → not default.
- Evidence: `docs/benchmarks/t006/2026-10-07-joao-pc/summary.md` (+ per-run JSON/logs).

## Code (T006)

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

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p lumen-bench --features directml --all-targets -- -D warnings
cargo xtask test            # cargo test in two calls (Windows doctests, xtask/src/test.rs)
cargo xtask arch            # 6 core crates + bench OK
LUMEN_EG2_MODEL_DIR=… LUMEN_ORT_DYLIB=… cargo test -p lumen-embedding-ort --release --test fidelity
cd apps/desktop && npm run check
```

All passed in the Linux sandbox (fidelity: fp32 1.00000 / q8 0.99991 / q4 0.97973 min cos).
Windows: `run-windows-bench.ps1` ran on joao-pc (results above).

## Exact next steps

1. **T013** device policy (CPU q4 default; probe + placement before ever using a GPU; profiles).
2. **T014** if indexing speed matters before M2: LiteRT-LM (int4 QAT, 270M text model),
   llama.cpp GGUF (CPU/Vulkan/CUDA), Windows ML EPs, WebGPU EP — same harness/fidelity bar.
3. Unblocked foundation tasks: T101 (after T009), T010 (CI),
   T003/T004/T012 (shell). T201/T204 can target `OrtBackend`; T203 can target `lumen-vector`.

## Known issues / notes

- `.cache/` holds ~2.3 GB of models/DLLs; safe to delete, `-Download` restores it.
- Windows memory numbers are working set (memory-stats), not private working set.
- Packaging must ship `onnxruntime.dll` (+ `onnxruntime_providers_shared.dll`) next to the exe;
  the model location/download UX is undecided (onboarding/T807).
- If Windows git reports "dubious ownership": `git config --global --add safe.directory D:/Proyectos/lumen`.

## Unresolved evidence-based decisions

- native backdrop path (T004); GPU/NPU embedding path (T013/T014);
  q4 vs fp32 relevance at scale (T205); FastFrame/egui spike timing (TX01); TS bindings (ADR-013).
