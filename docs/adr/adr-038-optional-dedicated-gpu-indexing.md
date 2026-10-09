# ADR-038 — user-selected dedicated-GPU indexing, CPU queries

**Status:** Accepted for T212 (explicit user request, Windows release evidence, 2026-10-09).

## Evidence and decision

T014 recheck on the Ryzen 5 5600H / GTX 1650 4 GB, q4, ORT CPU 1.30 / DirectML
1.24.4: indexing 2.71 / 6.07 chunks/s, query p95 66.74 / 728.92 ms, GPU vectors
stable with min cosine 0.99999946 vs CPU, 94.43% graph offload, 2296 MiB VRAM.
The resident indexer remained running: these are loaded-machine results, not idle
acceptance evidence or a completion-time estimate. Corpus: T013's ~260-token samples.
Evidence: `docs/benchmarks/t014/2026-10-09-joao-pc-gpu-recheck/`.

The user specifically requested GPU indexing, CPU queries and waived the default VRAM
limit. Add a persisted, off-by-default native tray option. Default Balanced policy is
unchanged. The requested mode keeps the correctness, stability, same-space and 1.5×
speedup gates, while allowing up to available dedicated video memory on AC, even during
user activity. Battery, unknown power and memory pressure retain the ordinary CPU plan
or pause. CPU fallback retains Balanced thread limits, not Turbo's CPU thread count.

GPU discovery uses DXGI and D3D12 architecture to reject software and UMA devices;
unknown hardware is excluded. Adapter index is explicit, so a preferred-device heuristic
cannot select the integrated Radeon; choose the eligible adapter with most dedicated VRAM.
Probes use synthetic inputs in a bounded child of
the same executable, before Tauri/SQLite startup, with a 120-second timeout. The resident
process remains the only application/index writer; no inference-worker IPC architecture
is introduced. Cache/quarantine are keyed by adapter/driver/runtime/model/tokenizer identity.
One untimed document batch warms each backend before measuring throughput, avoiding a
first-use DirectML shape-setup penalty that rejected the faster steady-state bulk lane.
Probes defer on battery/unknown power, memory pressure and user Pause; queue retries resume
them without another hidden timer. Runtime identity includes companion DLL hashes.

Queries always create a CPU session. Device changes keep weights/prompts/dimension and
generation identity; existing vectors survive and pending work resumes on the other device.
GPU failures release the session, quarantine that identity and retry pending work on CPU.
Existing interactive holds, one-chunk foreground preemption and 30-second slices remain.

## Runtime and distribution

One ORT library is loaded per process (the existing `init_runtime` guard). The optional
DirectML build hosts both CPU query and GPU indexing sessions; the original CPU runtime
remains the default install. GPU runtime installation is explicitly consented (26 MB,
files.pythonhosted.org, MIT), pinned at 1.24.4, SHA-256 verified for the wheel and every
extracted DLL/license member, and atomic/resumable through T210. Its DLLs and notices may
also ship beside a development exe. Library resolution is pinned for the process lifetime;
installation requires a restart, preserving the persistent queue. Development overrides
retain precedence. On restart, an enabled GPU preference selects its installed runtime
ahead of a CPU library beside the exe; otherwise the original resolution order/default
CPU install is preserved. No model download/weight change is required for an existing q4 index.

The existing backend disables memory patterns/parallel execution for DirectML, with one
call per session. See [ONNX Runtime DirectML requirements](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html).
Hardware classification uses [D3D12 UMA information](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_feature_data_architecture);
probe memory uses [per-process DXGI video-memory usage](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_4/nf-dxgiadapter3-queryvideomemoryinfo).

## Consequences and validation

- Off/no-device/missing-runtime/failure paths preserve CPU search/indexing and progress.
- Large VRAM use is explicitly selected, visible and reversible from the same tray menu.
- Automatic background discovery is local and off the UI thread; only an enabled GPU mode
  runs a probe. No idle polling, queries/content in reports, cloud inference or Python runtime.
- Release native queue selected GTX adapter 0 (DXGI dedicated memory 3935 MiB; marketed
  4 GB), rejected UMA hardware, preserved the CPU prefix and generation, and drained
  26 chunks without re-embedding on switching back to CPU. GPU bulk 6.80 chunks/s;
  actual CPU QueryEmbedder p50/p95 53.68/64.70 ms with GPU queue preemption. The warmed
  compatibility probe measured CPU/GPU 3.84/9.11 chunks/s (2.37×), stable min cosine
  0.99999982, 94.43% offload. These are different synthetic corpora, under resident
  indexing load, not an idle acceptance run or ETA. Evidence: docs/benchmarks/t212/.
- Unit tests cover same-generation recovery after device loss, cache rejection,
  quarantine, speed/fidelity/placement and battery/memory gates. Synthetic native
  probes do not substitute for visible tray/keyboard/download review or a long driver soak.
- Revisit the runtime when T014's broader matrix yields a faster compatible alternative;
  this feature does not settle the pending llama.cpp/thread sweep or change default weights.
