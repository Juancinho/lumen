# ARCHITECTURE.md — system architecture

## 0. Implementation status (2026-10-08)

Built (M0 + M1, see `PROJECT_STATE.md`): one resident process — Tauri shell
(`apps/desktop`) over shell-agnostic crates; catalog (files + Start-menu apps) in SQLite
(ADR-017/021); name matching with usage priors (ADR-022/023); root-search coordinator with a
latest-wins search thread (ADR-025); built-in actions behind the core policy (ADR-026);
embedding backend on ONNX Runtime with a device policy (ADR-014/015/019); ANN wrapper
(ADR-016); extraction/chunking (ADR-028) and the content pass + persistent embedding queue
with vectors in SQLite (`lumen-content`, ADR-029), run by the catalog thread under the
device policy (model via env until T210). Not built yet:
persistent ANN generations (T203), semantic/hybrid lanes (T204/T205), watcher (T207).
The crate/module layout lives only in `docs/DEVELOPMENT.md` §2; decisions in
`docs/DECISIONS.md` (one file per ADR in `docs/adr/`).

## 1. Architectural goals

Lumen must optimize for:

- sub-second and preferably sub-150ms warm semantic interactions;
- instant perceived response through progressive retrieval;
- local execution;
- crash-safe incremental indexing;
- low idle CPU;
- bounded memory;
- easy replacement of inference runtime;
- testable ranking and extraction;
- a UI that never waits on background work;
- straightforward handoff between coding agents.

## 2. High-level process model

Recommended production shape:

```text
┌───────────────────────────────────────────────────────────────┐
│ Lumen desktop process (Tauri)                                │
│                                                               │
│  React UI                                                     │
│      │                                                        │
│      ▼                                                        │
│  Search coordinator (Rust) ─────► lexical providers           │
│      │                     └────► semantic query service       │
│      │                                                        │
│      ├────► SQLite metadata/FTS                               │
│      └────► USearch vector index                              │
│                                                               │
│  Background scheduler                                         │
│      ├────► enumerator/watcher                                │
│      ├────► extractors                                        │
│      └────► embedding/index writer                            │
│                                                               │
│  Windows integration: hotkey, tray, DWM, shell open/reveal    │
└───────────────────────────────────────────────────────────────┘
```

The initial implementation can run as one resident process with isolated worker threads/tasks. Introduce a separate worker executable only if crash isolation, media decoding or runtime constraints justify it. Do not create IPC complexity before evidence.

## 3. Workspace layout

The authoritative crate and module layout is `docs/DEVELOPMENT.md` §2 (kept current by
each task). Crates still to come are created by the task that needs them (e.g.
`lumen-extract` with T201). Keep Tauri commands grouped by feature, never in one file.

## 4. UI ↔ Rust boundary

The UI should receive typed result models and state snapshots. It should not know SQLite schemas, HNSW details or model runtime details.

Preferred commands/events:

```text
show_overlay()
search(query, request_id, context) -> immediate lexical batch
semantic_results event(request_id, batch)
open_result(result_id)
reveal_result(result_id)
preview_result(result_id)
find_similar(result_id)
get_index_status()
update_settings(patch)
pause_indexing()
resume_indexing()
```

Use request IDs/cancellation tokens. Results arriving for stale queries must be ignored.

Do not send huge file bodies across the Tauri boundary. Preview should use a controlled resource URL, stream, or narrow read command.

## 5. Canonical domain identifiers

Use separate identifiers:

- `ItemId` — logical filesystem/application item;
- `ChunkId` — indexed searchable sub-part;
- `VectorId` — ANN label tied to a chunk;
- `IndexGeneration` — vector/index schema generation;
- `QueryId` — one UI search request.

For filesystem items on Windows, prefer stable identity based on volume + file ID where accessible. Fall back to canonical path only when needed. This reduces duplicate reindexing on rename.

## 6. Storage

### SQLite responsibilities

- item metadata;
- paths and aliases;
- chunk metadata and snippets;
- extraction version;
- embedding/index generation mapping;
- FTS5 lexical content;
- app catalog;
- settings;
- local usage/recent events;
- errors and retry state;
- saved searches/pins.

Use WAL mode. Migrations are versioned and tested.

### Vector index responsibilities

- ANN vectors only;
- mapping `VectorId -> ChunkId`;
- generation metadata;
- persisted index file(s), preferably memory-mappable.

Do not make the ANN index the source of truth. SQLite remains canonical; vector index can be rebuilt.

## 7. Schema

The literal schema is `crates/lumen-storage/migrations/` (ADR-017; append-only migrations):
`0001_initial.sql` — `items` (files, folders and apps: stable identity, exact +
case-insensitive path, name tokens, status), `scans`, `chunks` + `chunks_fts`, `names_fts`,
`settings`, and the usage aggregates `usage_stats`, `query_choices`, `pins` (ADR-023 — no raw
event log); `0002_content_and_vectors.sql` — per-item content state, `generations`, and
`chunk_vectors` (f16 vectors per generation: the durable embedding results, ADR-029).

Large binary previews/thumbnails should not be stored directly in SQLite unless benchmark evidence favors it. Prefer a bounded cache directory with content-addressed keys.

## 8. Embedding abstraction

`EmbeddingBackend` (crate `lumen-embedding`) is **synchronous** (ADR-014): backends embed a
batch of already-formatted inputs; the shared `Embedder` adds prompts, batching,
cancellation, shape/NaN checks and the 768→256 truncation + L2. Callers own threads.
Image/audio/video methods arrive with their tasks (T303, T701, T702).

### Backends

- `lumen-embedding-ort` — EmbeddingGemma 2 on ONNX Runtime, CPU + q4 by default (ADR-015);
  DirectML only behind a feature for diagnostics.
- `MockBackend` — deterministic test embeddings.
- No Python backend in any shipped path (`scripts/embedding/` only produces references).

### Model configuration

Store in index metadata:

- model identifier/version;
- modality configuration;
- output dimension;
- normalization setting;
- scalar storage type;
- task prefix/version;
- preprocessing version.

Any incompatible change creates a new `IndexGeneration` rather than silently mixing vectors.

## 9. EmbeddingGemma 2 defaults

Model capabilities relevant to Lumen:

- unified text/code/image/video/audio embedding space;
- native 768d output;
- supported truncations 512/256/128 via Matryoshka Representation Learning;
- 8K context;
- selective modality encoders.

Default Lumen profile:

- truncate to 256d;
- re-normalize after truncation;
- use the retrieval prompts verified in ADR-015 (`PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1`);
- keep text path warm while tray resident;
- load vision/audio encoders only during relevant indexing/query operations;
- store vectors as f16 in USearch HNSW, cosine (ADR-016); index space key per ADR-014.

## 10. Search pipeline

```text
Query arrives
   │
   ├─ parse deterministic operators
   ├─ name/path search (every keystroke) ─► UI batch A
   │      typing settles (~50–90 ms)
   ├─ content FTS (budgeted) ─────────────┐
   └─ semantic embedding ─────────────────┤
                  │
                  ▼
            vector ANN search
                  │
                  ▼
         hybrid RRF + priors
                  │
                  ▼
          UI semantic batch B
```

Important: do not await semantic embedding before sending lexical results. Content FTS is
not an every-keystroke lane: at 100k chunks it costs 13/68 ms p50/p95 (T016, ADR-017 note),
so it runs on the settled query with a generous budget; names (ADR-022, p95 ≈ 5 ms) carry
every keystroke. The coordinator already gates non-instant providers on `typing == false`
(ADR-025).

## 11. Ranking

Use Reciprocal Rank Fusion as the initial robust strategy because lexical and vector scores are not naturally calibrated.

Candidate sources:

- exact/prefix filename;
- fuzzy filename/path;
- FTS5 BM25;
- vector ANN;
- app provider;
- optional recent/pinned provider.

Then apply bounded priors:

- exact filename/path boost;
- pinned boost;
- recency/frequency boost;
- active project/root scope boost;
- type compatibility with query filters.

Do not let recency overwhelm strong semantic/exact relevance.

Add a learned reranker only after a relevance dataset demonstrates a clear need.

## 12. Indexing architecture

Pipeline stages:

```text
enumerate/change event
  -> metadata diff
  -> extraction job
  -> chunk job
  -> embedding job
  -> vector write
  -> commit searchable generation metadata
```

Each stage has:

- bounded queue;
- cancellation;
- retry policy;
- metrics;
- priority.

Interactive search has higher priority than all indexing work.

## 13. Concurrency

Principles:

- UI/event loop: never performs blocking disk or inference work;
- search lexical path: dedicated lightweight DB pool;
- semantic query embedding: latency-priority lane;
- background embedding: throughput lane;
- extraction: bounded CPU/I/O pool;
- preview generation: medium priority;
- media indexing: lowest priority and pauseable.

If the embedding runtime does not support safe concurrent calls, serialize inside the backend and prioritize query embeddings above batch indexing.

## 14. Crash safety

- SQLite transaction wraps metadata state transitions.
- Vector index files are generation/versioned.
- Build a new generation to a temporary path and atomically promote when complete where possible.
- On startup, reconcile `chunks` marked embedded but absent from active vector generation.
- Corrupt vector index must degrade to lexical search and offer rebuild.
- Indexing errors never prevent the overlay from opening.

## 15. Filesystem watcher

Initial implementation: reliable recursive watcher + periodic reconciliation scan.

Advanced Windows optimization: NTFS USN Journal for fast delta catch-up after sleep/offline periods. Implement only after correctness baseline.

Rename handling should use stable file IDs when possible.

## 16. Windows integration

Split today: the shell (`apps/desktop/src-tauri`) owns the global shortcut, tray, placement,
focus, WebView2 lifecycle and action executors; `lumen-windows` owns OS adapters the core
uses (AppsFolder enumeration) and the window-material plan + DWM calls (ADR-024);
`lumen-indexer` owns file identity. Move a piece down into `lumen-windows` when a second
consumer needs it. Target responsibilities of `lumen-windows`:

- global shortcut registration;
- active-monitor placement;
- focus/foreground behavior;
- tray integration;
- shell open/reveal/open-with;
- file IDs/volume IDs;
- system theme/accent/reduced-motion/high-contrast detection;
- DWM/Mica/Acrylic integration;
- power/battery status;
- later: OCR and Media Foundation.

Keep Windows-specific code out of search/storage crates.

## 17. Configuration

Use one typed settings model with migrations. Example groups:

- shortcut;
- indexed roots;
- exclusions;
- search quality profile;
- background indexing policy;
- launch at startup;
- appearance;
- privacy/clipboard opt-in;
- thumbnail cache cap.

Do not use scattered JSON files for subsystem-specific settings.

Indexed locations and exclusions are one typed, versioned settings value
(`index.locations`, T111 spec `docs/specs/T111-indexed-locations.md`); a saved list is never
silently replaced (same rule as the shortcut, T003).

## 18. Observability

Local structured logs:

- query ID;
- latency breakdown;
- result source counts;
- index queue depth;
- extraction failures;
- model load/unload;
- index generation changes.

Never log full query text or file content by default in release builds. Developer mode may opt into verbose logs locally.

## 19. Dependency philosophy

Prefer mature, narrow dependencies. For every native binary/runtime dependency document:

- license;
- redistribution terms;
- Windows architectures;
- update strategy;
- binary size;
- security maintenance.

Do not add a large framework merely to solve one small OS API.
## 20. Command-center architecture

### A. Shell independence is mandatory

The current shell is Tauri + React/TypeScript, but the dependency direction is strictly:

```text
UI shell / Tauri adapters
        ↓
application coordinator
        ↓
domain/core crates
        ↓
storage / search / index / OS adapter interfaces
```

`lumen-core`, search, indexing and provider/action domain code must never import Tauri/React/WebView concepts. A future FastFrame/egui or other shell can be evaluated without rewriting the core.

Do not split into multiple executables merely to prove this separation. One process is preferred initially; architectural independence is achieved through module/crate boundaries.

### B. Universal command architecture

Lumen is no longer modeled as `SearchEngine -> FileResult` only.

Conceptual internal layers:

```text
Root Query
   │
   ├── Intent/parser
   │
   ├── Provider coordinator
   │     ├── app/file provider
   │     ├── lexical provider
   │     ├── semantic provider
   │     ├── calculator/system providers
   │     └── later productivity/context providers
   │
   ├── normalization + global ranking
   │
   └── ResultItem[]
           │
           └── Action registry → contextual ActionDescriptor[]
```

Workflows later compose actions. The public extension SDK is explicitly deferred.

### C. Provider latency classes

Providers declare a rough class:

- `instant`: exact apps/files, calculator parsing, cached snippets;
- `fast`: SQLite/FTS/settings/history;
- `semantic`: query embedding + ANN;
- `deferred`: heavy context/media/network opt-in.

The coordinator may skip or delay providers based on query intent and cancellation state. Never wait for all providers before painting useful results.

### D. Result/action boundary

Use the canonical model in `COMMAND_MODEL.md`. The UI asks for valid actions; it does not hard-code business rules like "PDFs have these seven actions" where that rule belongs in Rust/domain code.

The primary action is safe and predictable. Destructive actions are never primary.

### E. Workflow boundary

The workflow engine executes registered actions using typed inputs/capabilities. It must not bypass permission logic by directly calling random shell code. See `EXTENSIONS_AND_WORKFLOWS.md`.

### F. Context/memory architecture

Temporal/context features use a separate local event store/table family linked to stable item IDs. They must remain optional and retention-controlled.

Possible event types:

- item created/modified/opened (where safely observable);
- app/workspace activation where explicitly supported;
- Lumen query/action events;
- collection/workspace interactions.

Do not make screen recording a prerequisite for Rewind.

### G. Process evolution

Initial: one resident process with worker pools.

Only split components when evidence requires it, e.g.:

- unstable media decoder;
- inference runtime process isolation;
- extension sandbox;
- indexing resource isolation.

Each split requires an ADR because IPC adds lifecycle/startup/debugging complexity.

