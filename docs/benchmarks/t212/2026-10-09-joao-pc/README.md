# T212 — dedicated-GPU indexing, CPU queries

Windows x64, Ryzen 5 5600H (6C/12T), GTX 1650 4096 MiB, driver 32.0.15.9227.
Cached EmbeddingGemma 2 q4, production retrieval prompts, L2-normalized 256d vectors,
DirectML runtime 1.24.4 for both CPU and GPU sessions. Release/LTO builds; T212 working
tree based on 69d741f. Resident T207 Lumen PID 11356 was kept indexing throughout.
All benchmark writes use synthetic temporary DBs/reports; no live DB writes/restarts.
Compile/lint processes finished before performance measurement. Results are loaded-machine,
small synthetic checks, not an idle hardware verdict, whole-files throughput or an ETA.

## Evidence

- cpu-queries-directml-runtime.json: release lumen-bench CPU provider, four threads,
  5 warmups/40 measured queries; p50/p95 52.03/63.68 ms, within 60/120 ms budgets.
  32 approximately 100-word documents, b8, 3.79 chunks/s, about 3.91 busy CPU cores.
  This CPU measurement uses the existing benchmark/backend; shell resolution is validated
  separately by the new executable's child mode and native queue example.
- gpu-queue-cpu-queries.json: release desktop example using production gpu_probe.rs,
  DXGI/D3D12 explicit dedicated-adapter discovery (rejects the integrated Radeon),
  CPU/GPU real backends, the persistent run_queue and actual QueryEmbedder/Control.
  26 synthetic chunks: CPU prefix 2, GPU bulk 16, GPU tail 8. The example asserts the
  same generation/space, untouched CPU-prefix vectors, drained queue and zero re-embedding
  when switching back to CPU. Forty unique CPU queries are timed with a GPU queue that
  is held/preempted by the query lane. GPU compute need not be concurrent with each query;
  the unrelated resident CPU indexer remains active. Bulk GPU throughput excludes warmup.
  GPU bulk 6.80 chunks/s; CPU query p50/p95 53.68/64.70 ms. Its warmed compatibility
  probe admitted the GPU (3.84 CPU / 9.11 GPU chunks/s, 2.37×, min cosine 0.99999982,
  94.43% offload). DXGI reports 3935 MiB dedicated memory; per-process usage 906 MiB
  is sampled after inference, so it is not equivalent to T014's sampled 2296 MiB peak.
- native-child-probe.json: the final GUI executable --gpu-probe path, exiting before Tauri,
  settings/SQLite and single-instance setup, with the runtime beside the delivered exe.
  Synthetic 8-document (~128-token) compatibility probe, 2 query warmups/8 measured queries,
  one untimed document warmup and two document passes for stability, CPU six threads /
  GPU one host thread. This is a small compatibility gate, not a soak.
  CPU/GPU use the same runtime and report matching weights/prompts/256d space. Memory is
  per-process DXGI CurrentUsage sampled after inference, not a sampled whole-run peak.
- probe-isolation.json: malformed request exits 1 and produces no report; original resident
  PID/start remains unchanged. An integrated/invalid adapter request is rejected as well.
- Native tray visual/keyboard/persistence, optional-download consent/cancel/restart and long
  GPU-pressure/other-hardware soak checks remain in HANDOFF. Unit tests cover cached report
  rejection, battery/unknown-power/memory gating, fidelity/speed/offload/quarantine; queue
  tests cover lost-device atomicity and same-generation CPU recovery with old vectors retained.

The earlier T014 ~260-token cross-runtime comparison is in
the repository-relative directory
docs/benchmarks/t014/2026-10-09-joao-pc-gpu-recheck/: 2.71 CPU vs 6.07 GPU chunks/s,
query p95 67 vs 729 ms, sampled GPU peak 2296 MiB. Do not compare different corpora/memory
sampling as equivalent runs. No model download was needed. One pinned 25,111,930-byte
DirectML wheel was downloaded for development manifest/hash/extraction verification,
then used by DirFetch's optional local-wheel install test without network.

## Reproduce

Run from repository root after other compilation has ended. Do not stop or write the
resident app for these synthetic checks. Requests below contain local software paths;
keep them in ignored target/, not committed evidence.

```powershell
npm --prefix apps/desktop run build
cargo build --release -p lumen-desktop --bin lumen --example gpu_indexing --features tauri/custom-protocol --locked -j 2
New-Item -ItemType Directory -Force target/bench/t212 | Out-Null
@{
  key='t212-native-smoke'
  model=(Resolve-Path .cache/t006/embeddinggemma-2-ONNX).Path
  runtime=(Resolve-Path .cache/t006/ort-dml/onnxruntime.dll).Path
  variant='q4'; adapter=0; name='native discovery'; total_mib=4096; threads=6
} | ConvertTo-Json | Set-Content target/bench/t212/request.json -Encoding utf8NoBOM
# Example discovers the actual discrete adapter and overrides placeholder hardware fields:
.\target\release\examples\gpu_indexing.exe target/bench/t212/request.json target/bench/t212/queue.json
# Use its actual adapter/name in the standalone child request:
$report=Get-Content target/bench/t212/queue.json -Raw | ConvertFrom-Json
$request=Get-Content target/bench/t212/request.json -Raw | ConvertFrom-Json
$request.adapter=$report.adapter
$request.name=$report.gpu_name
$request.total_mib=[uint64]$report.compatibility_probe.gpu.device_memory_total_mib
$request | ConvertTo-Json | Set-Content target/bench/t212/request.json -Encoding utf8NoBOM
$child=Start-Process target/release/lumen.exe -ArgumentList @('--gpu-probe',
  "$PWD\target\bench\t212\request.json", "$PWD\target\bench\t212\child.json") -WindowStyle Hidden -PassThru
$child.WaitForExit(120000) | Out-Null
if (!$child.HasExited -or $child.ExitCode -ne 0) { throw 'GPU probe failed/timed out' }
# Optional pinned-wheel extraction/install validation (no network):
# Supply the pinned URL filename under .cache/t212/ first; see GPU_RUNTIME manifest.
$env:LUMEN_TEST_GPU_WHEEL_DIR="$PWD\.cache\t212"
cargo test -p lumen-provision --locked -j 2 pinned_gpu_runtime_installs_from_an_optional_local_wheel
Remove-Item Env:LUMEN_TEST_GPU_WHEEL_DIR
```

Delivered development folder target/t212-release/ contains lumen.exe, onnxruntime.dll,
onnxruntime_providers_shared.dll, DirectML.dll, LICENSE and ThirdPartyNotices.txt.
The three DLL/member hashes match GPU_RUNTIME. Building does not install or start the app.
The user chooses Quit Lumen, launches this folder with the cached model path and selects
Content indexing → Use dedicated GPU for faster indexing. Progress is resumed, not reset.
