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
