# T212 — optional dedicated-GPU indexing

## 0. Implementation status

REVIEW, implemented by codex 2026-10-09 after the explicit GTX 1650 request.
Native tray/persistence/download and longer driver-pressure checks remain in HANDOFF.
Dependencies T013/T202/T204/T210 preserved; ADR-038 owns the explicit resource exception.
Windows release real queue: 26 synthetic chunks, CPU prefix preserved, GPU bulk/tail
completed, same generation/space and zero re-embedding on switching back to CPU.
GPU bulk 6.80 chunks/s; CPU query p95 64.70 ms. Evidence: docs/benchmarks/t212/.

## Scope and contract

- Native tray → Content indexing → dedicated GPU acceleration; persisted, off by default.
- Queries always use CPU. Same model/variant/prompts/dimension/normalization/generation;
  device changes preserve completed vectors and continue the persistent queue.
- Hardware adapters only, exclude UMA/integrated and software adapters using DXGI/D3D12.
- Probe with synthetic inputs in a short-lived child of the same executable, before
  loading GPU inference in the resident process; timeout/failure means CPU fallback.
  Warm a document batch before comparing throughput; exclude first-use shape setup.
- Require stable, finite, same-space vectors, cosine ≥0.999 and ≥90% graph offload;
  measured indexing speedup ≥1.5. Cache tied to model/runtime/adapter/driver identity.
- User-selected GPU mode may use available dedicated VRAM (user explicitly waived the
  Balanced cap); remain on CPU on battery or memory pressure, preserve interactive
  preemption, batch bounds and the existing single database writer.
- GPU device failure quarantines that runtime identity and retries pending work on CPU;
  do not mark successful older vectors stale or create a generation for a device switch.
- DirectML is optional and local; CPU continues on machines without it. One ORT library
  per process: the DirectML build hosts both the CPU query and GPU indexing sessions.
  Optional runtime install requires restart; the setting selects it at next launch,
  while development overrides retain priority. Already bundled DirectML supports toggling.
- No changes to weights/ranking/chunking/GUI architecture; no Python runtime, telemetry
  or content in probe reports. New runtime downloads retain explicit size/host/license
  consent and hash verification if exposed through provisioning.

## Product path and budgets

Root search remains universal, existing file/code actions and keys unchanged. Tray is
keyboard-accessible through the Windows notification area. Works offline with locally
installed model/runtime. Discovery/probes/inference/persistence run on workers; no
hidden polling. Reuse 30-second queue slices and single-chunk interactive preemption.
Measure release Windows CPU query latency, real GPU queue progress and preservation of
vectors/generation; check off/on/failure/no-device states and persistent preference.
