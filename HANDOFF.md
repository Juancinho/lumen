# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main` — initial planning/specification state

## Active task

None claimed yet.

## Current reality

This repository currently contains the refined specification/agent pack. Do not assume application code exists.

The product is now defined as a semantic command center, not just semantic file search. Internal architecture must support providers, result actions and future workflows while keeping the initial implementation narrow.

## Recommended next action

Take **T001** first. T005/T007/T008/T011 can start in parallel once the workspace exists.

## Exact first steps

1. Read `AGENTS.md`, `PROJECT_STATE.md`, `TASKS.md`, `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`.
2. Initialize workspace with shell/core dependency direction enforced.
3. Add minimal typed domain contracts for `ResultItem`, provider identity and actions only where required by T011; do not build plugin SDK.
4. Establish lint/format/test/benchmark commands.
5. Update task/state/worklog/handoff.

## Unresolved evidence-based decisions

- production EmbeddingGemma runtime;
- exact native backdrop path;
- vector scalar profile;
- FastFrame/egui comparative shell spike timing (not before Tauri baseline).
