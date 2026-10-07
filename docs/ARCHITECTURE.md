# ARCHITECTURE.md — system architecture

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

Suggested Rust/desktop organization:

```text
/apps/desktop/                 # Tauri app + React UI
/crates/lumen-core/            # shared domain types
/crates/lumen-search/          # query parsing, fusion, ranking
/crates/lumen-storage/         # SQLite repositories + migrations
/crates/lumen-vector/          # USearch wrapper/index generations
/crates/lumen-indexer/         # enumeration, watcher, queues
/crates/lumen-extract/         # text/PDF/image/media extractors
/crates/lumen-embedding/       # EmbeddingBackend abstraction
/crates/lumen-windows/         # Windows-specific APIs
/crates/lumen-bench/           # benchmark harness/tools
```

React UI:

```text
/apps/desktop/src/
  app/
  components/
  features/search/
  features/preview/
  features/settings/
  design-system/
  ipc/
  state/
```

Avoid dumping Tauri commands into one file.

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

## 7. Suggested schema

Conceptual, not literal final SQL:

```text
items(
  id,
  kind,
  volume_id,
  file_id,
  canonical_path,
  display_name,
  extension,
  size_bytes,
  modified_at,
  created_at,
  indexed_at,
  extractor_version,
  content_fingerprint,
  status,
  error_code
)

chunks(
  id,
  item_id,
  ordinal,
  chunk_kind,
  start_offset,
  end_offset,
  page_number,
  media_start_ms,
  media_end_ms,
  symbol_name,
  text,
  embedding_generation
)

fts_chunks USING fts5(...)

usage_events(
  item_id,
  event_kind,
  occurred_at
)

pins(...)
settings(...)
```

Large binary previews/thumbnails should not be stored directly in SQLite unless benchmark evidence favors it. Prefer a bounded cache directory with content-addressed keys.

## 8. Embedding abstraction

Define a backend trait/interface early:

```rust
trait EmbeddingBackend {
    fn capabilities(&self) -> Capabilities;
    async fn embed_text(&self, batch: &[TextInput], task: EmbeddingTask) -> Result<Vec<Embedding>>;
    async fn embed_image(&self, batch: &[ImageInput], task: EmbeddingTask) -> Result<Vec<Embedding>>;
    async fn embed_audio(&self, batch: &[AudioInput], task: EmbeddingTask) -> Result<Vec<Embedding>>;
    async fn embed_video(&self, batch: &[VideoInput], task: EmbeddingTask) -> Result<Vec<Embedding>>;
    async fn warm(&self, modality: Modality) -> Result<()>;
    async fn unload(&self, modality: Modality) -> Result<()>;
}
```

Exact async/threading form may differ by runtime, but the core must not import runtime-specific types.

### Backends

- `NativeBackend` — selected after M0 benchmark; intended production default.
- `DevBackend` — optional Python/sentence-transformers or test stub for development only.
- `MockBackend` — deterministic test embeddings.

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
- use search-specific task instruction/prefix defined by the model docs;
- keep text path warm while tray resident;
- load vision/audio encoders only during relevant indexing/query operations;
- benchmark f16 vector storage before making it default.

## 10. Search pipeline

```text
Query arrives
   │
   ├─ parse deterministic operators
   ├─ lexical filename/path search ─┐
   ├─ FTS search -------------------┼─► immediate fusion ► UI batch A
   └─ schedule semantic embedding --┘
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

Important: do not await semantic embedding before sending lexical results.

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

Responsibilities of `lumen-windows`:

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
# Refinement — command-center architecture

## A. Shell independence is mandatory

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

## B. Universal command architecture

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

## C. Provider latency classes

Providers declare a rough class:

- `instant`: exact apps/files, calculator parsing, cached snippets;
- `fast`: SQLite/FTS/settings/history;
- `semantic`: query embedding + ANN;
- `deferred`: heavy context/media/network opt-in.

The coordinator may skip or delay providers based on query intent and cancellation state. Never wait for all providers before painting useful results.

## D. Result/action boundary

Use the canonical model in `COMMAND_MODEL.md`. The UI asks for valid actions; it does not hard-code business rules like "PDFs have these seven actions" where that rule belongs in Rust/domain code.

The primary action is safe and predictable. Destructive actions are never primary.

## E. Workflow boundary

The workflow engine executes registered actions using typed inputs/capabilities. It must not bypass permission logic by directly calling random shell code. See `EXTENSIONS_AND_WORKFLOWS.md`.

## F. Context/memory architecture

Temporal/context features use a separate local event store/table family linked to stable item IDs. They must remain optional and retention-controlled.

Possible event types:

- item created/modified/opened (where safely observable);
- app/workspace activation where explicitly supported;
- Lumen query/action events;
- collection/workspace interactions.

Do not make screen recording a prerequisite for Rewind.

## G. Process evolution

Initial: one resident process with worker pools.

Only split components when evidence requires it, e.g.:

- unstable media decoder;
- inference runtime process isolation;
- extension sandbox;
- indexing resource isolation.

Each split requires an ADR because IPC adds lifecycle/startup/debugging complexity.

