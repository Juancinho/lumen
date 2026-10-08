# ADR-025 — Root search: one latest-wins search thread, merged updates as events


**Status:** Accepted (T107); merge policy amended by ADR-032 (weighted RRF, settled re-run). Code: `crates/lumen-search` (`Coordinator`, `SearchService`),
shell `search.rs`, `catalog.rs`, UI `features/root-search/useResults.ts`.

**Decision**

- **Coordinator (core, no Tauri):** providers registered once; per query they run in
  latency-class order (instant → fast; semantic/deferred only when the query is settled,
  `typing == false`). After each provider that changed the merged list an update is emitted;
  a final `done` update always closes a completed run. A failing provider is skipped and
  reported, never failing the query. Initial global order: provider-normalized confidence,
  then registration order, then the provider's own order; first occurrence of a result id
  wins; limit 30. Fusion/intent rules arrive with T205/T401.
- **One search thread, newest query wins:** `submit` cancels the running query and replaces
  any pending one (nothing queues per keystroke); ids older than the newest are dropped
  (async IPC can deliver out of order); superseded queries deliver nothing.
- **Wire:** UI numbers its queries (`search(queryId, text)` returns at once); results
  stream as app events `lumen:results {queryId, done, results}` and the UI keeps only its
  latest id. A Tauri `Channel` per call was not used: the latest-wins thread outlives calls,
  and payloads are ≤ 30 small rows. Payloads (paths, launch keys) never leave Rust.
- **UI:** keeps the previous rows until the new query's first update (no blank flash), asks
  only after its listener is registered, re-runs the query on show and on
  `lumen:catalog-changed`.
- **Catalog kept current by the shell:** a background thread syncs Start-menu apps, then
  the user's standard folders (Desktop, Documents, Downloads, Pictures, Music, Videos;
  nested/duplicate roots collapsed), 2 s after start-up and every 30 min, on its own writer
  connection (busy timeout). Incremental watching is T207; configurable roots come with the
  indexing settings.

**Consequences**

- Linux Xvfb smoke (real app, temp home): typing → `search_done` 0.4–2.2 ms in-process;
  results render. Windows keystroke→paint is measured once T104 lands (diag
  `search_done_ms` + UI paint).
- The second SQLite writer (catalog) and the settings writer contend only on rare settings
  writes; if T202's embedding queue adds a third, move writes to one storage thread.
- A full resync rewrites every item's metadata each 30 min (~3 s for 26k entries on
  joao-pc); T207 replaces it with change notifications.
