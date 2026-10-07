# DECISIONS.md — architecture decision log

> ADRs may be amended with evidence. Do not silently contradict them.

## ADR-001 — Local-first core

**Status:** Accepted

Core search/indexing/actions work without cloud services. Optional remote integrations are explicit later.

## ADR-002 — Rust core, Tauri/React shell

**Status:** Accepted provisionally

Use Rust for core/search/indexing and Tauri 2 + React/TypeScript for current UI shell.

Critical boundary: domain/core crates MUST NOT depend on Tauri/React. Shell migration must be possible without rewriting search/indexing.

FastFrame/egui remains an evidence-triggered comparison spike (`TX01`), not a current dependency.

## ADR-003 — SQLite + FTS5 canonical metadata store

**Status:** Accepted

SQLite is canonical for metadata/chunks/settings/history. ANN index is rebuildable derived data.

## ADR-004 — USearch/HNSW candidate ANN

**Status:** Accepted for benchmark path

Final parameters/scalar storage require T008 evidence.

## ADR-005 — Embedding runtime abstraction

**Status:** Accepted

EmbeddingGemma is accessed through `EmbeddingBackend`; no production Python requirement.

## ADR-006 — 256d default semantic index target

**Status:** Accepted provisionally

Use 256d normalized embeddings unless relevance testing justifies another profile.

## ADR-007 — Universal provider/result/action domain model

**Status:** Accepted

Lumen is a command center. Search sources return typed `ResultItem`s; results expose contextual actions. Workflows compose actions later.

This does NOT authorize building a public plugin SDK in M0–M3.

## ADR-008 — Single root search

**Status:** Accepted

Files/apps/commands/productivity features share one root surface. Modes may exist as explicit filters/prefixes but ordinary use must not require mode switching.

## ADR-009 — Progressive retrieval

**Status:** Accepted

Immediate lexical/provider results appear before semantic inference completes. Semantic results refine ranking with selection stability.

## ADR-010 — One-WebView target

**Status:** Accepted provisionally

Search, preview, settings/onboarding should share one WebView where feasible. Hidden state must minimize timers/render work. Split only with measured justification.

## ADR-011 — Multi-pass resumable indexing

**Status:** Accepted

Initial indexing is prioritized and resumable: metadata first, high-value text/code next, images then heavy media/refinement. Interactive work preempts background jobs.

## ADR-012 — Rewind is opt-in metadata/event memory first

**Status:** Accepted

Do not make continuous screen recording a default requirement. Rewind begins from local events/activity semantics with retention controls.

## ADR-013 — Wire DTOs are shell-owned projections; core types carry no serialization

**Status:** Accepted (T011)

Domain types in `crates/` (`ResultItem`, `ActionDescriptor`, ids, …) do not derive serde or any
wire format. The shell maps them into explicit camelCase DTOs (`apps/desktop/src-tauri/src/dto*`),
mirrored by hand-written TypeScript types in `apps/desktop/src/ipc/types.ts`, each guarded by a
Rust JSON-shape test.

Reasons:

- the wire contract is a *projection*, not the domain model: `Payload` (paths, provider keys) and
  provider confidence must never reach the UI; the UI refers to results only by `ResultId` and so
  cannot ask Lumen to act on arbitrary paths;
- keeps `lumen-core` dependency-free and shell-agnostic (ADR-002); another shell may need another
  encoding;
- explicit DTOs make breaking wire changes visible in review.

Revisit (generated TS bindings such as ts-rs/specta) when the DTO set grows beyond roughly ten
types or hand-mirroring causes a real defect. Generation must still run on shell DTOs, not core
types.

## ADR-014 — Synchronous embedding backend trait; shared correctness layer in `Embedder`

**Status:** Accepted (T005)

`EmbeddingBackend` (crate `lumen-embedding`) is a synchronous `Send + Sync` trait. Backends only
map fully formatted strings to raw native-dimension vectors (row-major `Vec<f32>`). Everything
that must be identical across runtimes lives in `Embedder`: versioned task prompts
(`PromptFormat`), batching to `max_batch`, cancellation between batches, output validation
(shape, NaN/inf, zero norm), Matryoshka truncation to the profile dimension with L2
renormalization (f64 accumulation), and the `EmbeddingSpace` key stored with index generations.

Reasons:

- inference is CPU/accelerator-bound; an async trait would add boxing/runtime coupling without
  concurrency gains, and most candidate runtimes expose blocking APIs;
- query-over-index prioritization (docs/ARCHITECTURE.md §13) needs dedicated worker lanes, which
  the query service (T204) and index queue (T202) own; the trait stays scheduling-free;
- one implementation of prompts/truncation/normalization prevents runtimes from silently
  producing incompatible spaces.

`EmbeddingSpace` = model id + weights revision (incl. quantization) + preprocessing version +
prompt id/version + dimension + normalization. It excludes backend and execution target (same
weights on CPU/NPU are one space). Prompt strings are the EmbeddingGemma v1 retrieval prompts and
MUST be verified against the EmbeddingGemma 2 model card in T006 (new `PromptFormat` version if
they differ). Image/audio/video entry points are added later as provided trait methods.
