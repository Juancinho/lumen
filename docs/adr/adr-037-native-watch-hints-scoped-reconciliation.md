# ADR-037 — Native watch hints and scoped reconciliation on the existing writer

**Status:** Accepted (T207; native overlay/reconnect review pending). Amends
ADR-018/021/027/029/031/036 for incremental file changes. No schema, extractor, embedding
space, UI contract, ranking, process boundary or resource-policy changes.

**Evidence and reason**

The catalog previously refreshed only at startup, settings edits and every 30 minutes.
That leaves new documents absent and old passages searchable while users edit files.
Existing stable identity, WAL, the persistent embedding queue and SQLite validation of
ANN hits already supply the required persistence and safety mechanisms.

Use the platform notification backend in `notify 8.2.0` (Windows
`ReadDirectoryChangesW`), behind a shell-independent adapter in `lumen-indexer`.
Notifications are hints: editors save differently, native buffers can overflow, a watch
can disappear with its directory, and network/unsupported filesystems may not deliver
events. See the [notify documentation](https://docs.rs/notify/8.2.0/notify/),
[event contract](https://docs.rs/notify-types/2.1.0/notify_types/event/struct.Event.html),
and pinned source in Cargo.lock. Register roots recursively and their parents
nonrecursively, without following descendant links. The existing periodic inventory
remains the recovery path; no polling watcher or USN journal is added.

**Decision**

- The native callback does no I/O or SQLite work. It filters access events, paths outside
  the selected roots, configured excluded ancestors, and Lumen's own app-data writes;
  then coalesces at most 4,096 distinct paths under the catalog control lock. Ordinary
  events settle after 300 ms quiet with a 2 s deadline during continuous writes. Overflow,
  native rescan flags/errors and root lifecycle changes request a recovery inventory,
  delayed 5 s to bound repeated recoveries. No debounce timer runs while idle.
  Already known hints remain bounded and are applied before a recovery inventory, so
  recovery does not discard a notified same-metadata write.
- Only the existing catalog/indexing thread owns writes. Native events cancel an
  embedding slice at its existing batch boundary; initial/full inventories finish while
  more hints accumulate. Register before inventory and re-register on each full pass,
  including location edits and reconnect recovery. An unavailable registration is reported
  by count and retried at recovery/startup/30-minute inventory; no paths/content are logged.
- Reconcile a file write by probing one entry. Creation/rename/deletion can reconcile a
  subtree. Reuse inventory metadata, exclusions, cloud-placeholder policy and identity;
  ancestor exclusions and links apply even when an event names a descendant directly.
  Project/venv/repository marker changes reconcile the containing subtree. Upsert new
  paths before pruning old scoped paths, using 2,000-row transactions/pages. Unrelated
  roots are never pruned. Failed root/listing verification and cancellation retain entries.
- Only move from an independently verified absent old path, with matching identity and
  file size/mtime/creation metadata. Folders use identity + creation time because their
  mtime changes when children change. A surviving hard link remains a distinct item;
  explicit writes invalidate all known aliases of the same physical file. For Windows
  case-only renames, check exact directory-entry spelling rather than treating
  case-insensitive path resolution as evidence that both names survive.
- Metadata/identity/extension changes and known write hints invalidate old chunks and
  vectors atomically with the catalog refresh. Same-size/same-mtime notified writes still
  invalidate. New chunks become pending in the existing persistent queue; old mmap ANN
  hits fail existing SQLite sequence validation immediately. No manual index reset or
  generation change is required.
- Windows can add a generic modified hint to a rename. If identity and content metadata
  still match, bounded extraction/chunk comparison against the old indexed representation
  resolves this ambiguity before the transaction: retain chunks/vectors only if they are
  identical. Reads require the location's content consent and a non-placeholder file,
  with the existing 4 MiB extractor cap. A concurrent edit, failed comparison/read or
  unknown extractor representation takes the safe invalidation path. No file body is
  hashed on every scan and no body is read by the native callback.
- Notify the existing payload-free `lumen:catalog-changed` event after scoped changes and
  content/vector commits while the overlay is shown. Hidden catalog changes do not wake
  JS search or query inference; the existing show event refreshes on the next invocation.
  The existing root query refresh, result IDs, selected-row stability,
  actions and previews remain the interaction path. Name freshness works offline with
  no model; semantic freshness follows the resource-controlled embedding queue.

**Validation and limits**

`lumen-bench watch` uses a temporary synthetic 10,001-item catalog, real native events,
20 create/edit/rename/delete operations and synthetic vectors to verify preservation.
Committed counts/timings and reproduction are in `docs/benchmarks/t207/`; they measure
mutation-to-lexical freshness, reconciliation and a parked watcher, not model throughput
or full overlay paint. Tests cover moves, aliases, replacement, same-metadata writes,
exclusions/markers, offline/cancel safety and case renames.

Native notifications are not durable. Recovery inventories detect identity/metadata
changes; an event lost while a writer also deliberately preserves all metadata can still
require explicit reindexing. Cross-volume moves or uncertain/recycled identities create
fresh items/chunks. Large subtree moves remain background work. Start-menu app discovery
still uses the existing startup/periodic pass. Long disconnected-volume and visible
overlay/action/selection checks remain native REVIEW items; do not claim them from the
synthetic benchmark.
