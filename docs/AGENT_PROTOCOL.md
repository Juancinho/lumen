# AGENT_PROTOCOL.md — multi-agent continuation protocol

## 1. Goal

Codex, Claude Code or another agent must be able to stop and resume work without relying on chat memory.

Canonical continuity files:

- `PROJECT_STATE.md` — milestone snapshot;
- `TASKS.md` — ownership/status/dependencies;
- `HANDOFF.md` — exact current continuation;
- `WORKLOG.md` — append-only compact history;
- `DECISIONS.md` — architecture decisions.

## 2. Claiming work

Before substantive edits:

1. choose one task ID;
2. verify dependencies;
3. mark `CLAIMED` + owner;
4. note branch/worktree if parallel.

Avoid broad "work on Lumen" sessions without a task boundary.

## 3. Worktrees

For parallel agents, prefer:

```text
main
worktrees/codex-T006
worktrees/claude-T103
```

Good parallel domains:

- inference benchmark;
- UI shell;
- SQLite migrations;
- ANN benchmark.

Bad parallel domains:

- two agents refactoring the same shared result model;
- two agents editing design tokens simultaneously.

## 4. Shared interfaces

Files/types with high coordination cost include:

- core domain result/action/provider types;
- migrations;
- IPC schemas;
- design tokens;
- task/ADR documents.

Breaking changes require explicit coordination and usually one owner.

## 5. Handoff quality

`HANDOFF.md` must let a fresh agent continue in minutes.

Include exact commands/tests, not "tests pass".

Bad:

> Worked on search; mostly done.

Good:

> T205: RRF fusion implemented in `crates/lumen-search/src/fusion.rs`; unit tests pass with `cargo test -p lumen-search`. Semantic provider normalization still returns raw cosine and must be mapped to calibrated score before enabling cross-provider global ordering. Next: implement normalization tests, then wire request cancellation.

## 6. No redo rule

A new agent may refactor completed work only if:

- current task requires it;
- tests/benchmarks reveal a problem;
- an ADR approves an architecture change.

Different taste is not evidence.

## 7. Session end

Before ending:

- validation;
- task status;
- state if needed;
- worklog;
- handoff;
- commit.

If context/time ends unexpectedly, prioritize `HANDOFF.md` over prose to the user.
