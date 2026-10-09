# HANDOFF.md

> Live continuation only. Read AGENTS.md in order; TASKS.md owns status and Next.
> Rewrite each session; history belongs in WORKLOG and evidence/ADRs.

## Branch and exact continuation

main on github.com/Juancinho/lumen. This session continues d2b9f55 (T208) with the
next ordered T301 while the user's resident application indexes. Inspect git status/log
for the coherent T301 commit. No push was requested/performed.
T301 is REVIEW; no implementation task remains claimed. Native checks below remain.
Next implementation is T302 PDF thumbnails/Quick Look and page navigation. Follow the
canonical reading order, then SEARCH_AND_INDEXING §10, RELEASE_AND_LICENSING,
specs/T301-pdf-text.md and ADR-039/036/032/025. Claim T302 before implementation, define
its bounded rendering/viewer contract and license evidence, and reuse current PDF targets,
actions, preview lifecycle and index. Do not add image/OCR/productivity/workflow scope.

Resident inspected read-only: PID 7404/start 2026-10-09 18:04:57 +02:00,
target/t212-release/lumen.exe. Previous native module inspection found its beside-exe
onnxruntime.dll and DirectML.dll. Do not infer current GPU use solely from loaded DLLs.
The agent did not stop/restart it, alter its settings or write its live app-data DB.
All validation writes used synthetic temporary databases. Inspect current PIDs before
acting; a second launch addresses the existing single-instance process until it quits.

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
Rendered thumbnails/page previews and direct page navigation remain T302; scans need
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
   selected identity and matching page. Enter opens the file (not yet at Page N),
   Ctrl+Enter reveals, Ctrl+K/Copy path and Escape work. Check IME and narrow preview.
4. Rename a PDF, edit its text, then restart: unchanged completed vectors survive;
   changed passages refresh. Names-only location must not extract the PDF.
5. Scan-only/encrypted/broken/oversized PDFs remain findable by filename and increment
   coverage skips; they do not acquire invented semantic/OCR text. No page images yet.

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

Usable optimized T301 output, including T208/T212 and verified DLLs/notices:
target/t301-release/lumen.exe. Keep the current process indexing. After the user chooses
Quit Lumen in the current tray, run from repository root:

```powershell
$env:LUMEN_EMBED_MODEL_DIR = "$PWD\.cache\t006\embeddinggemma-2-ONNX"
$env:LUMEN_ORT_DYLIB = "$PWD\target\t301-release\onnxruntime.dll"
.\target\t301-release\lumen.exe
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

With the new T208 executable above (includes T207/T212):

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
cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings
cargo xtask test --locked -j 2
cargo xtask arch
npm --prefix apps/desktop run format:check
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test -- --maxWorkers 1
npm --prefix apps/desktop run build
cargo build --release -p lumen-desktop --bin lumen --features tauri/custom-protocol --locked -j 2
cargo run --release -p lumen-bench --example pdf_text --locked -j 2 -- target/t301-timing.json
```

T301 ran the full Rust gate (349 passed, 2 network tests ignored; 34 shell tests included),
workspace lint plus DirectML-feature lint, the 14-crate architecture guard and 76 frontend
tests/build. PDF extraction/FTS release measurements and browser component visual review
passed; native overlay/real-PDF checks remain. T212's DirectML/runtime checks remain valid.
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
