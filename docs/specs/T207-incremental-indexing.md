# T207 — Incremental file indexing

## 0. Implementation status (2026-10-09)

Implemented in `lumen-indexer::watch` / `scan_changed`, `lumen-catalog::files::sync_changes`,
`lumen-storage` inventory/content helpers and the existing desktop catalog thread.
ADR-037 owns the watcher/reconciliation decision. T207 is REVIEW pending native overlay,
location editing and disconnected-volume checks. Evidence: `docs/benchmarks/t207/`.

## Scope and behavior

- Created files appear in ordinary root search after a native event batch; supported text
  in locations with Index file contents enabled gains lexical passages and pending vectors.
- Modified files retire old indexed passages/vectors, including known same-size/same-mtime
  writes and editor replacement saves. Semantic readiness follows the existing queue,
  pause, query preemption and power/memory policy; it never delays filename results.
- Rename/move within a volume preserves identity and embeddings when metadata/content
  prove unchanged. Folder descendants move too. Code path/repository context refreshes
  through T209. Cross-volume or uncertain moves safely reindex.
- Deletion removes scoped catalog items/chunks/vectors and invalidates old ANN hits through
  SQLite validation. Offline/unverified roots retain their items; cancellation retains
  unverified rows. Hard links remain separate names; writes retire content for every alias.
- Exclusions apply to ancestors, marker changes reconsider sibling build/venv trees, and
  descendant symlinks/junctions are not traversed. No cloud placeholder body is read.

## Product and resource contract

No mode, surface, keyboard binding or new action is added. Root search keeps Enter/Open,
Ctrl+Enter/Reveal, Ctrl+K/actions and Alt+Enter/Quick Look with the existing IDs and selected
row behavior. Files and lexical contents work offline; semantic search needs an installed
local model and completed queue entries. No content, paths, queries or vectors leave the
device; watcher callbacks and diagnostics carry no text to the UI/logs.

Queue: 4,096 coalesced paths; 300 ms quiet, 2 s maximum delay for ordinary active storms.
Loss/overflow: 5 s recovery delay plus existing startup/settings/30-minute full inventory.
All filesystem, extraction and database work runs on the existing background writer.
No idle polling backend or extra UI/DB process. Pending events preempt embedding at batch
boundaries. Mutations should become lexically searchable within 1 s on ordinary local
files after a batch can run; this is a validation target, not a change to query-latency
budgets. Subtree scans, startup scans and resource-paused embeddings can take longer.

## Validation

Run `cargo xtask test --locked`, the full lint/format/architecture/frontend gate, and
`cargo run --release -p lumen-bench --locked -- watch --json target/bench/watch.json`.
The benchmark creates/deletes only its own temporary synthetic directory/database.
Its vectors validate queue/preservation state, not semantic quality. Windows evidence and
remaining native checks are recorded in `HANDOFF.md`; photos remain name-only until T303.
