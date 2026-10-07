# T008 — ANN (USearch/HNSW) benchmarks

Decision: ADR-016 in `docs/DECISIONS.md`. Reports are `lumen-bench ann` JSON (schema_version 1).

- `2026-10-08-cloud-sandbox/`: 2 vCPU Linux sandbox. `ann-100k.json` (all scalars),
  `ann-1m.json` (f32/f16/i8), `ann-100k-uniform.json` (degenerate random data, for reference).

Windows latency run (no downloads, ~10-20 min):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\t008\run-windows-ann.ps1
```

Real-embedding recall: embed a corpus with `scripts/embedding/embed_corpus.py`, then
`lumen-bench ann --vectors docs.f32,queries.f32`.
