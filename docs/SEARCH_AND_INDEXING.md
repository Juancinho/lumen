# SEARCH_AND_INDEXING.md — retrieval, extraction and indexing

## 0. Implementation status (2026-10-09)

- **Navigational (built):** Pass 0 inventory with a coverage guarantee and stable identity
  (ADR-018), app + file catalog (ADR-021), tokenized name/path matching with typo,
  initials and folder context plus usage priors (ADR-022/023); every keystroke, p95 ≈ 5 ms
  on 26.5k entries + 330 apps (Windows).
- **Lexical content (core built):** `chunks` + `chunks_fts` with budgeted queries
  (ADR-017); text/code extraction and 128-token chunking (`lumen-extract`, ADR-028); the
  content pass writes chunks incrementally (`lumen-content`, ADR-029), run by the app after
  every catalog pass over locations with "Index file contents" on; searched on the settled
  query (ADR-032). Code chunks also index name/folder/language context (ADR-036); the
  background pass discovers repository metadata without replacing vectors.
- **Semantic (components):** embedding backend + device policy (ADR-014/015/019), ANN
  wrapper (ADR-016), persistent embedding queue with vectors per generation in SQLite
  (ADR-029); warm latest-wins query embedder that preempts indexing (ADR-030) and
  persistent ANN generations (file + delta, hits validated, validated switch; ADR-031) in
  `lumen-semantic`, maintained by the app's indexing thread and searched on settled queries.
- **Coordination (built):** latency-class lanes and latest-wins search thread (ADR-025);
  settled re-run after 80 ms and weighted-RRF fusion of names / contents / meaning
  (ADR-032; `lumen-bench eval`: fused top-1 0.98 vs 0.96 meaning alone on `fixtures/eval`).
- **Change tracking (built, T207):** bounded native watch hints and scoped reconciliation
  on the existing writer (ADR-037), safe file/folder moves and atomic content invalidation;
  periodic recovery remains. Native overlay/reconnect review is pending.
- **Manual exclusions (built, T112):** root file actions/native tray exclude exact files
  and literal extensions. Bounded writer cleanup also removes offline indexed entries;
  undo restores only affected files. ADR-027 amendment; `specs/T112-manual-exclusions.md`.
- **Optional GPU indexing (built, T212):** persisted native tray option, dedicated adapter
  only, synthetic compatibility probe, same-generation queue and CPU fallback (ADR-038).
  Queries stay on CPU. Native tray/keyboard and long driver soak review remains.
- **Query syntax (built, T208):** root `type/ext/in/before/after` hard filters across all
  lanes, metadata-only queries and required quoted lexical phrases. Same model/index and
  unfiltered ranking; bounded semantic overfetch. See `specs/T208-query-syntax.md`.
- **PDF text (built, T301):** physical-page chunks, FTS and the existing semantic queue;
  matched-page labels and indexed text Quick Look, unchanged generation/vectors (ADR-039).
  Consent-scoped, bounded Rust parser; scans/encryption/limits have explicit skip codes.
  T302 adds on-demand rendered pages/navigation; scanned PDF indexing remains future.
- **Images (built, T303):** bounded PNG/JPEG/WebP/BMP metadata and native q4 visual
  embeddings in the existing queue/generation (ADR-041). Optional consented 109 MB vision
  download, CPU image inference, CPU queries; text GPU indexing unchanged. Root meaning
  queries, `type:image`, file actions and metadata/coverage Quick Look work on the same ID.
  OCR/Drop/Similar and image raster previews remain separate. Native review is pending.

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

Index extracted text and optional symbol names. Implemented (ADR-017/036): external-content
FTS5 over `chunks`, `unicode61 remove_diacritics 2`, prefix indexes 3/4, bm25, user input
always quoted (`FtsQuery::from_user`), every interactive query under a `SearchBudget`. Names
use their own token index (`names_fts`, ADR-022). Migration 0004 adds derived
`search_context` for code (name/folder tokens, filename, language); other kinds leave it
empty. Its rebuild preserves chunk IDs and vectors. Metadata backfill runs on the content
thread, with no filesystem work in search.

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

Initial algorithm: weighted RRF. Built (ADR-032): `lumen_search::fuse` — `Σ w / (60 + rank)`
per entity over each lane's own order, exact/intent matches first, one row per entity;
weights 1/1/2 since ADR-033 (`fixtures/eval-hard`: fused NDCG@10 0.957, top-1 0.939).

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
Videos, nested/duplicate roots collapsed. T111 configures roots; T207 updates files from
native events. Startup/settings/30-minute inventories remain recovery and app discovery.

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

Implemented T207: [task contract](specs/T207-incremental-indexing.md) and ADR-037. Native
hints coalesce at 4,096 paths, with 300 ms quiet / 2 s storm deadline; loss/overflow requests
a 5 s delayed recovery inventory. Events preempt embedding at its existing batch boundary.
Only scoped paths are pruned, after upserts and availability checks. Same-metadata writes,
replacement identities, hard links, case-only renames and marker exclusions are covered.
Ambiguous rename/write hints compare bounded indexed text or T303 image digests within content consent;
unchanged moves retain vectors. No every-file body hash or USN journal is introduced.

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

Built (T209, ADR-036): content/semantic hits carry a typed code target with optional symbol,
language, repository and normalized-text byte offsets. One row keeps the file's entity ID;
symbol-free chunks remain file-level code matches. Ctrl+K offers Copy symbol / Reveal
repository when present, Enter opens the registered file handler, and Alt+Enter previews
the matching indexed passage. Repository discovery checks bounded `.git` ancestors during
indexing; moves clear stale metadata immediately, then the background pass rediscovers it.
No editor CLI is guessed and offsets are not treated as raw-file line numbers.

## 10. PDF indexing

Built text path (T301): see `specs/T301-pdf-text.md` and ADR-039. Text-bearing PDF pages
are independently chunked, including physical numbering through blank pages. Existing
FTS/semantic providers retain one file row with its best matched page. Alt+Enter shows
the indexed passage and page; Enter opens the registered PDF file handler. T302 renders
the matched page in Quick Look (page 1 for names), with physical-page controls and a
capability-based Open matched PDF page action. The registered Sumatra handler supports
direct page arguments; other handlers use the Lumen preview and ordinary Open file.
Rendering scans adds no searchable visual content; scans still need future OCR/vision.

Two complementary representations:

1. extracted text chunks with page numbers;
2. optional page-image semantic embedding for visually meaningful/scanned content.

Do not blindly embed every PDF page twice if storage/CPU cost outweighs benefit. Use heuristics:

- text-rich page → text chunks are primary;
- scan/image-heavy page → vision embedding and OCR enrichment;
- charts/diagrams → page image embedding may add substantial value.

Result opens directly at matched page when the target viewer supports it; otherwise open file plus preview page in Lumen.

## 11. Image indexing

T303 implementation: `lumen-image` performs local bounded headers/digest, EXIF orientation,
RGB decode and pinned Gemma patch preprocessing. Only content-enabled locations are read;
names-only/offline placeholders remain inventory. Supported codecs: PNG/JPEG/WebP/BMP.
Admission: 16 MiB compressed, 32M pixels, 16,384 px/side, 192 MiB decoder allocation.
Other formats/malformed/oversized files have explicit skip coverage. Only dimensions,
format, orientation and local SHA-256 persist; no EXIF GPS, camera history or pixels.

One empty `image` chunk has no caption/name/OCR FTS text. The optional pinned vision q4
encoder feeds the existing text q4 backbone and normalized 256d space. Missing assets
defer image vectors while text drains. One image at a time, below text priority; CPU
sessions load lazily, share the CPU indexing backbone, unload when drained/paused, and
visual work waits on battery. Holds apply at image boundaries; native calls cannot be
hard-interrupted. Source changes during inference cannot publish a stale vector. Schema
v5 and watcher updates preserve unchanged text/code/PDF and unchanged moved image vectors.

Ordinary descriptions (optionally `type:image`/`ext:jpg`) use the settled meaning lane;
filename results remain first and never wait for inference. Enter/Open, Ctrl+Enter/Reveal,
Ctrl+K/Copy path and Alt+Enter metadata/visual coverage use the existing file identity.
No image raster preview is added. See `specs/T303-images.md` and release evidence in
`benchmarks/t303/2026-10-09-joao-pc/`: roughly 9–11 s/image on this loaded CPU, two public
photos correctly ranked from English/Spanish descriptions. This is not broad relevance
or library-throughput evidence. OCR text is T304; perceptual duplicate hashes are future.

OCR is complementary for exact visible text such as screenshots.

## 12. Screenshot intelligence

Screenshots are a high-value category.

T303 provides visual embeddings for supported screenshot formats. T304 OCR is not built.
The remaining target is:

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

Built (ADR-029): `generations` rows (space key × chunker version, created `building`) and
`chunk_vectors` per generation. Built (ADR-031): the first generation is active at once;
a later one is activated atomically after its queue drains and a validation (≤ 1 % failed,
sampled self-recall ≥ 0.95); retired vectors are then deleted in small transactions.

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

T212/ADR-038 adds an explicit, off-by-default dedicated-GPU option in the native tray.
The user-requested mode allows available dedicated VRAM on AC while preserving battery,
memory-pressure and interactive-preemption rules. Queries retain a CPU session. A bounded
synthetic child probe requires same-space cosine ≥0.999, stable finite vectors, ≥90%
offload and ≥1.5× indexing speedup. Probe/cache/quarantine use model/runtime/adapter/driver
identity; a device failure retries pending chunks on CPU. Device switching never creates
a new generation or re-embeds completed chunks. Ordinary Balanced defaults stay in place.

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
