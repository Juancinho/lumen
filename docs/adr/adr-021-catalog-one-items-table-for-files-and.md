# ADR-021 — Catalog: one `items` table for files and apps, inventory sync, instant name provider


**Status:** Accepted (T101). Evidence: `docs/benchmarks/t101/2026-10-08-cloud-sandbox/
catalog-usr-home.json`, `lumen-catalog` tests, and Windows `docs/benchmarks/t101/
2026-10-08-joao-pc/catalog.json` (5 user folders: 26,469 entries complete, 0 blocking issues;
first sync 5.2 s, resync 3.2 s; **330 apps via AppsFolder** in 0.56 s; 25.6 MiB DB; keystroke
lookup p50 1.1 ms, p95 5.3 ms, max 10.5 ms). Full-name query finds the exact item in the top
10 for 92 % of sampled names — the misses are names shared by more than 10 items (e.g.
`desktop.ini`, camera file names); folder context in the query disambiguates them.

**Decision**

- **Provider contract** in `lumen-core::provider`: synchronous `Provider::search(&ProviderQuery,
  &CancellationToken) -> Vec<ResultItem>` with a `LatencyClass` (Instant/Fast/Semantic/Deferred);
  the coordinator (T107) owns threads. Built-in actions in `lumen-core::builtin`:
  `lumen.open`, `lumen.launch`, `lumen.reveal`, `lumen.copy-path` (executors: T109).
- **Schema (0001, pre-release edit allowed by ADR-017):** `items` gains `source`
  (files|apps), `raw_path` (exact OS path for non-Unicode names), `name_key` (folded search
  key), `launch_target`, `attributes`, `seen_scan`; table `scans`. The path is unique
  **exactly** (case-variant entries on case-sensitive dirs stay distinct, coverage) with a
  NOCASE lookup index; identity `(volume_id, file_id)` is indexed but **not unique** (hard links).
- **Inventory sync** (`lumen_catalog::sync_files`): every emitted entry is upserted (2,000 per
  transaction; metadata failures as `status='error'`, still searchable). Same path → refresh;
  unseen item with same identity + size + mtime → **move** (keeps the item id, later its
  chunks/vectors); else insert. Afterwards unseen items are removed — never after a cancelled
  pass, never under a directory that failed to list or a root that did not open.
- **Apps** (`sync_apps`): Windows AppsFolder (`lumen-windows`, Get-StartApps equivalent:
  packaged + desktop apps, launched as `shell:AppsFolder\<parsing name>`); fallback Start-menu
  shortcuts (.lnk/.url/.appref-ms). An empty discovery never wipes the app list.
- **Names**: `name_key` = NFKD, combining marks removed, lowercase, whitespace collapsed
  (`unicode-normalization`); "reunion" finds "Reunión". `CatalogProvider` (`lumen.catalog`,
  Instant): exact then prefix on `name_key`, apps first; `ResultId` = `item:<id>`.

**Evidence (2 vCPU sandbox, 245,623 entries with identity)**

- First sync 6.1 s (40k entries/s), resync 7.4 s (33k/s), 121 MiB database.
- Keystroke lookups (n=2,318, 1–8-char prefixes of sampled names): p50 0.086 / p95 0.163 ms.
- Full name returns that item in the top 10 for 88 % of sampled names (misses are names shared
  by >10 items — `python`, `__init__.py` — ranking is T102).
- Regression found and fixed: without statistics SQLite used `items_modified` for the move
  lookup (many files share an mtime) → 2.2k entries/s; `INDEXED BY items_identity` + a plan test.

**Consequences**

- T102 adds token-prefix ("code" → "Visual Studio Code"), fuzzy and path matching, and ranking
  signals; T106 recency/frequency; T207 incremental updates reuse `upsert_entries`/moves.
- `sync_files` must receive the complete root set (roots no longer configured are removed).
- Query-plan stability: catalog statements that matter keep explicit index hints or plan tests.
