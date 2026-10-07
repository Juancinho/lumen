# AGENT_PROMPTS.md — ready-to-use prompts

## 1. First Codex/Claude session in a fresh repo

```text
You are implementing Lumen, a premium local-first semantic command center for Windows.

Before writing code, read in this exact order:
PROJECT_STATE.md
TASKS.md
HANDOFF.md
docs/DECISIONS.md
docs/AGENT_PROTOCOL.md
docs/PRODUCT.md
docs/ARCHITECTURE.md
docs/PERFORMANCE.md
docs/DESIGN_SYSTEM.md
AGENTS.md (and CLAUDE.md if applicable)

Lumen is NOT merely a semantic file-search demo. The long-term product combines a universal launcher/search surface, contextual actions, productivity providers, deterministic commands, workflows and a local semantic/context layer. However, do NOT implement future roadmap features before their task/milestone.

Hard architectural constraint: Rust/domain core must remain independent of Tauri/React. Tauri + React/TypeScript is the current presentation shell only. Do not introduce IPC/multiple processes unless a task/ADR requires it.

Take the highest-priority unblocked task from TASKS.md. Do not redesign completed decisions without benchmark/test evidence. Work to production quality, preserve keyboard/accessibility behavior, benchmark performance-sensitive code, and complete the TASKS/PROJECT_STATE/HANDOFF/WORKLOG protocol before ending.

First, report in at most 6 bullets:
- current milestone
- implementation reality
- active/unblocked task you will take
- key constraints relevant to that task
- files you expect to touch
- validation you will run
Then implement it.
```

## 2. Continue Codex work in Claude Code

```text
Continue the existing Lumen repository exactly from its on-disk state. Do not recreate scaffolding or re-plan the project from chat assumptions.

Read PROJECT_STATE.md, TASKS.md, HANDOFF.md, the last few WORKLOG entries, docs/DECISIONS.md, docs/AGENT_PROTOCOL.md, docs/PRODUCT.md and the active task's domain specs. Then inspect git status and recent commits.

If HANDOFF.md has an active task, continue it. Preserve existing architecture and tests. Do not replace working code merely because you prefer another style.

Remember: the destination is an Alfred/Raycast-class Windows command center with a unique local semantic layer, but roadmap scope is strict. Implement only the active task and prerequisites.

At the end, validate, update task/state docs, rewrite HANDOFF.md with exact continuation, append WORKLOG.md and commit coherent changes.
```

## 3. Parallel isolated agent

```text
Work only on <TASK_ID> for Lumen in this isolated worktree.

Read AGENTS.md, PROJECT_STATE.md, TASKS.md, docs/DECISIONS.md, docs/AGENT_PROTOCOL.md, docs/PRODUCT.md and the relevant domain spec.

Do not modify unrelated domains. If you require a breaking shared-interface change, stop at the smallest safe point and document the proposed change instead of silently refactoring other agents' work.

Implement, test and benchmark as applicable. Commit with `<TASK_ID> ...`. Return a task-local handoff with behavior, files, validation, known issues and merge considerations.
```

## 4. Product/architecture review agent

```text
Audit Lumen against docs/PRODUCT.md, FEATURE_CATALOG.md, ROADMAP.md, ARCHITECTURE.md and DECISIONS.md.

Check especially:
- Is Lumen drifting back into "just semantic file search"?
- Is the universal result/action model still coherent?
- Is the root-search UX staying simple despite feature growth?
- Are providers/actions/workflows implemented at the correct milestone?
- Is core still UI-shell agnostic?
- Are privacy/performance constraints preserved?

Do not add features. Return concrete drift findings, severity, affected files/types and minimal corrective actions.
```

## 5. UI polish agent

```text
Polish Lumen to production quality without changing product architecture.

Read DESIGN_SYSTEM.md, PRODUCT.md, COMMAND_MODEL.md and PERFORMANCE.md. Preserve keyboard behavior/accessibility and the single root-search mental model.

Audit: opening/closing, empty query, mixed provider results, semantic refinement, selected row, action panel, Quick Look, drag/drop query, indexing state, errors, light/dark, reduced motion, high contrast and DPI scaling.

Target Apple-level care through restraint, typography, spacing, stable geometry and precise motion — NOT macOS copying, excessive blur, gradients or glass-on-glass cards.

No synchronous filesystem/model work in React. Keep one WebView assumption. Measure jank before micro-optimizing. Use design tokens rather than local magic values.

Finish with tests/visual checks and standard handoff.
```

## 6. Performance agent

```text
Profile Lumen against PERFORMANCE.md in a release build. Report hardware, runtime, corpus/index size and measurement method.

Break latency into: overlay show, provider/lexical search, query embedding, ANN, fusion, IPC/command boundary and render. Measure memory separately for hidden UI, active UI, warm text model and indexing.

Fix the largest measured bottleneck within task scope. Do not trade relevance/correctness/privacy for speed without explicit evidence and an ADR where needed. Add a stable regression benchmark.

Every optimization must include before/after numbers.
```

## 7. Search/relevance agent

```text
Improve Lumen search relevance without changing visual design.

Read SEARCH_AND_INDEXING.md, COMMAND_MODEL.md, TESTING.md and PERFORMANCE.md. Use the evaluation corpus; do not tune by a handful of anecdotes.

Preserve exact filename/app intent, query filters and selection stability. Calibrate provider scores before global fusion. Test semantic paraphrases, code retrieval, OCR, provider collisions and temporal queries.

Report metric changes and qualitative regressions. Do not add an LLM reranker unless a dedicated ADR/task explicitly requests it.
```

## 8. New provider agent

```text
Implement provider <NAME> under task <TASK_ID> using the existing internal provider/result/action model.

Before implementation, define:
- user job
- trigger/intent
- latency class
- result kind
- primary action
- secondary actions
- privacy/security concerns
- offline behavior
- cancellation behavior
- tests

The provider must integrate into the single root search and not create a separate mini-app UI unless PRODUCT/ROADMAP explicitly calls for a dedicated surface.
```

## 9. Workflow engine agent

```text
Implement only the workflow task assigned in TASKS.md.

Read EXTENSIONS_AND_WORKFLOWS.md, COMMAND_MODEL.md and PRIVACY_SECURITY.md. Start with deterministic, typed, inspectable workflows. Do not build an autonomous agent framework or a complex node editor.

Every action declares capabilities. Privileged/destructive/external-data steps follow confirmation/permission policy. Failures must identify the failed step and already-completed side effects.
```
