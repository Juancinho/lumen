# T112 — manual exclusion cleanup

2026-10-10, JOAO-PC: Ryzen 5 5600H (6C/12T), 16 GB RAM, GTX 1650, Windows 11 Pro
build 26300. Optimized Rust release, no model or GPU needed. The resident T303 app and
another user-owned GPU/progress session were active; loaded-machine evidence, not idle
or whole-app latency. No live database/settings/files used or altered by this agent.

`exclusions.json` uses 100,000 synthetic files (equal js/json/log/md), one text chunk,
256d f16 vector and usage aggregate each. Local temporary SQLite/FTS fixture, same
production exclusion matcher, 512-row keyset pages/deletion transactions, simultaneous
independent CatalogProvider reads. The reader's known retained Markdown file must remain
the first hit throughout; existing fuzzy alternatives are allowed.

| Operation | Result |
|---|---:|
| Remove 75,000 explicitly excluded files with cascades | 16,471.34 ms |
| Repeat cleanup across 25,000 retained files (0 removed) | 232.78 ms |
| Concurrent retained-name queries | 641 |
| Provider query p50 / p95 / p99 | 25.44 / 32.99 / 43.11 ms |
| Maximum sampled provider query | 55.95 ms |

Final-source run; all assertions passed: 75k removed, 25k items/vectors retained, same active generation,
known retained file stays first for every query. This is a crowded synthetic numeric-name
corpus with usage on every file; these timings do not replace T303's quieter hot-name
baseline or predict a real library's completion time. No end-to-end/WebView measurement,
model inference, idle CPU, peak/private memory or native tray/picker/DPI claim.

The production cleanup runs on the existing background writer and cooperatively cancels
between pages; already-running native embedding must finish first. Including again
restores affected data through normal inventory/content/embedding work. Unit tests also
cover exact-file disk safety, offline/cancelled rows, application/directory preservation,
multi-page cascades and undo; native application review is retained in HANDOFF.

Reproduce from the repository root, using a task-specific cache (another checkout was
reusing target/ and replacing dependency metadata during this session):

```powershell
cargo run --release --locked --target-dir target/t112-build -p lumen-bench --example exclusions -- target/t112-exclusions.json
```

The example rejects debug builds, generates only temporary synthetic data and removes
its own fixture on completion. Initial fixture creation is outside the cleanup timing.
No fixture paths or user content are stored in the report. One reported final-source run;
no repeatability claim or controlled before/after ranking performance comparison.
