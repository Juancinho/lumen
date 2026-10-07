# TESTING.md

## 1. Test pyramid

### Unit
- query parser
- ranking/fusion
- score normalization
- action permission classification
- chunkers
- file identity logic
- workflow validation

### Integration
- SQLite migrations/FTS
- ANN generation swap
- provider coordinator
- watcher → reindex
- action execution adapters
- preview pipeline

### End-to-end Windows
- global shortcut
- focus/IME
- keyboard navigation
- action panel
- hide/show lifecycle
- light/dark/DPI
- open/reveal actions

## 2. Search relevance set

Maintain a small committed synthetic corpus and query judgments for:

- exact filenames;
- fuzzy names;
- FTS;
- semantic paraphrase;
- code retrieval;
- screenshot OCR vs semantic;
- provider collisions (`calculator` vs file vs app);
- temporal filters.

Track MRR/nDCG/Recall@k where meaningful.

## 3. Provider contract tests

Every provider should test:

- cancellation/stale query behavior;
- empty input behavior;
- latency class expectations;
- result typing;
- valid action IDs;
- no destructive primary action.

## 4. Workflow tests

Use a mock action registry to test:

- validation;
- parameter binding;
- permissions;
- step failure;
- cancellation;
- partial side-effect reporting.

## 5. Visual/interaction states

Cover:

- hidden/opening/open;
- empty query;
- results;
- semantic refinement;
- action panel;
- preview expanded;
- drag/drop query;
- no results;
- indexing progress;
- provider error;
- reduced motion;
- high contrast;
- 100/125/150/200% scaling;
- long paths/titles;
- IME where practical.

## 6. Performance regression

See `PERFORMANCE.md`. Benchmark release builds; store machine/config metadata.

## 7. Failure testing

Simulate:

- corrupt ANN file;
- SQLite migration failure;
- model unavailable;
- removable drive disappears;
- file deleted mid-extraction;
- app crash during index write;
- workflow action failure;
- extension host crash later.

Lumen must recover without losing canonical metadata or blocking root search unnecessarily.
