# SEARCH_AND_INDEXING.md — retrieval, extraction and indexing

## 0. Implementation status (2026-10-08)

- **Navigational (built):** Pass 0 inventory with a coverage guarantee and stable identity
  (ADR-018), app + file catalog (ADR-021), tokenized name/path matching with typo,
  initials and folder context plus usage priors (ADR-022/023); every keystroke, p95 ≈ 5 ms
  on 26.5k entries + 330 apps (Windows).
- **Lexical content (storage only):** `chunks` + `chunks_fts` with budgeted queries
  (ADR-017); no extractor fills them yet (T201).
- **Semantic (components only):** embedding backend + device policy (ADR-014/015/019), ANN
  wrapper (ADR-016); no queue, generations or query lane yet (T202–T205).
- **Coordination (built):** latency-class lanes and latest-wins search thread (ADR-025).

## 1. Retrieval philosophy

Lumen combines three different kinds of retrieval because each solves a different user memory pattern:

1. **navigational:** “I remember the name/path”;
2. **lexical:** “I remember words inside it”;
3. **semantic/multimodal:** “I remember what it meant/looked/sounded like.”

A high-quality product fuses them. Pure vector search is not enough.

## 2. Query lifecycle

### 2.1 Keystroke path

On every meaningful input change:

- parse filters synchronously and cheaply;
- cancel the running/stale query (latest wins, ADR-025);
- run name/path matching immediately and show results (instant lane);
- once typing settles (short adaptive delay or a stable word boundary): content FTS with a
  generous budget, query embedding + ANN retrieval;
- fuse and send refined results.

Content FTS is not run per keystroke: with real hits it costs 13/68 ms p50/p95 at 100k
chunks on a 2-vCPU machine (T016, ADR-017 note).

Suggested initial semantic trigger: 50–90 ms after last keystroke, adaptive based on typing cadence. Benchmark; do not hard-code without testing.

### 2.2 Query caching

Use a bounded LRU for recent query embeddings keyed by:

- normalized query text;
- task prefix/version;
- model/index generation;
- dimension.

Prefix reuse may be explored later, but correctness first.

## 3. Lexical retrieval

### Filename/path

Signals in descending importance:

- exact filename;
- filename prefix;
- token prefix;
- fuzzy subsequence/edit similarity;
- path segment match;
- extension match;
- folder scope.

Use normalization that respects Windows case-insensitive behavior while preserving display form.

### FTS5

Index extracted text and optional symbol names. Implemented (ADR-017): external-content
FTS5 over `chunks`, `unicode61 remove_diacritics 2`, prefix indexes 3/4, bm25, user input
always quoted (`FtsQuery::from_user`), every interactive query under a `SearchBudget`. Names
use their own token index (`names_fts`, ADR-022).

Store only what is needed for retrieval/snippets; large full bodies may be stored in a separate content cache if required.

Tokenization must handle:

- natural language;
- camelCase/snake_case for code;
- filenames;
- Unicode;
- common programming punctuation where useful.

## 4. Semantic retrieval

### Default vector dimension

Use EmbeddingGemma 2 at 256 dimensions by default, with explicit L2 renormalization after truncation. The model is trained for supported truncation dimensions 128/256/512/768.

### Similarity

Cosine similarity over normalized vectors. If vectors are normalized and the ANN engine uses inner product efficiently, equivalent formulations may be benchmarked.

### Vector scalar type

Decided (ADR-016): **f16** storage in USearch HNSW, cosine, M=16, ef_search=256 — recall@10
1.000 at 100k / 0.988 at 1M. i8 rejected (recall 0.85). f32 stays the reference.

Never change index scalar type without generation/version migration.

## 5. Hybrid fusion

Initial algorithm: weighted RRF.

Conceptual form:

```text
score =
  w_name   * RRF(rank_name)
+ w_fts    * RRF(rank_fts)
+ w_vector * RRF(rank_vector)
+ bounded metadata priors
```

Start with roughly balanced lexical/semantic weights and tune against an evaluation set. Do not tune solely by “feels good” on five queries.

Special cases:

- exact filename can override fusion;
- explicit `type:` filters are hard filters;
- quoted phrases favor exact/FTS;
- image-by-example queries may be vector-dominant.

## 6. Result stability

Progressive reranking can feel broken if rows jump under the cursor.

Rules:

- before user navigation: semantic batch may reorder freely with animation;
- after user moves selection: preserve selected result and keyboard focus;
- avoid moving the selected row unexpectedly;
- batch multiple semantic updates into one commit;
- ignore stale query IDs;
- if a result disappears due to a hard filter change, select nearest stable neighbor.

## 7. File discovery

Initial roots:

- user-selected locations;
- optional Documents/Desktop/Downloads/Pictures defaults;
- optional developer roots.

Implemented (T107): the standard folders Desktop, Documents, Downloads, Pictures, Music and
Videos, nested/duplicate roots collapsed, synced at start-up and every 30 min until the
watcher (T207) and configurable roots exist.

User-configurable locations and exclusions are **T111** (spec:
`docs/specs/T111-indexed-locations.md`): folders or whole drives as locations with a visible
state (ok / not available / partial — unavailable drives keep their items), exclusions by
folder, by name anywhere, and toggleable defaults (OS set + developer noise; build folders
only next to a project marker), one versioned settings value `index.locations`, any change
restarts the pass, per-root `lumen:catalog-changed`.

Default exclusions should include obvious high-noise locations such as caches/temp and optionally:

- `.git/objects`
- `node_modules`
- virtual environments
- build output
- package caches

Do not exclude source folders themselves.

User exclusions always win.

## 8. Change detection

For each item track enough metadata to avoid unnecessary extraction:

- stable file ID when available;
- path;
- size;
- modified time;
- extractor version;
- lightweight fingerprint when ambiguity exists.

Do not hash every multi-gigabyte file on every scan.

On rename with stable identity, update path without recomputing embeddings if content metadata indicates no change.

## 9. Text and code chunking

### Plain prose

Prefer semantic-ish boundaries:

- headings;
- paragraphs;
- list groups;
- sentence windows.

Target chunks should remain well under model context limits; retrieval chunks are typically much smaller than 8K tokens. Start in the few-hundred-token range and evaluate.

Use overlap sparingly to preserve boundary context.

### Markdown

Preserve:

- heading path;
- code fences;
- list context;
- link text where meaningful.

### Code

Prefer language-aware boundaries where practical:

- function/method;
- class/type;
- module-level block;
- docstring + symbol body.

Fallback to line windows for unsupported languages.

Store symbol name and language as metadata. A query like `retry failed requests python` should return the symbol and file, not an arbitrary 500-line blob.

## 10. PDF indexing

Two complementary representations:

1. extracted text chunks with page numbers;
2. optional page-image semantic embedding for visually meaningful/scanned content.

Do not blindly embed every PDF page twice if storage/CPU cost outweighs benefit. Use heuristics:

- text-rich page → text chunks are primary;
- scan/image-heavy page → vision embedding and OCR enrichment;
- charts/diagrams → page image embedding may add substantial value.

Result opens directly at matched page when the target viewer supports it; otherwise open file plus preview page in Lumen.

## 11. Image indexing

For each image:

- vision embedding;
- EXIF/basic metadata where available;
- optional OCR text;
- dimensions;
- perceptual hash optional for duplicate detection.

Do not generate captions merely to make semantic search work; EmbeddingGemma 2 already supports cross-modal retrieval.

OCR is complementary for exact visible text such as screenshots.

## 12. Screenshot intelligence

Screenshots are a high-value category.

Index:

- image semantic embedding;
- OCR text;
- application/window metadata only if available through a future explicit capture integration—not inferred retrospectively.

This enables both:

- `screenshot with a Docker error` (semantic);
- `screenshot containing ECONNREFUSED` (OCR/lexical).

## 13. Audio indexing

When enabled:

- decode with Windows Media Foundation;
- segment into overlapping temporal windows appropriate for the model;
- embed audio directly;
- store time ranges;
- later optionally enrich with local speech-to-text, but do not require transcription for semantic retrieval.

Keep audio encoder unloaded when not indexing/querying audio if runtime supports it.

## 14. Video indexing

Avoid embedding every frame.

Initial strategy:

- detect/schedule representative keyframes or fixed sparse samples;
- use shot-boundary detection later;
- optionally pair a short frame sequence with audio segment where supported;
- store start/end timecode.

Result: `demo.mp4 · 03:42` with representative frame.

Tune sampling to balance recall and index size.

## 15. Thumbnail cache

Thumbnails improve perceived quality but can dominate disk space.

Use:

- content-addressed cache keys;
- bounded LRU size (configurable, sensible default 256–512 MB);
- modern compressed format where rendering stack supports it;
- lazy generation for low-priority file types;
- invalidate on content/version change.

Do not store full-resolution duplicates.

## 16. Index generations

An index generation is defined by at least:

- model ID/version;
- model preprocessing version;
- task prefix version;
- modality config;
- embedding dimension;
- normalization;
- vector scalar type;
- chunker version.

If these change incompatibly:

1. keep old generation searchable;
2. build new generation in background;
3. validate;
4. atomically switch active generation;
5. delete old generation later.

This prevents “update app → search broken for an hour”.

## 17. Prioritization of indexing jobs

Highest to lowest:

1. explicit user-requested reindex / item just searched;
2. new small text files;
3. modified files in active/recent project roots;
4. PDFs/images;
5. large archives/media;
6. historical bulk catch-up.

Interactive query embedding preempts all background embedding.

## 18. Resource-aware indexing

Default Balanced policy:

- limit concurrency;
- pause/reduce on battery unless user chooses otherwise;
- yield when foreground CPU pressure is high;
- schedule large media at low priority;
- persist queue so restart does not restart from zero;
- expose pause/resume.

Never consume all CPU/GPU merely because indexing can be parallelized.

## 19. Relevance evaluation

Create a repeatable local test corpus with:

- exact filename queries;
- paraphrase queries;
- multilingual queries;
- code intent queries;
- screenshot/image description queries;
- ambiguous queries;
- temporal filters.

Measure:

- Recall@K;
- MRR;
- NDCG@K;
- top-1 success;
- latency by stage.

Every ranking-weight change should run this suite.
## 20. Search is broader than files

The retrieval engine feeds the universal provider coordinator. File lexical/vector scores are not directly comparable to calculator/app/settings/workflow confidence. Normalize per-provider before global fusion; strong intent rules may override numerical fusion.

Examples:

- query `spotify` should normally rank installed Spotify above a semantically related PDF;
- query `2+2` should return calculator immediately;
- quoted exact strings should strongly favor lexical/FTS;
- an image query object should make visual/vector providers dominant.

See `COMMAND_MODEL.md`.

## 21. Multi-pass initial indexing

The first index must become useful progressively and must survive shutdown.

### Pass 0 — inventory

- stable IDs where possible;
- path/name/type/size/timestamps;
- app catalog;
- exclusions.

Root search becomes useful immediately.

### Pass 1 — high-value text/code

Prioritize Desktop/Documents/Downloads/user project roots/recent files. Parse only useful project sources; exclude generated trees such as `node_modules`, `.venv`, build output, package caches and `.git/objects` by default.

### Pass 2 — documents/images

PDF/page extraction, image embedding, screenshot OCR.

### Pass 3 — historical bulk media

Sparse/scene-aware video and segmented audio.

### Pass 4 — refinement

Optional improved OCR, richer visual sampling, re-chunking after algorithm upgrades.

Persist checkpoint/job state. If shutdown occurs after 732,814 units, continuation should not restart from zero.

## 22. Resource profiles

Expose at least conceptual profiles:

- **Eco:** low concurrency, defer heavy media;
- **Balanced:** default adaptive resource use;
- **Turbo:** user explicitly requests faster initial indexing.

Interactive query embeddings always outrank background indexing.

## 23. 1 TB planning target

For ~1 TB of genuinely indexable personal content, design for roughly hundreds of thousands to low millions of semantic units, not one vector per file and never one vector per byte/frame.

Initial engineering targets:

- metadata usable in minutes;
- high-value semantic content within first hours;
- Smart full index may take ~20–60 hours of CPU-effective work on a mainstream i5-class machine depending heavily on corpus/modalities;
- exhaustive multimedia can take days and is not the default goal;
- target total index/cache size below ~1–2% of indexed source data, with configurable cache bounds.

These are planning envelopes, not UI promises. Benchmark real corpora before publishing claims.

