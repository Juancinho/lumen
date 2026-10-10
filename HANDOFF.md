# HANDOFF.md

> Live continuation only. Read AGENTS.md in order; TASKS.md owns status and Next.
> Rewrite each session; history belongs in WORKLOG and evidence/ADRs.

## Branch and exact continuation

T304 is implemented, REVIEW, on `codex/t304-image-ocr`, based on pushed main 1aa51ca.
Managed worktree: `C:\Users\tj14o\.codex\worktrees\t213-indexing-progress\lumen`.
The user authorized commit/push to main and continued improvements; this coherent T304
revision is intended for normal fast-forward integration. Verify current git status,
`git log -1` and origin/main before continuation; do not assume a revision from chat.
Next ordered implementation is **T305 Semantic Drop**, still TODO/unclaimed. Read
SEARCH_AND_INDEXING §11/12, PRODUCT, COMMAND_MODEL, PRIVACY_SECURITY and the relevant
ADRs before defining its ephemeral query-object/consent/provider lifecycle. Preserve
the single writer, T112 JSON v3 exclusions, T213 GPU/scheduling, CPU queries and all vectors.
T306 Similar and T014 full runtime matrix remain separate; keep older REVIEW checklists.

## T304 — optional local image text

Read `docs/specs/T304-image-ocr.md` and ADR-043. Native **Content indexing → Index text
in images (OCR)** is off by default and persists explicit opt-in. Installed Windows
profile recognition runs locally on the catalog MTA writer, CPU, lazy/unloaded when
off/drained/deferred. No package/language/model installation, capture or cloud path.
Portable capability works with es-ES on this machine; Microsoft's supported desktop
contract requires package identity, so missing language/engine remains explicit.

Bounds: existing 16 MiB source/placeholder admission, 4,096 px/edge and 8M pixels before
full decode, 16 KiB UTF-8 text, four sequential images/two-second cooperative round,
five-second native operation deadline. Native allocation/cancellation are not hard
sandbox guarantees. Pause, interactive hold, battery and ADR-019 memory threshold use
existing policy retry without a held-OCR hot loop or delaying queued vector turns.
Transparency composites over white for OCR; visual preprocessing/version is unchanged.

Schema6 `image_ocr` tracks digest/version/state/language/error independently. Exact text
enriches the same image chunk/FTS; chunk IDs, visual vector/ANN sequence and generation
stay. Source scope/digest rechecked at commit, moves preserve unchanged units, edits/
deletions invalidate them, cancellation stays eligible. Off clears only OCR text/coverage
in crash-resumable 512-row writer transactions, including while indexing is paused.
The cached menu reports removal until it finishes. OCR is lexical enrichment, not new
OCR semantic vectors. Scanned PDFs/raster image preview remain separate.

Root quoted/type/ext searches and file actions keep same-row identity. Alt+Enter shows
bounded text and off/pending/indexed/empty/skipped/failed/unavailable states. Preview
reads on spawn_blocking and verifies current stored path against the remembered result,
rejecting reused/moved IDs; paths/digests/backend errors never enter OCR DTOs. Cached
OCR percentage is expressly of prepared image units; root Files read covers inventory.

Validation: `cargo xtask test --locked` passes **382 Rust tests, including 41 shell**,
two deliberate network tests ignored. `cargo clippy --workspace --all-targets --locked
-- -D warnings`, fmt and `cargo xtask arch` (15 core crates) pass. `npm run check`
passes formatting/lint/types and **88 tests**; `npm run build` passes. Latest optimized
custom-protocol executable embeds index-CJ4Y37xj.js. Native release synthetic pipeline
finds five quoted/type/ext captures and preserves vectors/ANN sequences through OCR/off.
Final sample: engine 4.57 ms, full decode/native/digest/FTS commits 7.43–19.21 ms, working
set 8.37→15.29 MiB snapshots. Clear small text only, not general accuracy/ETA/peak or
whole-app budgets. Empty/unaligned recognition and cooperative preemption pass;
reproduction/numeric-only evidence: `docs/benchmarks/t304/2026-10-10-joao-pc/`.
Actual PreviewPane browser QA at 1280×720 and 800×420 (640/340 px panels) covers long
names, Unicode/scroll and all seven states in light theme. Temporary fixture/tab/server
removed. Automated keyboard/IPC/source/migration tests pass; native reviews below remain.

Remaining human REVIEW:

1. On an explicit test content location, opt in from the native checkbox; cached counts
   advance and exact known words with `type:image` return the same image. Alt+Enter shows
   OCR text; Escape/Alt+Enter close and Ctrl+K/file actions retain selection/focus.
2. Pause/resume, battery/unplugged and restart preserve opt-in/pending work. Off suppresses
   preview text immediately, reports bounded cleanup, then exact OCR-only hits disappear;
   ordinary image semantics/vectors remain. Re-enable processes only admitted content.
3. Missing installed profile language/engine gives explicit unavailable state without an
   installer. Complex/multilingual/small-font/transparent real photos need accuracy review.
4. Native dark/high contrast/high DPI/narrow pane and long library/resource/driver soak.

## Delivered resident app and schema compatibility

Optimized bundle: `D:\Proyectos\lumen\target\t304-release\lumen.exe`, 21,582,848 bytes,
SHA-256 F6DBF968E443F715EDCF14B1B6AD77D67C7424D9161A7BA2459051A338FA2F91.
Runtime/DirectML and license/PDF/image notices hash-match the existing T213 bundle.
Normal Quit stopped old PID4632 before replacement. The familiar launcher path
`D:\Proyectos\lumen\target\t213-release\lumen.exe` now contains the same verified
T304 bytes; it is the current resident, PID **18468**. Check actual PID/path before
maintenance. Launched `--background` from an override-free environment; no model download
or root/settings edit. OCR remains off/setting absent/zero OCR rows after migration.
Read-only check: schema6, active generation1, zero vector errors, code 48,729, image 1,699, text 179,301 vectors;
locations/settings hash unchanged, nine roots/five excluded types/one exact exclusion.

```powershell
Start-Process -FilePath 'D:\Proyectos\lumen\target\t213-release\lumen.exe'
```

Do not overwrite a running exe or launch old v5 binaries against the v6 DB. Future
PDF-first/scheduler rollback must retain schema6 support; no indexing reset is required
for that code change. Old stopped exe is `target/t213-release/lumen-schema5.exe` and a
consistent pre-migration local SQLite backup is `target/t304-schema5-checkpoint/lumen-schema5.db`.
Those are archival recovery material only, containing local user data in ignored target;
never commit/upload them or run the old exe on current AppData. Do not restore a checkpoint
without checking newer derived work/settings. Existing `lumen-prewarm.exe` is also v5.
No LUMEN overrides are needed; don't propagate empty override strings. The single writer
resumes the current queue; cached pinned models and current visual/text generation remain.
Build in this worktree with `$env:CARGO_TARGET_DIR='D:/Proyectos/lumen/target/t213-build'`;
never copy Cargo fingerprints from another checkout.

The installed-app startup recheck completed and stored a newly accepted report: CPU/GPU
text 5.385/12.199 chunks/s, prior/hybrid visual cycles 18.967/15.689 s (1.209x), minimum
cosine 0.99999982. GPU query p95 319.224 ms versus CPU25.639 ms; queries remain CPU.
Only parent PID18468 remains, probe child exited, stderr empty, and both text/image
vector counts advanced after the hold released. Metrics-only report:
`docs/benchmarks/t304/2026-10-10-joao-pc/installed-startup-gpu.json`.

## T213 — preserved indexing repair and native review

ADR-042/specs/T213-indexing-progress.md: bounded alternating image/text/PDF preparation,
eight-text-batch image fairness, cached phase/device/root Files read/vector progress.
Full GPU vision fails Reshape on GTX1650; validated hybrid is CPU vision plus shared GPU
backbone. Queries stay CPU. Previous T213 warm probe: text CPU/GPU 5.582/11.074 chunks/s,
prior/hybrid visual cycles 19.674/16.351 s (1.203x), minimum cosine 0.99999982. Numeric
synthetic/public-licensed evidence remains under benchmarks/t213; no new probe/quality claim
is made by OCR. Task Manager's default 3D chart alone does not show that inference lane.
Native dark 800×576 footer previously fit and photo-description queries returned photos.
Remaining native review: keyboard selection/Alt+Enter/Ctrl+K with footer, narrow/high-DPI/
high contrast, pause/battery and longer driver/library soak. Previous key injection was
inconclusive under active user input/focus loss; do not claim a pass or repeatedly interfere.

All old binary/schema/override launch instructions below are historical and superseded
by the schema6 resident instructions above; preserve their pending human review checklists.

## T112 — manual exclusions and native review

Read specs/T112-manual-exclusions.md and ADR-027's dated amendment. Reuses core descriptors,
catalog provider/actions/locations, indexer full+watcher rules, native shell tray/pickers.
Locations JSON v3 adds exclude_extensions; v1 content upgrade and v2 explicit names-only
choices survive, unknown fields preserved and future versions remain read-only. No SQLite
migration, model/index generation, query parser, ranking or new presentation surface.

Root file/code/PDF/image → Ctrl+K offers Exclude this file and Exclude all .ext files;
trusted result payload determines the path/type, UI sends ids. Tray → Exclusions → File
types offers .js/.json/.log checks plus custom types, an example-file picker to add another,
and Exclude files… for multiple exact paths. Types default off. Uncheck / Include again
undo. Literal suffixes exclude non-directories only; .json does not match .jsonl.

Before full inventory, explicit-user cleanup on the existing writer pages at most 512
file-source path rows/transaction, including offline entries. Cascades remove their chunks,
FTS/vectors/usage/pins; application inventory and unrelated vectors stay. Disk files are
untouched. Including again queues only affected files when available. Already-running
native inference must finish before cancellation/cleanup; no synchronous menu-thread scan.

Automated tests: full/watch scan + undo, settings normalization/v1/v2/future compatibility,
multi-page offline cleanup/cancel/idempotence/vector and application preservation,
exact-file disk safety/restoration, capability/id policy and Ctrl+K/arrow/Enter routing.
Browser actual RootSearch/ActionPanel at 1280×720 and 800×420: long JSON filename, both
new action labels and selected state, query retains focus; light theme observed. Temporary
fixture/server/tab/viewport removed. Native tray/pickers/DPI remain pending:

1. On a synthetic folder with .js/.json/.log/.jsx/.jsonl and folder.js/notes.md, toggle
   File types → .js/.json/.log; old name/content/meaning/suggestion hits disappear after
   writer cleanup. folder.js/notes.md, .jsx/.jsonl and application results remain.
2. Ctrl+K a file/code/PDF/image hit → exclude this file, then Include again in Exclusions;
   repeat all .ext files. Extensionless file offers exact exclusion only. Enter opens;
   Ctrl+Enter reveals, Alt+Enter previews and keyboard selection remains valid.
3. Exclude files… selects several exact files; Include again restores each. Exclude another
   file type… picks an example .txt; check appears; uncheck restores. Cancel picker and
   choose extensionless file; no rule added. Verify persistence after a user-chosen restart.
4. Disconnect an indexed test location, exclude a known type, verify stale excluded hits
   vanish; unrelated offline entries remain. Reconnect/undo restores affected files only.
   Pause and indexing under load stay responsive; deleting on-disk user data is never involved.

Release evidence: docs/benchmarks/t112/2026-10-10-joao-pc/README.md.
Final 100k synthetic cleanup removes 75k files in 16.47 s; 641 concurrent retained-name
queries p95 32.99 ms, maximum 55.95 ms; 25k items/vectors and active generation retained.
Loaded machine/crowded numeric names, not whole-app or real-library ETA.
New bundle is
target/t112-release/lumen.exe; use it after the user's chosen Quit, not while expecting
single-instance to replace the resident app. Cached-model development launch from repo:

Optimized custom-protocol exe: 21,407,744 bytes, SHA-256
494F838DC5C3495C52B4559CA7D190E92824C2E209145F4C18BF26A762F4A38A.
Embedded index-DimVfZd4.js and exclusion action/menu ids verified. Runtime/DirectML,
licenses, PDF and image codec notices hash-match T303. Resident PID 6344 remained intact
at the final 2026-10-10 00:29 check. Launch only after the user's chosen Quit:

```powershell
$env:LUMEN_EMBED_MODEL_DIR="$PWD\.cache\t006\embeddinggemma-2-ONNX"
$env:LUMEN_EMBED_VISION_DIR="$PWD\.cache\t303\model"
$env:LUMEN_ORT_DYLIB="$PWD\target\t112-release\onnxruntime.dll"
$env:LUMEN_EMBED_VARIANT="q4"
& .\target\t112-release\lumen.exe
```

## T303 — implementation and native review

Historical T303 device/menu descriptions below are superseded by T213/ADR-042 above.

Read specs/T303-images.md and ADR-041. Files: new lumen-image, storage migration 0005 /
images/catalog/content queue, content images/queue and catalog move verification;
embedding ImageInput/Embedder and ORT vision/backbone sessions; provision VISION_MODEL;
typed core/catalog/semantic/desktop DTO/Quick Look/UI and tray/indexing wiring.

Content-enabled local PNG/JPEG/WebP/BMP produce bounded dimensions/format/EXIF orientation
and a local SHA-256, one empty image chunk and one native visual vector. No filename or
caption enters embedding/FTS, no EXIF GPS/pixels retained. Names-only locations stay
inventory; unsupported/oversized/malformed files expose coverage without fake dimensions.
Same file ID/Open/Reveal/Copy path, generation/256d space, text/code/PDF vectors and fusion.
Absent vision defers images while text drains. Metadata resume and unchanged moves retain
vectors; edit/delete and changes during inference invalidate only the affected image.

Optional q4 vision component: 109,145,816 bytes including Apache model card, pinned to
daa72c51243991dfcaf9f9137d2c573d8f7790c0, explicit native consent/resume/hash/atomic install.
Tray → Semantic search → Download image search…; Remove image search… releases indexing
sessions on the writer before deleting only visual assets. Text/queries and stored vectors
remain. The release's normal provisioned q4 path supports images; q8/fp32 development
overrides do not. Image inference is CPU initially. The T212 probe measured only text;
its dedicated-GPU choice is not silently applied to the vision graph. The GPU menu now
says text indexing. CPU indexing reuses its text backbone to reduce retained memory.
One image/call; lazy load/unload, existing pause/duty/holds, wait on battery. Source limits
16 MiB/32M pixels/16,384 side, decoder allocation 192 MiB. Native calls cannot be forcibly
interrupted; holds/cancellation apply between images. No new WebView/hidden poll/process.

Release evidence/reproduction: docs/benchmarks/t303/2026-10-09-joao-pc/README.md.
Two public CC0 numeric-named photos rank correctly for English/Spanish cat/beach
descriptions, using actual vision → q4 backbone and independent CPU query lane. Initial
CPU run: 11.36 s cold / 9.32 s warm, two-image queue 18.20 s; resident snapshot 940 MiB
with query session. These are small loaded-machine samples, not peak/private/whole-app
memory, real-library ETA or general relevance guarantees. CPU backbone sharing avoids
~107 MiB retained compared with the first sample. Runtime/query-under-image and 100k
hot-name regressions are recorded separately in the README.
Bundled-runtime CPU image sample: 8.60/7.76 s; 30 uncached independent queries during
one image invocation p50/p95 53.76/63.14 ms. Native repeated-image and post-image text
fidelity tests pass with the bundled runtime, cosine >0.99999.

Browser component QA: actual ResultRow/PreviewPane at 1280×720 and 800×420, long names,
metadata/EXIF, pending/indexed/unsupported state and unknown dimensions. Light theme
observed; temporary fixture/server/tab/viewport override removed. No image raster preview
or OCR/Drop/Similar implemented. Native checks after the user's chosen Quit/switch:

1. Semantic search → Download image search…: consent names 109 MB/HF/Apache, cancel/resume,
   install without restart; remove without deleting model/text vectors or corrupting jobs.
   Unset LUMEN_EMBED_VISION_DIR to test the installed assets rather than development cache.
2. Add a small known photo folder, enable Index file contents. Content indexing shows
   image indexed/pending/skipped/failed separately. Alt+Enter distinguishes metadata
   coverage from visual meaning; missing encoder leaves pending, names-only remains unread.
3. After visual coverage is indexed, search a description (`gato type:image`, `playa`,
   `ocean ext:jpg`) whose words do not occur in the filename. Name results still arrive
   immediately; Ctrl+Enter/Reveal, Enter/viewer, Ctrl+K/Copy path, Alt+Enter and selection.
4. Pause/resume/restart; rename unchanged, edit during inference, delete/unavailable/cloud
   placeholders, malformed/oversized/unsupported formats. No stale visual result/vector,
   no re-embedding unrelated text/code/PDF. Battery pauses visual work; AC resumes.
5. Narrow/short monitors, light/dark/high contrast/100–150% DPI, photo-library soak and
   concurrent searching. Image inference takes seconds and already-running native calls
   finish cooperatively; no GPU-image acceleration or broad quality/throughput promise.

T303 optimized custom-protocol bundle: target/t303-release/lumen.exe, 21,338,112 bytes,
SHA-256 FFA2B737444662927E45E803DD27D99C41DE3E9492E118A60FBC653E25596F1B.
Frontend index-DimVfZd4.js embedded. Beside-exe DirectML/runtime
assets/notices match T212; PdfExtractorNotices.txt and ImageCodecNotices.txt included.
Do not launch beside the resident process expecting an upgrade; single-instance targets it.
After the user's chosen Quit, a cached-model development launch from repository root is:

```powershell
$env:LUMEN_EMBED_MODEL_DIR="$PWD\.cache\t006\embeddinggemma-2-ONNX"
$env:LUMEN_EMBED_VISION_DIR="$PWD\.cache\t303\model"
$env:LUMEN_ORT_DYLIB="$PWD\target\t303-release\onnxruntime.dll"
$env:LUMEN_EMBED_VARIANT="q4"
& .\target\t303-release\lumen.exe
```

## T302 — implementation and native review

Read specs/T302-pdf-preview.md and ADR-040. Files: lumen-windows pdf/pdf_viewer,
core PdfPage capability/open-pdf-page descriptor, catalog PDF enrichment, desktop
pdf_preview worker/commands/DTO/actions/overlay events and frontend usePreview/PreviewPane.
No new Cargo package/model/schema/vector generation or WebView; T301 extraction remains.

Alt+Enter renders the matched physical page; filename-only PDFs (including scans) start
at page 1. Previous/next/page entry and Return to match stay in the existing pane.
Alt+PageUp/Down navigate PDF pages; ordinary arrows/PageUp/Down continue navigating rows.
Ctrl+L returns query focus. Escape/Alt+Enter also work from controls; Return to match
restores query focus before its button disappears. Indexed text stays available in a
collapsible excerpt labelled with its original page, including when another page is viewed.
Enter/Open uses the normal file handler. Ctrl+K → Open matched PDF page is authorized
through ids/core capabilities: registered SumatraPDF uses separate documented arguments;
unknown handlers open Lumen's matched page preview, with Open file still available.
No guessed Edge/Adobe CLI, viewer installation, association edit or UI-supplied path.

Windows.Data.Pdf runs on one MTA worker, with one running/one latest pending job.
Cancellation before submission and older cleanup ordering are guarded. Hidden admission
is rejected atomically; hide cancels jobs and clears native/UI cache. Native object release
runs without the show/hide mutex. No idle polling; async status is inspected only during
requested work. Source/mtime/creation metadata and new queries invalidate cache (including
T207 known same-metadata changes, which start a fresh visible query).
Bounds: 16 MiB source, 512 pages, PNG edge 960 px, 6 MiB encoded output, four/12 MiB cached
rasters and one source document. Five-second cooperative deadline per load/render stage;
OS cancellation completion/native decoder heap are not hard sandbox guarantees. Placeholder,
encrypted, malformed/unsupported/limit failures keep metadata and indexed text. Explicit
preview of scans adds no OCR/vision indexing. No source-derived production disk cache.

Windows release evidence: docs/benchmarks/t302/2026-10-09-joao-pc/README.md and
pdf-preview.json. Synthetic 128-page PDF: cold first OS load/page 333.138 ms;
new pages p50/p95 22.679/39.169 ms, cache 0.035/0.053 ms. Resident snapshots 5.23→50.70 MiB,
not peak/whole-app memory. Timings exclude IPC/base64/paint, complex PDFs and inference.
Browser visual check at 1280x720 with actual Windows PNG, 800 px covered-list layout,
long names, navigation/Return to match, loading/unavailable matched text passed.
Light theme observed; native material/DPI/dark/high contrast and real viewer checks remain.
Screenshot is ignored target/t302-preview-qa.png; temporary fixture/server/tab removed.

Native checklist after the user's chosen Quit/switch to the latest bundle (includes T302):

1. Real text PDF with a later match/blank page, search with ext:pdf → Alt+Enter shows the
   physical matched page; filename-only and scan PDFs preview page 1 without new indexing.
2. Next/previous, page entry bounds, Return to match, Alt+PageUp/Down, Tab, Ctrl+L and
   Escape/Alt+Enter from controls. Plain arrows/PageDown keep result selection; refinement
   resets to the new matching page without moving the selected file. Check IME.
3. Ctrl+K → Open matched PDF page with an already-installed registered Sumatra handler
   opens that page, including spaces/Unicode paths and an already-open document. Other
   viewers get the Lumen page preview; Enter/Open file still uses their handler. Do not
   change the user's default viewer merely for validation.
4. Close/rapidly change results/hide during rendering: no late image, wrong page or hidden
   work. Edit/rename while the preview is open: new query refreshes, unchanged vectors stay.
5. Real scan-heavy/rotated/malformed/encrypted/large PDFs, inaccessible/cloud placeholder,
   narrow/short monitors, light/dark/high contrast, 100/125/150% DPI and longer memory soak.
   Unsupported rendering retains metadata/indexed text; no OCR/search coverage invented.

Delivered optimized custom-protocol target/t302-release/lumen.exe (20,562,944 bytes),
SHA-256 A666A2987126D0127928A50746FFC95C167E88832AD8698ACECB7BE7FD48DBAA.
Latest frontend index-BmPanQqu.js is embedded. Beside-exe DirectML/runtime assets/notices
are hash-matched to T212; PdfExtractorNotices.txt from T301 is included. No OS PDF DLL
is redistributed. The running T212 executable was left untouched.

## T301 — implementation and native review

Read specs/T301-pdf-text.md and ADR-039. Files: lumen-extract indexed/pdf, content pass,
shared move verification, ChunkRef.page_number, core PdfTarget/Payload/ResultKind,
catalog/semantic projection and fusion; desktop DTO/preview and typed frontend rows.
The frontend event decoder also required a fix: it discarded code metadata/kinds and
would discard PDF context. Real-event tests now cover both and strip extra payload fields.

Pinned lopdf 0.45.0 without default features extracts text only on the existing writer
thread, per physical page, preserving blank-page numbering. Limits: 16 MiB file,
512 pages, 4 MiB per load/page/font stream and combined text, cooperative 5 s deadline.
No hard parser-call interrupt/total heap sandbox; uncommon font/column reading order can
be imperfect. Cancellation leaves the PDF pending without partial chunks; immutable
scan/encrypted/malformed/limit failures have stable pdf:* coverage skip codes. I/O retries.

Existing page_number column is used: no schema migration, EXTRACTOR_VERSION/model/
generation/fusion-weight change. Text/code vectors remain. PDFs use the same resumable
embedding queue, selected CPU/GPU indexing backend and CPU queries; new PDF chunks must
be embedded before meaning retrieval. Name/lexical search works offline without a model.
One row per file has its best page/passage paired through fusion; exact filename rows
retain file presentation. Page prefix stays visible for long names. Enter opens the file,
Ctrl+Enter reveals, Ctrl+K uses existing file actions, Alt+Enter shows indexed page text.
T302 now renders Quick Look and provides page navigation; scans still need
future OCR/vision. Consent locations, exclusions/placeholders and OS paths stay unchanged.

Windows release evidence: docs/benchmarks/t301/2026-10-09-joao-pc/README.md and pdf-text.json.
128-page synthetic extraction p95 14.848 ms; 100 PDFs/600 chunks content pass 54.166 ms;
settled FTS p95 4.002 ms with 30 page-3 hits; unchanged resume processes 0 files.
Simple standard-font warm corpus, no model/GPU work, not a real-library ETA or peak heap
measurement. Browser component visual review at 1280x720 passed with page 7/123,
long titles, text preview and accessible selected row; native window checks remain.

Native checklist after the user's chosen quit/switch to the new bundle:

1. A location with Index file contents enabled: a real text PDF with a unique term on
   a later page and a blank page before it. Search that term with ext:pdf, check Page N
   matches the physical page. type:document and quoted phrases obey current filters.
2. With installed model, wait for those chunks to embed; a meaning query with ext:pdf
   must show the same kind of page/text context. Lexical works offline/model absent.
3. Arrows/PageDown before refinement, Alt+Enter followed by another query: preserve focus,
   selected identity and matching page. Enter opens the file with its normal handler;
   use the T302 page action/Quick Look above for the matched physical page.
   Ctrl+Enter reveals, Ctrl+K/Copy path and Escape work. Check IME and narrow preview.
4. Rename a PDF, edit its text, then restart: unchanged completed vectors survive;
   changed passages refresh. Names-only location must not extract the PDF.
5. Scan-only/encrypted/broken/oversized PDFs remain findable by filename and increment
   coverage skips; they do not acquire semantic/OCR text. T302 can preview ordinary scans.

Ship docs/licenses/pdf-extractor-notices.txt with existing model/runtime notices;
scripts/t301/pdf-notices.ps1 regenerates all 28 added pinned package notices offline.
alloc-stdlib's omitted root license is checked in and tied to its exact VCS commit.

Delivered target/t301-release/lumen.exe (custom-protocol optimized build), SHA-256
8E1C1938CC541A2DDFF20AC29C16677FF922A7DF637F62AB00810C8903E1EFDA.
Latest frontend index-yOz2oRLU.js is embedded. Beside-exe DirectML/runtime/notices match
the verified T212 bundle hashes; PdfExtractorNotices.txt is also included. The running
T212 executable was left untouched. Use the exact launch under T212 below only after
the user's chosen Quit Lumen; installed model assets need no development overrides.

## T208 — implementation and native review

Read docs/specs/T208-query-syntax.md. Files: lumen-core/query.rs; lumen-storage/filters.rs;
lumen-catalog provider/content/usage and tests/query_syntax.rs; lumen-semantic provider/
index_tests; lumen-bench/examples/query_syntax.rs. No schema/model/generation/weights,
UI/DTO, architecture or indexing change. Existing keyboard/actions/selection remain.

Root type/ext/in/before/after filters combine with AND across all lanes; known incomplete/
invalid filters fail closed. Dates are mtime/UTC calendar days; paths use directory
boundaries and ASCII case folding. Unknown operators/URLs/drive letters stay text;
quoted operators stay literal. Metadata-only queries enumerate names without a model.
Category lists in QueryType::extensions describe inventory, not media extraction.
Closed phrases require filename/FTS tokens in order, no prefix/typo inside quotes,
semantic expansion or two-of fallback. Bare text keeps existing behavior; open quotes
remain forgiving. Filters bind SQL parameters before lexical LIMIT; learned choices obey
them. Semantic overfetch is capped at 1,024 candidates/100 ms extra retrieval, potentially
partial for narrow filters; ordinary ANN and stale-row handling remain unchanged.

Release evidence: docs/benchmarks/t208/2026-10-09-joao-pc/README.md. 100k items/chunks:
names ordinary/filtered p95 13.429/13.086 ms; filtered content 0.372 ms; phrase 1.343 ms;
1,024 ANN-ID metadata check 1.678 ms. Zero empty measured queries. Parser p95 0.001136 ms.
Loaded-machine selective fixture, not visible paint/common-term worst cases/real-library
recall or full semantic embedding latency. Delta and persisted ANN filters are tested.

Native checks after the user's chosen switch to the new executable:

1. Existing file query with ext:rs / type:code / in:<actual directory>, then type:folder/
   type:image alone. type:image finds metadata/names; T208 adds no vision/OCR/PDF extractor.
2. Actual indexed phrase in quotes plus a bare word; impossible ext/date/path or incomplete
   ext: must not leak excluded rows. Date comparisons use UTC day boundaries.
3. Arrows/PageDown before settle, edit/clear filters, Enter/Ctrl+Enter/Ctrl+K/Alt+Enter/
   Escape: preserve focus, selected identity, local actions and matching passage.
4. Lexical operation offline/without a model. Current UI retains its existing no-results
   state for invalid syntax. Non-ASCII directory spelling follows existing SQLite semantics.

## T212 — implementation and launch

Read docs/specs/T212-gpu-indexing.md and ADR-038. Files: lumen-windows::gpu (DXGI/D3D12
non-UMA discovery), lumen-embedding::policy::accelerated_indexing_plan, pinned GPU_RUNTIME
in lumen-provision, desktop gpu.rs/gpu_probe.rs, tray/provisioning/indexing integration.
Native checkbox: Content indexing → Use dedicated GPU for faster indexing, off by default,
saved as indexing.gpu.enabled. Queries always build CPU sessions; indexing uses the
explicit dedicated adapter after its synthetic compatibility check. Same q4/prompts/256d
space/generation; completed vectors remain. Device failures quarantine that identity and
retry pending work on CPU. Mode changes take effect at queue/batch boundaries (30 s slices).
Ordinary Balanced defaults, battery/memory pauses, interactive holds and single writer remain.
No frontend/schema/model/ranking changes, Python runtime or production inference-worker IPC.

A pinned 26 MB DirectML 1.24.4 wheel can be installed through the same option, with native
size/host/license consent and SHA-256 for archive/members. ORT's library cannot switch in
process: installation asks for restart. On restart enabled GPU preference selects the
installed DirectML runtime ahead of a beside-exe CPU library; environment overrides win.
A GPU runtime already beside the exe supports toggling without another restart.
Development downloaded this fixed runtime wheel for hash/extraction validation; cached
models were reused. No indexed content/files/queries went into probes or left the device.

Usable optimized T302 output, including T208/T212 and verified DLLs/notices:
target/t302-release/lumen.exe. Keep the current process indexing. After the user chooses
Quit Lumen in the current tray, run from repository root:

```powershell
$env:LUMEN_EMBED_MODEL_DIR = "$PWD\.cache\t006\embeddinggemma-2-ONNX"
$env:LUMEN_ORT_DYLIB = "$PWD\target\t302-release\onnxruntime.dll"
.\target\t302-release\lumen.exe
```

Installed model/runtime assets need no development overrides. A stale CPU override masks
GPU support; remove it or point it to DirectML. Progress resumes from the existing queue.

Native checks still needing the user (automated synthetic checks are not visible QA):

1. Tray checkbox off/on, Checking → named GTX 1650 ready, progress showing GPU; CPU queries
   remain responsive from root search. First activation runs a bounded synthetic check.
2. Disable acceleration while work is pending: CPU resumes, counts continue and completed
   vectors stay. Restart after setting on/off: preference persists. Already drained queue
   remains drained; switching devices must not re-embed it.
3. Native keyboard access via notification area, menu focus, Pause/resume while probing,
   cancellation, failed/no-device/missing-runtime/restart states. Download consent/cancel/
   resume on an otherwise unconfigured machine still needs native review.
4. Battery/unknown power uses CPU, low battery/memory pauses. Probe defers under those
   conditions and user Pause, with existing queue retries and no new hidden timer.
5. Longer GPU driver/VRAM-pressure soak and other-GPU hardware remain unmeasured; do not
   intentionally reset the user's GPU or modify the live DB to test fallback.

Windows release evidence/reproduction: docs/benchmarks/t212/2026-10-09-joao-pc/README.md.
Native example uses a temporary DB and the actual CPU/GPU backends/query lane; child mode
exits before Tauri/SQLite. Small synthetic loaded-machine runs are not whole-library ETAs.
T014 q4 recheck: CPU 2.71 vs GTX 6.07 chunks/s, stable cosine 0.99999946, 94.43% offload,
2296 MiB sampled peak; query p95 67 vs 729 ms supports keeping queries on CPU.
Evidence: docs/benchmarks/t014/2026-10-09-joao-pc-gpu-recheck/.
Full T014 thread/llama.cpp matrix and ADR-030 Windows budget verdict remain pending.

## T207 — native review remaining

Read docs/specs/T207-incremental-indexing.md and ADR-037. Native watch/scan_changed,
scoped catalog reconciliation, storage invalidation/content comparison and visible-query
refresh are implemented. Schema/model/extractor/ranking/UI contracts stay unchanged.
Unchanged moves preserve embeddings; ambiguous Windows rename+modify hints compare bounded
indexed chunks within content consent. ANN sequence checks reject stale hits.

With the new T302 executable above (includes T207/T208/T212):

1. Add a scratch folder, enable Index file contents, create/edit text/code including an
   editor replacement save. Names/words refresh without rescan; new vectors become pending.
   Old passages disappear. Rename file/folder: Open/Reveal/Copy path/code context follow.
2. Keep overlay/query open through changes: surviving selection stays at its position;
   Alt+Enter and Ctrl+K refresh appropriately. Hidden commits start no JS query/inference;
   next Alt+Space refreshes once.
3. Change roots/exclusions during indexing, including marker rules and contents-disabled
   locations. Disconnect/reconnect a removable root: unverified items remain, recovery
   and watching resume. Unsupported/network watching retains periodic inventory fallback.

Evidence: docs/benchmarks/t207/windows-watch.json, 10,001 synthetic items, 20 operations,
lexical freshness 361/381 ms p50/p95 including 300 ms debounce, 29 entries emitted, zero
full inventories, rename vectors preserved. Two seconds parked: zero notifications/
measured CPU; not a long idle/network/storm soak or visible paint measurement.

## T209 — native review remaining

Read ADR-036. Content indexing must be enabled for the scratch repository.

1. Search a symbol/get_with_retry or a code description: settled rows gain symbol/file,
   language/repository context and matching passage. Exact filenames retain navigation.
2. Enter uses registered handler; Ctrl+Enter reveals. Ctrl+K → Copy symbol pastes exactly;
   Reveal repository selects nearest Git root (.git marker files supported). Unknown
   symbol/repository means no corresponding action. Precise editor launching is not built.
3. Alt+Enter shows matching indexed passage, including beyond first 16 KB; early previews
   refine on same file identity. Escape closes actions, preview, overlay. Arrow/PageDown
   selection survives refinements.
4. Restart retains context; moves clear stale repository/language and T207 rediscovers it.
   Newly created/deleted repository markers are watched by T207's scoped reconciliation.

Migration 0004 preserves chunks/vectors/sequences/generations. Backfill is on the content
worker; startup schema migration is on a named worker before first show. Evidence:
docs/benchmarks/t209/2026-10-09-joao-pc/ (100k upgrade 1.10 s, backfill 1.60 s; six code
queries top-1 0.50→1.00; q4 fused NDCG 0.964). Synthetic UI screenshot is ignored under
target/t209/. Native clipboard/Explorer/default-handler checks remain.

## Older native REVIEW checks

- T003: Keyboard shortcut → Ctrl+Space persists; conflicts preserve old shortcut.
- T004/T103: Acrylic vs Mica verdict, light/dark, 100/125/150% DPI, long paths, corners,
  shadow/text over busy backgrounds. Transparency off/high contrast uses Solid on show.
  Optional: powershell -ExecutionPolicy Bypass -File scripts\t004\run-windows-material.ps1
- T107/T104/T108/T109/T105/T110: first catalog, keystroke results, arrows/PageUp/PageDown/
  Ctrl+L/IME, Enter/Ctrl+Enter/Ctrl+K, usage ranking, Alt+Enter, fixed search bar/Escape order.
  LUMEN_DIAGNOSTICS=1 exposes provider/match/confidence and local timing only.
- T111: Indexed locations → Add D:\Proyectos, remove, USB recovery, noise/marker exclusions,
  Action Panel Exclude folder / Include again and restart persistence.
  powershell -ExecutionPolicy Bypass -File scripts\t111\run-windows-locations.ps1 -Drive D:\
- T202: content progress, remembered Pause, per-location content off, CPU share/idle/battery.
  scripts\t202\run-windows-indexing.ps1 -Root D:\Proyectos\lumen; -Launch -SkipBench starts
  an app, so wait for the user's chosen quit first and select the intended runtime.
- T205/T206: descriptive queries refine into contents/meaning, move ↓ twice before settle,
  same selected position/snippet/Quick Look. Installed model required; lexical works offline.
- T210: unset development overrides, Semantic search → Download… consent/progress/cancel/
  resume/atomic status; original model+CPU runtime 222 MB, fixed hosts/licenses; available
  without restart. Remove deletes the model. GPU runtime has its own restart rule above.
  No installer/signing/About screen or WinHTTP transport; system curl honors HTTPS_PROXY.

## Other pending Windows evidence

These may download sizeable models or take minutes; do not start them casually alongside
resident indexing. Record counts/timings under each task's docs/benchmarks/ directory.

```powershell
# Full T014 matrix (~3 GB first download, 20–40 min, plugged in/idle):
powershell -ExecutionPolicy Bypass -File scripts\t014\run-windows-throughput.ps1 -Download
# T204 Windows verdict (T006 cache, ~10 min; preempted b1 p95 ≤80 ms):
powershell -ExecutionPolicy Bypass -File scripts\t204\run-windows-query-lane.ps1
# Optional large ANN timing:
powershell -ExecutionPolicy Bypass -File scripts\t203\run-windows-ann-gen.ps1 -Large
# Original T205 evaluation (T209 hard-set evidence is separate):
powershell -ExecutionPolicy Bypass -File scripts\t205\run-windows-eval.ps1
```

## Automated validation and reproduction

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features lumen-bench/directml --locked -j 2 -- -D warnings
cargo xtask test --locked -j 2
cargo xtask arch
npm --prefix apps/desktop run format:check
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test -- --maxWorkers 1
npm --prefix apps/desktop run build
cargo build --release -p lumen-desktop --bin lumen --features tauri/custom-protocol --locked -j 2
cargo run --release -p lumen-bench --example pdf_preview --locked -j 2 -- target/t302-pdf-preview.json
cargo test -p lumen-windows --test pdf_render --locked -j 2
```

T112 gate: 371 Rust tests passed (including 37 shell), 2 network tests deliberately
ignored; fmt, workspace/DirectML clippy and 15-crate architecture guard. Frontend full
check and build passed, 83 tests. Release build/benchmark use --target-dir target/t112-build.
Final selected-root name-rule boundary regression also passes: 44 release catalog tests,
release catalog/example lint and repeated final-source benchmark. Reproduce cleanup with
`cargo run --release --locked --target-dir target/t112-build -p
lumen-bench --example exclusions -- target/t112-exclusions.json`. Build with `cargo build
--release --locked --target-dir target/t112-build -p lumen-desktop --bin lumen --features
tauri/custom-protocol -j 2`; never overwrite a running bundle. Logs: target/t112-*.log.

T303 full Rust gate: 366 passed, 2 network tests deliberately ignored, including 37 shell
tests; workspace/DirectML lint, 15-crate architecture guard, fmt, 82 frontend tests/check/
build. Actual native CPU vision/text-space test, release image/retrieval/query regression
and optimized custom-protocol desktop build recorded in the T303 evidence. Earlier PDF
and GPU tests remain; native overlay/library/viewer/tray checks are deferred by the user.
Logs are ignored .cache/t303/*.log. T301 extraction reproduction: example pdf_text;
T302 renderer: example pdf_preview and lumen-windows --test pdf_render.
T212 previously ran the optional local-wheel test with LUMEN_TEST_GPU_WHEEL_DIR=.cache/t212; normal
tests skip it without a supplied wheel. Real network/model download tests remain ignored.
See T212 README for native release probe/example commands and limitations.

## Remaining decisions / environment

Default material Acrylic vs Mica is the user's verdict. T014 broader runtime/thread verdict,
ADR-030 Windows query-lane verdict, precise editor transport, log/data two_of tuning and
TX01 remain open. Cached model/runtime: .cache/t006/{embeddinggemma-2-ONNX,ort-cpu,ort-dml}.
ANN files live under app-data vectors/; first build around 2,000 chunks. No cloud inference.
Inspect current Git locks before changing them; .git/stale-locks/ is older sandbox residue.
CI should be pushed/run only when authorized; this session does not push.
