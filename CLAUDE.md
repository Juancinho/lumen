# CLAUDE.md — Claude Code operating instructions

Claude Code must follow `AGENTS.md` and `docs/AGENT_PROTOCOL.md`.

## Start of every session

Read:

1. `PROJECT_STATE.md`
2. `TASKS.md`
3. `HANDOFF.md`
4. last 3–5 entries of `WORKLOG.md`
5. `docs/DECISIONS.md`
6. `docs/PRODUCT.md`
7. relevant specs

Then inspect `git status` and recent commits.

Do not begin by re-architecting. If another agent left uncommitted changes, inspect them before modifying or discarding them.

## Continuation protocol

If `HANDOFF.md` names an active task:

- continue it unless blocked/already complete;
- reuse existing abstractions/tests;
- do not regenerate scaffolding;
- verify prior test claims;
- finish task-local TODOs before unrelated work.

If complete, close it and take the highest-priority unblocked task.

## Feature temptation rule

`FEATURE_CATALOG.md` describes the product destination, not permission to implement everything now. Follow `ROADMAP.md` and `TASKS.md`.

When adding a provider/action/workflow, use the existing internal domain model. Do not create a competing command architecture.

## UI rule

Read `DESIGN_SYSTEM.md` before visual work. Preserve the single root-search mental model, keyboard navigation and action panel. Avoid generic AI-dashboard patterns, decorative gradients, glass-on-glass cards, giant hero areas and inconsistent motion.

## End of session

Run relevant validation, update task/state if needed, rewrite `HANDOFF.md`, append `WORKLOG.md`, and commit coherent work.

The handoff must state:

- branch + task;
- implemented behavior;
- exact files changed;
- exact validation commands and result;
- known issues;
- exact next 1–3 steps;
- unresolved evidence/decision needs.
