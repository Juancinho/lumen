# T208 — Windows release query syntax

100,000 synthetic catalog entries and chunks; Ryzen 5 5600H (6C/12T), 16 GB RAM,
Windows x86-64. Resident T212 Lumen PID 7404/start 18:04:57 kept running with its
beside-exe DirectML library loaded; agent did not stop it or write its database.
Compilation overlapped this loaded-machine run. See `context.json`.

| Scenario | p50 ms | p95 ms | Empty measured queries |
| --- | ---: | ---: | ---: |
| Names, ordinary | 10.475 | 13.429 | 0 |
| Names, extension/path/date filters | 10.549 | 13.086 | 0 |
| Metadata-only extension/path/date | 0.212 | 0.300 | 0 |
| Content, ordinary | 0.162 | 0.213 | 0 |
| Content, extension/path/date filters | 0.232 | 0.372 | 0 |
| Quoted content phrase + extension | 1.194 | 1.343 | 0 |
| Check 1,024 ANN IDs against metadata | 1.083 | 1.678 | — |

100 measured requests per scenario after 10 warm-ups. Parser p95 0.001136 ms per
request, measured in 100-call loops (1,000 samples). Timing is the entire provider
call except the separately reported metadata/parser microbenchmarks. Names include
existing bounded typo/token stages. `query-syntax.json` holds raw summaries.

```powershell
cargo run --release -p lumen-bench --example query_syntax -- target/t208-timing.json
```

The example creates and cleans up its own temporary SQLite database. It stores only
counts/timings in the output; every fixture name/query is generated and synthetic.
No production runtime, live catalog traversal or test-only settings are needed.

These selective queries validate overhead at 100k rows; they do not characterize
common-term worst cases, real library recall, visible first paint or CPU embedding
latency. Existing 25 ms name / 150 ms content budgets remain. Filtered semantic
overfetch caps at 1,024 candidates / 100 ms extra retrieval work and may return a
partial list for narrow filters. Delta and persisted ANN behavior are integration
tested; no new embedding model/weights/generation or full vector scan is introduced.

Native root-search keyboard/selection/action review remains in HANDOFF; the existing
frontend's 71 tests and build pass without UI/DTO/layout modifications. No visible
native overlay inspection or long GPU soak was performed in this task.
