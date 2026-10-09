# ADR-036 — Code context on the file row, lexical metadata and local actions

**Status:** Accepted (T209 implementation; native action review pending). Amends
ADR-028/032/033 for code metadata and results. Evidence:
`docs/benchmarks/t209/2026-10-09-joao-pc/` (Windows release builds, 12 logical CPUs).

**Decision**

- Keep one entity per file (`item:<id>`) across names, contents and meaning. A matched code
  chunk supplies a boxed `CodeTarget` (file, optional symbol, language, optional repository,
  normalized-text byte offsets, bounded passage) and `ResultKind::Code`. Missing symbols
  retain a file-level code row. Exact filename navigation retains its file presentation;
  fusion preserves code actions/context even when the name lane wins the copy.
- Migration **0004** adds item language/repository/discovery-path metadata and a derived
  `chunks.search_context` column to external-content FTS5. Code chunks index name tokens,
  nearest-folder tokens, filename and language alongside the original passage/symbol.
  Prose/data retain an empty context. Original snippets still come from the text column.
  No chunk identities, extracted text, vector sequences, embedding prompts, extractor
  version or generations change. FTS is rebuilt atomically once during the upgrade.
- The existing background content pass backfills metadata for old code without re-reading
  bodies or embedding again. It also refreshes metadata after content changes and moves.
  A move immediately clears the old repository/language and replaces path tokens; trigger
  updates read the current item to avoid SQLite trigger-order dependence.
- Repository discovery checks the nearest `.git` directory or worktree marker file, at
  most 32 ancestors, with a per-pass cache bounded to 4,096 directories. It reads no Git
  contents and does not follow marker symlinks. Unknown/inaccessible/non-Unicode repository
  roots omit the repository action; lossless file paths still open/reveal normally.
  There is no filesystem traversal in a query and no new watcher (T207 remains next).
- The UI receives a typed display projection (symbol/language/repository name), never
  executor paths, offsets or commands. Existing 52 px rows show `symbol · filename` and
  the matching passage; hover carries language/repository/location. No new surface/tokens.
- Enter opens the file with its registered handler; Ctrl+Enter reveals the file;
  Ctrl+K additionally offers **Copy symbol** and **Reveal repository in Explorer** only
  when their capabilities are present. These are safe, ids-only actions authorized through
  the existing result lookup/core policy. Alt+Enter shows the indexed matching passage,
  including matches beyond the original 16 KB preview-read window. No guessed editor CLI
  or raw-offset-to-line conversion: editor selection and a precise editor transport need
  their own future contract. An open preview refreshes when the same file gains code
  context or a different matching passage; late answers are discarded. Existing
  Escape/focus/selection behavior is preserved.
- Metadata and lexical code search work offline without an installed model. All reads,
  index contents and clipboard actions are local; no additional network or telemetry.
  Names remain the instant lane, contents remain settled with the existing 150 ms provider
  budget, and fusion weights remain **1/1/2**. Tree-sitter is not justified by this evidence.
- Settings database open/migration executes on a named startup worker. Initial setup and
  first show wait for its canonical schema before readers register; the one-time upgrade
  can therefore extend cold startup, while disk/CPU migration work runs off the UI thread.
  Normal startup does not rebuild FTS. No extra process/WebView is introduced.

**Evidence**

- Same Windows synthetic corpus: 162 documents, 177 chunks, 49 queries. The content lane
  is model-independent, so before/after mock runs measure the lexical change directly:
  code top-1 **0.500 → 1.000** (six queries), code NDCG@10 **0.617 → 0.902**, overall
  content NDCG@10 **0.558 → 0.592**. Names and mock semantic metrics are unchanged.
- Real CPU q4, six threads: fused NDCG@10 **0.964**, top-1 **0.959**; code top-1 **1.000**,
  code NDCG@10 **0.979**. Content p50/p95 **0.85/2.32 ms**, semantic **33.52/65.67 ms**.
  These are fixture-sized timings, not a 100k-query latency claim; the first semantic
  query includes model loading (p99/max 1.8 s). Compare lexical metrics with the local
  before run; ADR-033's earlier real-model run was on different hardware.
- Synthetic 100k code chunks plus 100k f16 vectors: v3→v4 migration **1,097 ms**;
  subsequent metadata/FTS backfill **1,597 ms** (one deliberately oversized file).
  Every vector/sequence was retained and FTS external-content integrity passed.
- Regressions cover migration/backfill, stale repository/path tokens on moves,
  lexical/semantic code targets, capability authorization, fusion ties, wire privacy,
  matching-passage Quick Look and accessible result rows. Browser visual QA: light/dark,
  760/800 px rows, long symbols/names, symbol-free fallback, Action Panel and preview.

**Consequences**

- New code metadata costs derived FTS space, not a new embedding generation. Backfill
  completes on the next content pass; before it, original lexical/semantic content remains
  searchable. The native Open/Copy symbol/Reveal repository flow still needs human review.
- ADR-033's log/data `two_of` finding is unchanged; this task addresses code context and
  language without retuning unrelated fallback behavior or weights.
