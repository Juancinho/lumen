# T208 — root query syntax

## 0. Implementation status (2026-10-09)

Built in `lumen-core::SearchQuery`, catalog/content providers and the semantic provider.
No schema, vector generation, model, fusion weight or wire DTO change. Implements the
existing COMMAND_MODEL §7 and ADR-017/022/032 policy. Native root-search review remains.

## 1. Root interaction

Type operators in the ordinary search field; no mode or separate surface. Examples:

```text
contrato ext:pdf
reunión type:document in:"D:\Mis documentos"
invoice after:2026-01-01 before:2026-10-01
"connection refused" type:code
type:image in:Pictures
```

Names arrive immediately; content/meaning still wait for the existing 80 ms settle.
Up/Down/PageUp/PageDown, Enter, Ctrl+Enter, Ctrl+K and Alt+Enter keep their existing paths.
Rows retain their file identity, local actions and selected-row stability (ADR-035/036).
Clearing/changing a filter is an ordinary new query, with stale results cancelled.

## 2. Stable operator semantics

All operators are ASCII case insensitive; all constraints combine with AND. Repeated,
contradictory filters produce no results. A maximum of 16 constraints bounds SQL size.

| Operator | Meaning |
| --- | --- |
| `type:file`, `type:folder`, `type:app` / `type:application` | Catalog kind |
| `type:document`, `type:image`, `type:audio`, `type:video`, `type:code` | File extension category; lists in `QueryType::extensions` are authoritative |
| `ext:pdf` / `ext:.PDF` | Exact extension, ASCII case insensitive |
| `in:D:\Docs` / `in:"D:\My Docs"` | Absolute directory and descendants, boundary checked |
| `in:docs` / `in:projects/lumen` | Contiguous directory components anywhere in a path |
| `before:YYYY-MM-DD` | Modification time strictly before that date's UTC midnight |
| `after:YYYY-MM-DD` | Modification time after that entire UTC date, starting at next midnight |

`in:` accepts slash/backslash separators and a trailing slash; it uses literal components,
not a glob, filesystem traversal or fuzzy path search. `docs` excludes `docs-old` and a
file named `docs`. ASCII case folding matches SQLite's existing NOCASE behavior; non-ASCII
directory spelling must match. `.` / `..` components are invalid; paths are not resolved.
Missing modification metadata fails date constraints. ISO dates validate Gregorian leap
years; no ambiguous locale format or implicit timezone conversion. Unknown `key:` text,
URLs and drive letters stay ordinary search text. Operators inside quotes stay literal.

Recognized empty, malformed or incomplete operators return zero results until corrected;
they never silently broaden a query. Current UI uses its existing no-results state.
Filters without text enumerate inventory by name/stable ID, under the instant budget;
they do not load the model or request an empty embedding. Category filters do not imply
media/PDF extraction support: `type:image` currently finds names/metadata, not visual
meaning. This task adds no OCR, photo or PDF model/extractor.

## 3. Quoted exact requests

Completed quoted phrases require contiguous normalized tokens in the filename or FTS
passage, in order, without prefix/typo expansion inside quotes. Bare text outside quotes
keeps existing behavior. Content's two-of fallback and semantic expansion are skipped
for these explicit lexical requests, preventing near meanings or a partial match from
violating the phrase. Quoting punctuation uses existing safe FTS tokenization; this is
token exactness rather than case/punctuation-sensitive byte matching. An unfinished
quote keeps existing forgiving typing behavior until it is closed.

## 4. Retrieval, privacy and resource limits

Providers share the deterministic shell-independent parser; ordinary text borrows its
input and keeps existing storage fast paths. Filters never enter embeddings/FTS terms.
All metadata values bind SQL parameters. Name prefix/token/typo and content predicates
apply before LIMIT; learned choices are checked too. Content/semantic rows recheck current
canonical metadata before producing actions. Usage keys use only unquoted remaining text;
quoted/filter-only requests retain frecency without creating operator learned keys.

Semantic queries keep the same space, CPU lane, relative floor and fusion weights. Filtered
ANN searches progressively overfetch up to 1,024 candidates, check canonical metadata in
batches of 500 and check cancellation/deadline between expansions. Additional retrieval
has a 100 ms budget; narrow filters may return partial/empty semantic results at the cap.
The relative floor uses the best eligible neighbour. No full-corpus vector scan or index
rewrite is introduced. Ordinary ANN behavior and its existing stale-row handling remain.

Offline behavior is identical to existing local retrieval. No network calls, telemetry,
indexed-content exports or new persistence. All work stays on the existing search thread;
instant names never wait for embedding. Existing 25 ms name and 150 ms content budgets
remain. Release performance evidence is in `docs/benchmarks/t208/`.

## 5. Validation and native review

Automated: parser invalid/unknown/operator/quote/Unicode-safe boundaries/date cases;
real name/content providers with combined metadata constraints, dates, filter-only queries,
learned-choice exclusions, quoted phrases and pre-LIMIT exclusion; semantic embedding
equivalence, narrow-filter overfetch through both exact delta and persisted ANN, code
actions and cancellation. Existing catalog relevance and progressive-selection tests remain.

Release synthetic timing:

```powershell
cargo run --release -p lumen-bench --example query_syntax -- target/t208-timing.json
```

Native review: use existing indexed files to compare a plain query with `ext:` / `in:` /
date filters; try a complete quoted phrase; then modify/clear the filter while navigating.
Confirm the chosen row/focus stays stable, Enter/Ctrl+Enter/Ctrl+K/Alt+Enter retain their
actions, and offline model-unavailable operation still returns lexical results. Do not
stop a resident indexer or mutate its database merely to perform validation.
