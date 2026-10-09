# T209 — Windows code-result evidence

`2026-10-09-joao-pc/`: Windows x86_64, Ryzen 5 5600H / 12 logical CPUs, release builds.
All source content is the committed synthetic corpus; reports contain counts/ranks/timings,
not personal paths, query strings or contents.

- `before-mock.json`: pre-T209 `1859f1e`, `fixtures/eval-hard`, default mock backend.
- `after-mock.json`: T209, same fixture/backend. Content metrics are independent of the
  embedding backend: code top-1 0.5→1.0; code NDCG@10 0.617→0.902.
- `after-q4.json`: T209, cached EmbeddingGemma 2 q4 / CPU ORT, six threads. Fused NDCG@10
  0.964 / top-1 0.959; content p95 2.32 ms. Names/mock semantics retain their baseline
  metrics. The real-model first query includes load (max 1.8 s); warm p95 is 65.67 ms.
- `upgrade-100k.json`: synthetic v3 database with 100k code chunks and 100k 256d f16
  vectors. Atomic migration 1,097 ms; separate background context backfill 1,597 ms.
  This intentionally puts all chunks in one file to exercise a large trigger update.
  Vector count/sequence preservation and FTS integrity are assertions in the harness.

Reproduce from the repository root (no downloads; the ORT run requires the T006 cache):

```powershell
cargo run --release -p lumen-bench --features ort -- eval --fixture fixtures/eval-hard --json target/bench/t209-mock.json
target/release/lumen-bench.exe eval --backend ort --model-dir .cache/t006/embeddinggemma-2-ONNX --ort-dylib .cache/t006/ort-cpu/onnxruntime.dll --variant q4 --threads 6 --fixture fixtures/eval-hard --json target/bench/t209-q4.json
cargo run --release -p lumen-storage --example code_upgrade -- 100000 target/bench/t209-upgrade.json
```

The small relevance fixture measures matching quality and bounded result projection; it
does not establish query latency at 100k code chunks. Fusion weights remain 1/1/2. See
ADR-036 for the schema, result/action decisions and limits. Native clipboard/Explorer
review remains in HANDOFF.md; screenshots of synthetic UI states stay in `target/t209/`.
