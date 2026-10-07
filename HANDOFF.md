# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main`. Commits: spec baseline → T001 → T011 → T002 → T005.

## Active task

None claimed. T001, T011, T002, T005 are DONE.

## T005 — implemented behavior

- `crates/lumen-embedding` (depends only on `lumen-core`):
  - `EmbeddingBackend` (sync, `Send + Sync`): `capabilities`, `warm/unload/is_warm(Modality)`,
    `embed_text(&[&str]) -> Vec<f32>` raw native-dim row-major. ADR-014 explains why sync.
  - `Capabilities`/`ModelInfo` (native dim, Matryoshka dims, max batch, target cpu/gpu/npu,
    preprocessing version, concurrent_calls).
  - `Embedder::new(Arc<dyn EmbeddingBackend>, EmbeddingProfile)` validates the profile;
    `embed(task, &[TextInput], Option<&CancellationToken>) -> EmbeddingBatch` and `embed_query`.
    Empty inputs rejected up front; cancel checked before each backend batch; error indices are
    caller indices.
  - `PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1` (`task: search result | query: …`,
    `title: {title|none} | text: …`) and `RAW`. **T006 must verify against EmbeddingGemma 2.**
  - `EmbeddingProfile::DEFAULT` = 256d + Gemma prompts; `EmbeddingSpace::key()` e.g.
    `embeddinggemma-2@q8-1/pre1/embeddinggemma-retrieval@1/d256/l2`.
  - `MockBackend` (FNV-1a feature hashing of words + trigrams, 768d) with `MockLatency`.
  - `TextInput` Debug never prints content.
- `crates/lumen-bench` (binary `lumen-bench`): `embed` subcommand; `make_backend()` in
  `src/embed.rs` is where T006 adds real backends (behind cargo features).

## Validation (Linux sandbox, Rust 1.97.0)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace      # core 32+3 doc, embedding 25, bench 9, shell 7, xtask 8
cargo xtask arch            # OK - lumen-bench, lumen-core, lumen-embedding
cd apps/desktop && npm run check   # 10 tests
cargo run --release -p lumen-bench -- embed --json target/bench/embed-mock.json
cargo run --release -p lumen-bench -- embed --iterations 30 --batch-sizes 8 --docs 64 \
  --mock-load-ms 300 --mock-call-ms 40 --mock-item-ms 2   # measured 300.2 / 42.2 / 56.9 ms
```

Sandbox numbers are harness sanity checks only (2 vCPU Xeon), not evidence.

## Exact next steps

1. **T006** (needs Windows hardware): pick candidate runtimes (e.g. ONNX Runtime CPU/DirectML,
   OpenVINO CPU/NPU, llama.cpp/GGUF), implement each as an `EmbeddingBackend` behind a cargo
   feature (separate crate per runtime keeps `lumen-embedding` runtime-free), verify
   EmbeddingGemma 2 prompts, run
   `cargo run --release -p lumen-bench --features <rt> -- embed --backend <rt> --label "<machine, power>"`
   per target, compare against mock-independent reference vectors (cosine ≥ 0.99 vs reference
   implementation), record license/size/packaging per docs/ARCHITECTURE.md §19, write the ADR.
   Commit reports under `docs/benchmarks/`.
2. Parallel-safe now: T007 (SQLite/FTS → `crates/lumen-storage`), T008 (USearch 256d bench — add
   `lumen-bench ann`), T009 (file identity), T010 (CI: wire the gate + `lumen-bench`), T004, T012, T003.

## Known issues / notes

- Windows memory in reports is the working set (memory-stats), not private working set.
- `lumen-bench` measures in-process only; IPC/render latency belongs to T010/T012.
- Plain `cargo run` of `lumen-desktop` loads the dev URL; use `npm run tauri dev|build`.
- Overlay height 64 is a placeholder until T103.

## Unresolved evidence-based decisions

- production EmbeddingGemma runtime + verified prompts (T006); native backdrop path (T004);
  vector scalar profile (T008); FastFrame/egui spike timing (TX01); TS binding generation
  (ADR-013 revisit).
