# GTX 1650 q4 recheck — 2026-10-09

Ryzen 5 5600H (6C/12T), GTX 1650 4 GB, Windows, driver 32.0.15.9227.
Cached EmbeddingGemma 2 q4; ORT CPU 1.30 and DirectML 1.24.4. Release source
`69d741f`; the script change only hides helper windows. No live database writes.

| Device | Query p50 / p95 ms | Chunks/s | Stable | Min cosine vs CPU | GPU memory |
|---|---:|---:|---|---:|---:|
| CPU | 45.20 / 66.74 | 2.709 | yes | 1.0 | — |
| GTX 1650, DirectML | 528.10 / 728.92 | 6.071 | yes | 0.99999946 | 2296 / 4096 MiB |

GPU throughput was 2.24× CPU; 94.43% of graph nodes offloaded. Measurements use
T013's 32 synthetic ~260-token documents (two runs for stability), 40 measured queries
and the production prompts/256d normalization. These are embeddings, not whole files.

The resident Lumen indexer stayed running throughout (`context.json`, same PID/start
time). It used 220.36 CPU seconds during 77.42 seconds (about 2.85 logical cores).
Do not interpret the speedup as an idle-machine acceptance result, 128-token production
throughput or an ETA. Compile work had finished before measurement. No iGPU was probed.

The unchanged policy chooses CPU for queries in every scenario; GPU indexing only in
the explicit Turbo profile because the ordinary VRAM cap is 1536 MiB. This is a policy
simulation, not an option in the then-current desktop app. T212 adds the requested opt-in
and allows available dedicated VRAM without changing default Balanced behavior.

Reproduce with the existing local T006 cache:

```powershell
cargo build --release -p lumen-bench --features directml --locked -j 2
scripts\t013\run-windows-device-probe.ps1 -SkipBuild -OutDir target\bench\gpu-recheck
```

`probe-cpu.json`, `probe-dml-high.json`, `policy.json`, `gpus.json` and `context.json`
are counts/metrics/hardware only. The broader T014 runtime/thread sweep remains pending.
