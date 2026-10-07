# FEATURE_CATALOG.md — destination catalog

> This is a product catalog, NOT the implementation queue. `ROADMAP.md` and `TASKS.md` control scope.

## A. Universal search

- filename/path exact, prefix, fuzzy
- applications
- full-text documents
- semantic text/code
- PDF pages
- images/screenshots
- OCR exact text
- audio moments
- video moments
- clipboard
- snippets
- quicklinks
- workflows
- Windows settings/commands
- recent work / Rewind

## B. Query ergonomics

- natural language
- operators: `type:`, `ext:`, `in:`, `before:`, `after:`
- quoted exact phrases
- provider prefixes/aliases for power users
- query history
- pinned queries
- recent queries
- optional learned local ranking preferences

## C. Actions

- open / open as admin where valid
- reveal in Explorer
- copy path / value / content / timestamp
- Quick Look
- open with
- pin/favorite
- delete only behind explicit confirmation/policy
- find similar
- related/context lens
- OCR/copy visible text
- run workflow
- window actions
- transform clipboard/snippet actions

## D. Productivity

### Calculator
- arithmetic
- percentages
- units
- date/time arithmetic
- common programmer conversions

### Clipboard
- opt-in history
- search
- pin
- preview
- delete/clear
- app/source metadata where OS permits safely
- sensitive-content rules

### Snippets
- keyword/search invocation
- placeholders for date/time/clipboard/arguments
- plain text first
- optional rich content later

### Quicklinks
- URLs
- local paths
- templates with `{query}` or named arguments
- custom aliases

### System
- settings pages
- common safe actions
- services/processes only with cautious semantics
- window management if reliable

## E. Workflows

- named workflows searchable from root
- sequential actions
- parameters
- conditions later
- clear permissions
- user-visible failure step
- no hidden destructive operations
- import/export format later

Examples:

```text
dev gestureos
→ open repo in VS Code
→ open terminal in repo
→ run environment command
→ open docs URL
→ position windows
```

```text
research quantization
→ open saved collection
→ open notes
→ open paper folder
```

## F. Semantic-native capabilities

### Find Similar
Available from image, document, code symbol and eventually media moment.

### Semantic Drop
Drop/paste an object to create a similarity or multimodal query.

### Smart Collections
Persist a query/filter/semantic definition and update dynamically.

### Context Lens
Inspect related content around a selected item.

### Semantic Workspace
Aggregate a project/topic using path, usage and semantic relations.

### Rewind
Optional local timeline and temporal retrieval.

## G. Media

- direct audio embeddings
- timecoded audio results
- video scene/sparse sampling
- representative frame previews
- play from timecode
- optional local transcript enrichment

## H. Extensibility — post-stability

Potential extension capabilities:

- search provider
- command provider
- result action
- workflow action
- preview renderer
- settings contribution

Extensions require explicit capabilities and a security model. No arbitrary in-process untrusted code by default.

## I. Optional future AI layer

Only after deterministic/product foundations:

- summarize selected local results;
- ask a question over an explicit local result set;
- natural-language workflow composition with confirmation;
- local-first model where feasible;
- remote model only opt-in with exact disclosure of data sent.

Never make LLM output authoritative for destructive/system actions without deterministic validation and confirmation.
