# T006 — EmbeddingGemma 2 runtime benchmarks

Decision: ADR-015 in `docs/DECISIONS.md`.

Each subfolder is one machine/run: `machine.json` (hardware, drivers, power), one `<run>.json`
per configuration (`lumen-bench embed` report, schema_version 1), `<run>.log`, `runs.json`
(status, NVIDIA VRAM peak) and `summary.md`.

Run names: `cpu-<variant>[-t<threads>]`, `dml-<high|low|adapter>-<variant>` (DirectML GPU chosen
by performance preference or DXGI index). Variants are the onnx-community export files:
fp32 `model`, fp16 `model_fp16`, q8 `model_quantized`, q4 `model_q4`, q4f16 `model_q4f16`.

## Contribute a run (any Windows 10/11 x64 PC)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\t006\run-windows-bench.ps1 -Download
```

Needs Rust (rustup) + MSVC build tools; downloads ~2.3 GB into `.cache\t006` (git-ignored),
verifies SHA-256, takes 20–45 min. Keep the PC plugged in and idle. `-Only a,b` reruns a subset.

## Reading the numbers

- *query*: warm single short query (the interactive path; budget 60/120 ms p50/p95).
- *~128-tok*: one ~100-word input, comparable to Google's published 128-token figures.
- *docs/s*: 200-word chunks (~260 tokens), batch 8 (the indexing path).
- *GPU nodes %*: share of graph nodes ONNX Runtime placed on DirectML; the rest run on CPU.
- *min cos*: worst cosine vs the fp32 Python reference over the 60-text fidelity corpus.
