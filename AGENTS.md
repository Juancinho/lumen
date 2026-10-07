# AGENTS.md — rules for Codex and all coding agents

This file is authoritative for agent behavior in this repository.

## Mission

Build Lumen as a premium, local-first **semantic command center for Windows**. It must combine launcher, search, actions and workflows while remaining visually restrained and extremely fast.

Do not reduce Lumen to a vector-search demo. Do not turn it into an overloaded dashboard either.

## Before touching code

Read in order:

1. `PROJECT_STATE.md`
2. `TASKS.md`
3. `HANDOFF.md`
4. `docs/DECISIONS.md`
5. `docs/AGENT_PROTOCOL.md`
6. `docs/PRODUCT.md`
7. the relevant domain spec

Then inspect `git status`, recent commits and relevant tests. Claim a task before substantial work.

The repository is the source of truth. Never infer current implementation state from chat history.

## Core architectural rules

- `lumen-core` and domain crates MUST NOT depend on Tauri, React or WebView2.
- Tauri + React/TypeScript is the current presentation shell, not the identity of Lumen.
- The UI consumes typed domain models and commands; it never reaches directly into SQLite/HNSW/model-runtime implementation.
- Search is provider-based; actions are capability-based; workflows compose existing actions.
- Build the internal abstractions needed by current milestones, not a public plugin SDK prematurely.
- Do not introduce a separate UI/core process until evidence justifies IPC/crash isolation.
- Never make semantic inference block first lexical results.

## Hard rules

- Do not redo completed work merely because you prefer another implementation.
- Do not silently change architecture, design tokens, schemas, task IDs or performance budgets.
- Architectural changes require an ADR with evidence.
- No expensive work on the UI thread.
- No cloud dependency for core search.
- No indexed content, queries, filenames, embeddings or telemetry leave-device by default.
- No LLM when deterministic parsing, OS APIs, FTS or embeddings solve the task better.
- No Electron unless an ADR explicitly reverses the shell decision.
- No required Python production runtime.
- Preserve keyboard interaction, accessibility and result-selection stability.
- Do not add a feature if it violates current milestone scope just because it is mentioned in `FEATURE_CATALOG.md`.

## Product-quality rules

Every user-facing feature must answer:

1. Can it be invoked from the root search without mode confusion?
2. What actions become available on its results?
3. What is the keyboard path?
4. What happens offline?
5. What are its privacy implications?
6. What is its performance budget?
7. Does it preserve the clean single-surface UX?

"Apple-like" means precision, restraint, hierarchy, typography, responsiveness and motion quality. It does NOT mean copying macOS chrome or adding excessive blur.

## Performance discipline

- Measure release builds on Windows hardware.
- Keep one WebView unless an ADR proves another is necessary.
- No polling/animations/timers while hidden unless explicitly justified.
- Virtualize result lists and lazy-load previews.
- Bound background queues and worker counts.
- Indexing must be resumable, multi-pass and value-prioritized.
- Interactive query work preempts background embedding.
- Use benchmarks before changing vector precision, chunking or model runtime.

## Definition of done

A task is done only when applicable items are complete:

- implementation;
- tests;
- lint/format/build;
- accessibility/keyboard path;
- visual-state check;
- performance measurement for sensitive code;
- docs/ADR updates;
- `TASKS.md` status;
- `PROJECT_STATE.md` if milestone state changed;
- `HANDOFF.md` rewritten;
- `WORKLOG.md` appended;
- coherent commit with task ID.

## Commit convention

```text
T123 feat(search): add progressive provider fusion
T124 feat(actions): add contextual PDF actions
T125 perf(ui): suspend hidden webview work
T126 docs(adr): select embedding runtime
```

## Parallel work

Follow `docs/AGENT_PROTOCOL.md`. Use isolated worktrees for genuinely parallel tasks. Never edit another agent's claimed task files casually.

## Priority when trade-offs conflict

1. correctness/data safety/privacy;
2. perceived speed;
3. measurable latency/resource use;
4. interaction and visual polish;
5. maintainability;
6. feature breadth.
