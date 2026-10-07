# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main` (no remote yet). Commits: spec baseline → T001 → T011 → T002 → T005 → T006 → T008 → T007 → T009 → T010.

## Active task

**T010 REVIEW** — `.github/workflows/ci.yml` (actionlint-clean; every command it runs passes
locally) and `cargo xtask bench [--quick]`. Becomes DONE after the first green GitHub run, which
needs a remote (the repo has none yet — the user decides where to host it).
**T013 DONE** — joao-pc: GTX 1650 2.35x indexing, cos 0.9999995, 94 % offloaded, but 2.3 GB
VRAM (> 1.5 GiB cap) → CPU everywhere. User decision: Turbo may use up to 60 % of VRAM → on joao-pc Turbo indexes on
the GTX 1650; Balanced/Eco stay on CPU.
DONE: T001, T002, T005, T006, T007, T008, T009, T011.

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
