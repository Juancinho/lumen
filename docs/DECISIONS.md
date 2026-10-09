# DECISIONS.md — architecture decision index

> One file per decision in `docs/adr/`. ADRs may be amended with evidence (append a dated
> note to the ADR); never silently contradict one. New ADR: next number, file
> `docs/adr/adr-NNN-short-slug.md` with `**Status:**`, **Decision**, **Consequences**, and a
> row here.

| ADR | Decision | Status |
|---|---|---|
| [ADR-001](adr/adr-001-local-first-core.md) | Local-first core | Accepted |
| [ADR-002](adr/adr-002-rust-core-tauri-react-shell.md) | Rust core, Tauri/React shell | Accepted provisionally |
| [ADR-003](adr/adr-003-sqlite-fts5-canonical-metadata-store.md) | SQLite + FTS5 canonical metadata store | Accepted |
| [ADR-004](adr/adr-004-usearch-hnsw-candidate-ann.md) | USearch/HNSW candidate ANN | Accepted for benchmark path |
| [ADR-005](adr/adr-005-embedding-runtime-abstraction.md) | Embedding runtime abstraction | Accepted |
| [ADR-006](adr/adr-006-256d-default-semantic-index-target.md) | 256d default semantic index target | Accepted provisionally |
| [ADR-007](adr/adr-007-universal-provider-result-action-domain.md) | Universal provider/result/action domain model | Accepted |
| [ADR-008](adr/adr-008-single-root-search.md) | Single root search | Accepted |
| [ADR-009](adr/adr-009-progressive-retrieval.md) | Progressive retrieval | Accepted |
| [ADR-010](adr/adr-010-one-webview-target.md) | One-WebView target | Accepted provisionally |
| [ADR-011](adr/adr-011-multi-pass-resumable-indexing.md) | Multi-pass resumable indexing | Accepted |
| [ADR-012](adr/adr-012-rewind-is-opt-in-metadata-event-memory.md) | Rewind is opt-in metadata/event memory first | Accepted |
| [ADR-013](adr/adr-013-wire-dtos-are-shell-owned-projections.md) | Wire DTOs are shell-owned projections; core types carry no serialization | Accepted |
| [ADR-014](adr/adr-014-synchronous-embedding-backend-trait.md) | Synchronous embedding backend trait; shared correctness layer in `Embedder` | Accepted |
| [ADR-015](adr/adr-015-embeddinggemma-2-runs-on-onnx-runtime.md) | EmbeddingGemma 2 runs on ONNX Runtime, CPU, q4 weights by default | Accepted |
| [ADR-016](adr/adr-016-ann-usearch-hnsw-f16-storage-cosine-m.md) | ANN: USearch HNSW, f16 storage, cosine, M=16, ef_search=256 | Accepted |
| [ADR-017](adr/adr-017-sqlite-store-bundled-3-53-wal-user.md) | SQLite store: bundled 3.53, WAL, user_version migrations, budgeted FTS5 | Accepted |
| [ADR-018](adr/adr-018-inventory-coverage-guarantee-and-stable.md) | Inventory coverage guarantee and stable file identity | Accepted |
| [ADR-019](adr/adr-019-embedding-device-policy-cpu-by-default.md) | Embedding device policy: CPU by default, accelerators only on measured proof | Accepted |
| [ADR-020](adr/adr-020-hidden-webview-trim-to-low-memory-after.md) | Hidden WebView: trim to low memory after 30 s hidden | Accepted |
| [ADR-021](adr/adr-021-catalog-one-items-table-for-files-and.md) | Catalog: one `items` table for files and apps, inventory sync, instant name provider | Accepted |
| [ADR-022](adr/adr-022-name-path-matching-tokenized-names-in.md) | Name/path matching: tokenized names in FTS5, Rust scoring, bounded stages | Accepted |
| [ADR-023](adr/adr-023-usage-signals-aggregates-only-decayed.md) | Usage signals: aggregates only, decayed frecency, learned query choices, pins | Accepted |
| [ADR-024](adr/adr-024-window-material-system-acrylic-by.md) | Window material: system Acrylic by default, Solid fallback, native corners | Accepted on measurements |
| [ADR-025](adr/adr-025-root-search-one-latest-wins-search.md) | Root search: one latest-wins search thread, merged updates as events | Accepted |
| [ADR-026](adr/adr-026-actions-ids-from-the-ui-recent-result.md) | Actions: ids from the UI, recent-result lookup, core policy, shell executors | Accepted |
| [ADR-027](adr/adr-027-indexed-locations-one-versioned-setting.md) | Indexed locations: one versioned setting, visible default exclusions | Accepted |
| [ADR-028](adr/adr-028-retrieval-chunks-128-token-target.md) | Retrieval chunks: ~128-token target, heuristic structure, bounded decoding | Accepted |
| [ADR-029](adr/adr-029-content-indexing-db-is-the-queue.md) | Content indexing: the database is the queue, vectors in SQLite, thread cap before duty cycle | Accepted |
| [ADR-030](adr/adr-030-query-lane-own-session-latest-wins-preempts-indexing.md) | Query lane: own runtime session, latest wins, preempts indexing at single-chunk boundaries | Proposed |
| [ADR-031](adr/adr-031-ann-generations-file-plus-delta-validated-against-sqlite.md) | ANN generations: derived HNSW file + exact delta, every hit validated against SQLite | Accepted |
| [ADR-032](adr/adr-032-hybrid-fusion-weighted-rrf-settled-lanes.md) | Hybrid root search: weighted RRF over names / contents / meaning, settled-query lanes | Accepted |
| [ADR-033](adr/adr-033-fusion-weights-meaning-doubled-hard-set.md) | Fusion weights from the harder set: meaning ×2; lexical-lane findings | Accepted |
| [ADR-034](adr/adr-034-model-provisioning-pinned-consented-curl.md) | Model + runtime provisioning: pinned files, explicit consent, system curl, verified atomic install | Accepted |
| [ADR-035](adr/adr-035-progressive-refinement-pinned-selection-snippets.md) | Progressive refinement: one settled update, selected row stays put, passages for content/meaning matches | Accepted |
| [ADR-036](adr/adr-036-code-context-file-identity-and-local-actions.md) | Code context on the file row, lexical metadata without re-embedding, capability-based local actions | Accepted |
| [ADR-037](adr/adr-037-native-watch-hints-scoped-reconciliation.md) | Bounded native watch hints, scoped reconciliation and safe content invalidation on the existing writer | Accepted |
| [ADR-038](adr/adr-038-optional-dedicated-gpu-indexing.md) | Opt-in dedicated-GPU indexing with CPU queries, isolated compatibility probes and CPU fallback | Accepted |
