# ADR-017 — SQLite store: bundled 3.53, WAL, user_version migrations, budgeted FTS5


**Status:** Accepted (T007), implements ADR-003. Evidence:
`docs/benchmarks/t007/2026-10-08-cloud-sandbox/storage-100k.json` and `lumen-storage` tests.

**Decision**

- **Bundled SQLite** (`rusqlite` 0.40 `bundled`, SQLite 3.53.2 with FTS5): identical engine on
  every machine; no dependency on a system SQLite.
- **One writer + N read-only readers, WAL, `synchronous=NORMAL`, `foreign_keys=ON`.** A reader
  is never blocked by an open write transaction (tested). NORMAL can lose the last
  transactions on power loss but never corrupts; the index is rebuildable.
- **Migrations:** embedded SQL files, forward-only, versions contiguous from 1 in
  `PRAGMA user_version`, one transaction per step (a failing step leaves no partial schema);
  a database newer than the binary is refused, readers refuse unmigrated databases.
  `0001_initial`: `items` (stable identity + case-insensitive unique path), `chunks`
  (`embedding_generation` for T203), `chunks_fts`, `settings` (JSON), `usage_events`.
- **FTS5:** external-content table over `chunks` kept in sync by triggers;
  `unicode61 remove_diacritics 2` (case/accent-insensitive: "reunion" finds "reunión");
  prefix indexes `3 4`; bm25 ranking; snippets marked with U+E000/U+E001.
- **User input is never raw FTS syntax:** `FtsQuery::from_user` quotes every term, keeps
  `"phrases"`, makes the last term a prefix only while typing and only from 3 characters
  (`MIN_PREFIX_CHARS`).
- **Interactive queries carry a `SearchBudget`** (deadline and/or `CancellationToken`)
  enforced by SQLite's progress handler; an exceeded budget returns `Interrupted`, never a
  late or partial answer.

**Evidence (100k chunks × 120 words, 2 vCPU sandbox, worst-case 50-word vocabulary)**

- Insert 8.8k chunks/s (16.9k without prefix indexes) — far above embedding speed.
- 156 MiB database; path lookup p50 2 µs.
- Per-keystroke FTS p50 0.05 ms but p95 73 ms / max 115 ms when common terms force bm25 over
  most rows; with a 20 ms budget p95 20.1 ms / max 20.4 ms (73 of 852 interrupted).

**Consequences**

- The coordinator (T107) gives FTS a per-keystroke budget and cancels stale queries;
  filename/app results (T102) cover keystrokes where FTS is interrupted or skipped.
  **Interruption affects one keystroke's query only, never the index:** when typing pauses,
  the final query is re-issued with a generous budget so complete lexical results always
  appear (T107 must test this).
- Very common terms remain the cost driver; T205 may drop high-document-frequency terms
  (fts5vocab) when selective terms exist. **Measured (bench schema v2, 100k chunks, sandbox):**
  final queries whose every term is in ~all chunks take p50 106 / p95 117 ms with 50 hits —
  the worst case (50-word synthetic vocabulary). It scales with matching rows, so at 1M chunks
  T205/T107 must either drop high-df terms or bound the final query too. Earlier "final query"
  numbers (0.05 ms) timed queries with no hits and were not evidence.
- `0001_initial.sql` may still change until the first release; afterwards only new migrations.
- **T016 correction (bench schema v3, Zipf corpus):** the earlier per-keystroke numbers timed
  mostly hit-less queries. With every realistic query term present in a Zipf(1) corpus of
  160 words (`docs/benchmarks/t016/2026-10-08-cloud-sandbox-storage-100k.json`, 100k chunks,
  2 vCPU sandbox): per-keystroke FTS p50 **13.3 ms**, p95 **67.6 ms**, max 120 ms with 47
  hits on average; under the 20 ms budget **374 of 852 keystroke queries are interrupted
  (44 %)**; realistic final queries p50 6.1 / p95 66 ms (37 hits avg, 1 of 32 with none);
  vocabulary finals p50 43 / p95 90 ms. Insert 6.9k chunks/s, 148 MiB.
  **Consequence for M2 (T205/T206):** chunk FTS cannot be an every-keystroke lane at 100k+
  chunks on modest CPUs. Keep names (T102, p95 ≈ 5 ms) as the instant lane; run content FTS
  when typing pauses (same settle point as semantic, `typing == false` in the coordinator)
  with a generous budget, skip short last tokens, and drop high-document-frequency terms.
  Re-measure on Windows with `lumen-bench storage --chunks 100000`.
