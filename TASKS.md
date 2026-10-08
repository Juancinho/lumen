# TASKS.md

Statuses: `TODO`, `CLAIMED`, `BLOCKED`, `REVIEW`, `DONE`.
Owners: `codex`, `claude`, `human`, or explicit agent/worktree.

Do not renumber task IDs. New tasks get new IDs.

# M0 — technical spikes and foundation

| ID   | Status | Owner  | Task                                                                                                                 | Depends on |
| ---- | ------ | ------ | -------------------------------------------------------------------------------------------------------------------- | ---------- |
| T001 | DONE   | claude | Initialize Tauri 2 + React/TS + Rust workspace; enforce shell → core dependency direction; lint/format/test baseline | —          |
| T002 | DONE   | claude | Overlay prototype: borderless window, show/hide/focus/Escape/tray                                                    | T001       |
| T003 | REVIEW | claude | Configurable global shortcut + conflict/error UX                                                                     | T002       |
| T004 | REVIEW | claude | Windows material/backdrop spike: Mica foundation, Acrylic/translucent fallback, shadows/rounding                     | T002       |
| T005 | DONE   | claude | `EmbeddingBackend` interface + inference benchmark harness                                                           | T001       |
| T006 | DONE   | claude | Benchmark viable Windows EmbeddingGemma runtimes; write ADR                                                          | T005       |
| T007 | DONE   | claude | SQLite schema + migrations + WAL + FTS5 proof                                                                        | T001       |
| T008 | DONE   | claude | USearch 256d 100k/1M benchmark; f32/f16 candidate comparison                                                         | T001       |
| T009 | DONE   | claude | File enumeration + stable identity spike                                                                             | T001       |
| T010 | DONE   | claude | CI baseline + release-mode benchmark command                                                                         | T001       |
| T011 | DONE   | claude | Minimal universal domain contracts: `ResultItem`, `ProviderId`, `ActionDescriptor`, typed execution context          | T001       |
| T012 | DONE   | claude | WebView lifecycle/RAM spike: one WebView, hidden state, timers, optional suspension/low-memory path                  | T002       |
| T013 | DONE   | claude | Embedding device selection + fallback policy: CPU default; GPU/NPU only when placement + probe beat CPU; battery/memory profiles; never switch weights inside an index generation | T006       |
| T014 | TODO   | —      | Indexing-throughput runtime spike: LiteRT-LM (QAT int4, 270M text model), llama.cpp GGUF (CPU/Vulkan/CUDA), Windows ML EPs (TensorRT-RTX/OpenVINO/WebGPU); reuse fidelity harness | T006       |
| T015 | TODO   | —      | Docs consolidation (no code): §0 implementation status in each spec + fix sections contradicted by ADR-014..018; merge "Refinement" appendices; crate layout only in DEVELOPMENT.md; reading order only in AGENTS.md; split ADRs into docs/adr/ with DECISIONS.md as index; fixed roles for TASKS/HANDOFF/WORKLOG/PROJECT_STATE; indexing-throughput (chunks/s @ CPU %) budget and one memory metric in PERFORMANCE.md; ordered "Next" list in TASKS.md. Start after T013 merges (touches shared docs) | T013       |
| T016 | TODO   | —      | Fix `lumen-bench storage` corpus: no query is covered by the 50-word synthetic vocabulary, so final FTS queries always return 0 hits and typing p50 is optimistic (p95/budget worst case stays valid). Mix query terms in with a skewed distribution, report hits per query, warn when mean hits = 0; re-measure and correct ADR-017 numbers. After T013 (may touch lumen-bench) | T007,T010  |

# M1 — instant launcher and universal surface

| ID   | Status | Owner | Task                                                                          | Depends on     |
| ---- | ------ | ----- | ----------------------------------------------------------------------------- | -------------- |
| T101 | DONE   | claude | App/file catalog provider                                                     | T007,T009,T011 |
| T102 | DONE   | claude | Filename/path exact/prefix/fuzzy retrieval                                    | T101           |
| T103 | REVIEW | claude | Premium root-search overlay from design system                                | T002,T004      |
| T104 | REVIEW | claude | Keyboard navigation, stable selection, root/action-panel shortcuts            | T103,T011      |
| T105 | TODO   | —     | Quick Look preview shell                                                      | T103           |
| T106 | DONE   | claude | Recent/frequency/pin signals + local usage store                              | T101           |
| T107 | REVIEW | claude | Progressive result stream Rust → UI                                           | T102,T103      |
| T108 | REVIEW | claude | Universal Action Panel (`Ctrl+K`/Tab policy) with contextual action discovery | T104,T011      |
| T109 | REVIEW | claude | Core actions: open, reveal, copy path/value, pin/favorite, open with          | T108           |
| T110 | REVIEW | claude | Provider/result diagnostics mode for development only                         | T107           |

# M2 — text/code semantic search

| ID   | Status | Owner | Task                                                                    | Depends on          |
| ---- | ------ | ----- | ----------------------------------------------------------------------- | ------------------- |
| T201 | TODO   | —     | Text/code extractor + language-aware chunker (Tree-sitter where useful) | T006,T007           |
| T202 | TODO   | —     | Background embedding queue with bounded backpressure/cancel/pause       | T201                |
| T203 | TODO   | —     | Persistent ANN + generation/version management                          | T008,T202           |
| T204 | TODO   | —     | Warm query embedding service + cancellation                             | T006                |
| T205 | TODO   | —     | Hybrid provider/lexical/vector fusion + evaluation harness              | T203,T204,T102,T011 |
| T206 | TODO   | —     | Progressive semantic UI refinement without focus jumps                  | T107,T205           |
| T207 | TODO   | —     | Incremental watcher/reindex/delete/rename                               | T009,T202           |
| T208 | TODO   | —     | Query syntax: type/ext/in/before/after/quoted exact                     | T205                |
| T209 | TODO   | —     | Code result model: symbol/file/repository context + code actions        | T201,T108           |

# M3 — PDF/image intelligence and semantic objects

| ID   | Status | Owner | Task                                                         | Depends on     |
| ---- | ------ | ----- | ------------------------------------------------------------ | -------------- |
| T301 | TODO   | —     | PDF text/page extraction + page-level hits                   | T201           |
| T302 | TODO   | —     | PDF thumbnails/Quick Look + jump/open-page action            | T301,T105,T108 |
| T303 | TODO   | —     | Image metadata + vision embedding                            | T006,T202      |
| T304 | TODO   | —     | Optional Windows OCR enrichment for screenshots/images       | T303           |
| T305 | TODO   | —     | Semantic Drop: paste/drag image/file/text as query object    | T303,T205      |
| T306 | TODO   | —     | `Find Similar` action for supported semantic items           | T205,T303,T108 |
| T307 | TODO   | —     | Related-content primitive for Context Lens (no graph UI yet) | T205           |

# M4 — Alfred/Raycast-class daily utility

| ID   | Status | Owner | Task                                                                           | Depends on |
| ---- | ------ | ----- | ------------------------------------------------------------------------------ | ---------- |
| T401 | TODO   | —     | Stabilize internal provider registry: files/apps/commands/productivity         | T205,T011  |
| T402 | TODO   | —     | Calculator/conversions deterministic provider                                  | T401       |
| T403 | TODO   | —     | Windows settings/system command provider                                       | T401,T108  |
| T404 | TODO   | —     | Quicklinks with parameter placeholders                                         | T401,T108  |
| T405 | TODO   | —     | Snippets with variables/date/clipboard placeholders                            | T401,T108  |
| T406 | TODO   | —     | Clipboard history provider, opt-in, encrypted/sensitive-data controls          | T401       |
| T407 | TODO   | —     | Smart Collections / saved semantic searches                                    | T205,T401  |
| T408 | TODO   | —     | Window-management actions/provider if Windows API spike validates UX           | T401,T108  |
| T409 | TODO   | —     | Search/command history + learnable local ranking signals with privacy controls | T401       |

# M5 — workflows and command center

| ID   | Status | Owner | Task                                                                          | Depends on |
| ---- | ------ | ----- | ----------------------------------------------------------------------------- | ---------- |
| T501 | TODO   | —     | Workflow data model + validation + permission model                           | T401,T108  |
| T502 | TODO   | —     | Declarative workflow runner: sequential actions, parameters, safe failure     | T501       |
| T503 | TODO   | —     | Workflow trigger provider in root search                                      | T502       |
| T504 | TODO   | —     | Workflow editor/minimal management UI (not node-canvas unless justified)      | T502       |
| T505 | TODO   | —     | Built-in workflow templates: dev project, open workspace, transform clipboard | T502       |
| T506 | TODO   | —     | Shell/process execution action with explicit safety boundaries                | T501       |

# M6 — memory, projects and context

| ID   | Status | Owner | Task                                                             | Depends on |
| ---- | ------ | ----- | ---------------------------------------------------------------- | ---------- |
| T601 | TODO   | —     | Temporal "recent work" query interpretation using local metadata | T205,T409  |
| T602 | TODO   | —     | Semantic Workspace clustering/entity model                       | T307,T601  |
| T603 | TODO   | —     | Workspace surface: recent/related/code/media/activity            | T602,T103  |
| T604 | TODO   | —     | Context Lens UI over related-content primitive                   | T307,T108  |
| T605 | TODO   | —     | Optional Rewind event journal with retention/privacy controls    | T601       |
| T606 | TODO   | —     | Rewind query surface: "what was I working on..."                 | T605,T205  |

# M7 — audio/video

| ID   | Status | Owner | Task                                                    | Depends on     |
| ---- | ------ | ----- | ------------------------------------------------------- | -------------- |
| T701 | TODO   | —     | Audio decoding/segmentation/direct embedding            | T006,T202      |
| T702 | TODO   | —     | Video sparse/scene-aware sampling + direct embedding    | T006,T202      |
| T703 | TODO   | —     | Timecoded media results + play-from-time actions        | T701,T702,T108 |
| T704 | TODO   | —     | Optional local transcription enrichment after benchmark | T701           |

# M8 — extensibility and hardening

| ID   | Status | Owner | Task                                                                       | Depends on         |
| ---- | ------ | ----- | -------------------------------------------------------------------------- | ------------------ |
| T801 | TODO   | —     | Review internal provider/action APIs for public stability                  | T401,T501          |
| T802 | TODO   | —     | Design extension manifest/capability/permission model                      | T801               |
| T803 | TODO   | —     | Sandboxed extension host spike; decide in-process vs subprocess/WASM/other | T802               |
| T804 | TODO   | —     | Extension SDK/store/discovery only if product usage justifies it           | T803               |
| T805 | TODO   | —     | Accessibility audit, localization readiness, crash recovery, update path   | all prior relevant |
| T806 | TODO   | —     | Privacy/security review and threat-model closure                           | all prior relevant |
| T807 | TODO   | —     | Release packaging/signing/licensing audit                                  | all prior relevant |

# Deferred spikes — only when triggered

| ID   | Status | Owner | Task                                                          | Trigger                                                         |
| ---- | ------ | ----- | ------------------------------------------------------------- | --------------------------------------------------------------- |
| TX01 | TODO   | —     | Tauri vs FastFrame/egui equivalent minimal launcher benchmark | After M1 baseline OR if RAM/startup target is missed            |
| TX02 | TODO   | —     | Split indexer into separate worker process                    | Only if crash isolation/runtime/resource evidence justifies IPC |
| TX03 | TODO   | —     | Optional local/remote LLM action layer                        | Only after deterministic command/search/action system is mature |
