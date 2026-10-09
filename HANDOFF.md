# HANDOFF.md

> Live continuation only. Read AGENTS.md in order; TASKS.md owns status and Next.
> Rewrite each session; history belongs in WORKLOG and evidence/ADRs.

## Branch and exact continuation

main on github.com/Juancinho/lumen. This session continues 69d741f (T207) with T212,
explicitly requested by the user: dedicated-GPU indexing, CPU queries, available VRAM.
Inspect git status/log for the final coherent T212 commit. No push was requested/performed.
T212 is REVIEW; no implementation task remains claimed. Its native tray review below
remains; next implementation is T208 query syntax.
Claim T208 after the canonical reading order and its relevant specs/ADRs. Do not begin
photo/PDF/multimodal work, change weights/ranking or redo completed providers.

The original T207 process PID 11356/start 15:59:27 stayed running through the performance
checks. No agent restart or live app-data DB writes were used for validation; all test
writes were synthetic/temporary. At the user's later continuation, the original process
had been replaced independently by PID 8884/start 17:29:01, already running
target/t212-release/lumen.exe. Do not stop it or modify its DB for validation.
Read-only checks at session end: indexing.gpu.enabled=true, no cached probe/quarantine,
Pause=false; the process still loads .cache/t006/ort-cpu/onnxruntime.dll (development
override), so GPU is not active yet. AC/battery 100%. The user needs one chosen Quit and
relaunch with the DirectML override below. A second ordinary launch only addresses the
current resident instance. Inspect current PIDs/state again before acting.

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

Usable optimized output, with verified DLLs and notices: target/t212-release/lumen.exe.
The user must first choose Quit Lumen in the current tray, then run from repository root:

```powershell
$env:LUMEN_EMBED_MODEL_DIR = "$PWD\.cache\t006\embeddinggemma-2-ONNX"
$env:LUMEN_ORT_DYLIB = "$PWD\target\t212-release\onnxruntime.dll"
.\target\t212-release\lumen.exe
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

With the new T212 executable above (includes T207):

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
cargo clippy -p lumen-bench --features directml --all-targets --locked -j 2 -- -D warnings
cargo xtask test --locked -j 2
cargo xtask arch
npm --prefix apps/desktop run format:check
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test -- --maxWorkers 1
npm --prefix apps/desktop run build
cargo build --release -p lumen-desktop --bin lumen --example gpu_indexing --features tauri/custom-protocol --locked -j 2
```

T212 ran the full Rust gate, DirectML lint, architecture guard and 71 frontend tests/build.
Later source changes were rechecked with workspace lint and content/desktop/provision tests.
The optional local-wheel test was run with LUMEN_TEST_GPU_WHEEL_DIR=.cache/t212; normal
tests skip it without a supplied wheel. Real network/model download tests remain ignored.
See T212 README for native release probe/example commands and limitations.

## Remaining decisions / environment

Default material Acrylic vs Mica is the user's verdict. T014 broader runtime/thread verdict,
ADR-030 Windows query-lane verdict, precise editor transport, log/data two_of tuning and
TX01 remain open. Cached model/runtime: .cache/t006/{embeddinggemma-2-ONNX,ort-cpu,ort-dml}.
ANN files live under app-data vectors/; first build around 2,000 chunks. No cloud inference.
Inspect current Git locks before changing them; .git/stale-locks/ is older sandbox residue.
CI should be pushed/run only when authorized; this session does not push.
