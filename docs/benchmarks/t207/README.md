# T207 — Windows native watcher evidence

2026-10-09, Windows x86_64, 12 logical CPUs, AMD Family 25 Model 80 Stepping 0.
Release build; real native notifications on a local temporary volume. Synthetic files and
database only. The existing resident Lumen process/database were left alone; compilation
and other validation work ran concurrently, so these are loaded-machine timings.

Reproduce from the repository root (no model/download required):

```powershell
cargo run --release -p lumen-bench --locked -j 2 -- watch --json target/bench/watch.json
```

The probe seeds 10,001 inventory items, then repeats create → edit → rename → delete five
times. Each operation waits for the production bounded debounce, reconciles scoped paths,
runs the real content pass, and confirms the expected catalog/FTS state. It writes small
synthetic vectors only to check identity/preservation and pending-state behavior; it does
not measure EmbeddingGemma inference, relevance or WebView paint. Failed rename
preservation fails the command. Temporary files are removed when the probe exits.

`windows-watch.json` results:

| Measure | p50 | p95 |
|---|---:|---:|
| Mutation → lexical catalog/content ready (including 300 ms debounce) | 360.86 ms | 380.78 ms |
| Scoped reconciliation + content pass | 49.28 ms | 70.05 ms |

20 operations emitted 29 entries (including directory-mtime hints), with zero full
inventories. All five unchanged renames retained their vectors. A parked native watcher
received zero notifications and consumed 0.0 s measured process CPU over a 2.00 s idle
window (coarse process-time resolution; this is not a long resident-memory/CPU soak).

Tests separately cover folder/Unicode/case-only moves, surviving and edited hard-link
aliases, atomic replacement, known same-size/same-mtime writes, ambiguous rename + edit,
names-only content scope, marker exclusions, junction descendant hints, offline roots,
cancel safety and 4,096-path overflow. Network/USB reconnection and the visible root
query/selection/actions/preview behavior remain native checks in HANDOFF. Large catalogs,
long native storms and model throughput are outside this small freshness sample.
