# IMPLEMENTATION_NOTES.md

## Workspace direction

Recommended:

```text
/apps/desktop          Tauri + React shell
/crates/lumen-core     domain types, capability model
/crates/lumen-command  providers/actions/workflows coordination
/crates/lumen-search   query parsing/fusion/ranking
/crates/lumen-storage  SQLite/FTS
/crates/lumen-vector   ANN wrapper
/crates/lumen-indexer  enumeration/jobs/watcher
/crates/lumen-extract  text/PDF/image/media extraction
/crates/lumen-embedding model backend
/crates/lumen-windows  Windows adapters
/crates/lumen-bench    benchmark harness
```

Exact crates may consolidate initially. Preserve dependency direction over crate-count purity.

## React shell

Prefer feature folders:

```text
features/root-search
features/results
features/action-panel
features/preview
features/index-status
features/settings
```

Do not implement core business rules twice in TypeScript and Rust.

## State

Keep transient UI state local. Rust owns canonical search/index state. Avoid a giant global frontend store unless profiling/complexity justifies it.

## Result rendering

Use discriminated kinds only for presentation differences. Do not fork entirely separate list systems per provider.

## Native integration

Wrap Windows-specific APIs behind `lumen-windows`. Avoid Win32 handles leaking into domain types.

## Model lifecycle

Treat text, vision and audio capability loading independently where backend allows. Warm text path is more valuable than keeping all modalities resident.

## Indexing scheduler

Queues must be bounded. Suggested priorities:

1. interactive query embedding;
2. explicit user index request;
3. changed/recent small text/code;
4. docs/images;
5. historical media;
6. refinement/re-OCR.

Persist job checkpoints so shutdown never means starting the corpus over.
