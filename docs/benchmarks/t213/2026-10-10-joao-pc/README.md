# T213 — image scheduling, coverage and hybrid GPU

2026-10-10, joao-pc: Ryzen 5 5600H (6C/12T), GTX 1650, 16 GB RAM, Windows 11 Pro
26300. Rust optimized release, pinned q4 model/vision, two threads, bundled DirectML
ORT 1.24.4 (`onnxruntime.dll` SHA-256
`302C69F9779D63EF4AB90316E59444C4ACBACA7FE3455020D79D10BCFCB00715`).

`image-gpu.json` compares two public CC0 photos with numeric filenames. Fixture hashes,
sources and license attribution are inherited from
[T303](../../t303/2026-10-09-joao-pc/README.md). No personal source content, names or
queries are stored in this report. Same normalized 256d space; weights unchanged.

| Mode (vision always CPU) | Two image→text cycles | Visual cosine vs CPU | Post-image text cosine |
|---|---:|---:|---:|
| CPU backbone | 18,967 ms | reference | 1.0000001 |
| Prior GPU text + separate CPU image backbone | 21,929 ms | >= 0.99999994 | 1.0 |
| Shared GPU text/image backbone | 18,401 ms | >= 0.99999994 | 1.0 |

The accepted hybrid route is 1.192x mixed-cycle throughput (16.1% less elapsed time)
than the prior mixed-device route. Per-image warm timings are close/noisy: 8,884 ms
prior versus 9,023 ms hybrid for the second photo. Benefit is primarily avoiding
text-model unload/reload, not a claim of uniformly faster visual kernels. These are
loaded-machine samples, not p50/p95, causal library speed or completion ETA.

DirectML **vision encoder** was also attempted and fails native inference at
`node_view_1` (Reshape, `80070057`, invalid parameter). Production therefore keeps
the encoder on CPU. The separate isolated app probe admits the GPU backbone only
after >= 1.15x measured cycle speed and >= 0.999 cosine against CPU references.
Drivers/assets change the cache key; failure falls back to CPU without rebuilding vectors.

`synthetic-probe.json` runs the actual desktop child with the resident old indexer and
builds still active. It completes in 163.4 s, so combined text/image checks have a
240 s child bound (text-only remains 120 s). CPU/GPU text throughput 4.157/5.148 chunks/s,
offloaded fraction 0.9443; GPU query p95 461 ms reinforces keeping interactive queries
on CPU (CPU p95 40.7 ms). Hybrid image cycles 23.771→19.463 s (1.221x), minimum cosine
0.99999982. Device-memory snapshot 906 MiB is not peak/whole-app VRAM. No user files in
probe input/output. Results pass the existing opt-in text gate and separate image gate.

Reproduce with cached/licensed T303 fixtures and existing verified model/runtime:

```powershell
$env:CARGO_TARGET_DIR = 'D:\Proyectos\lumen\target\t213-build'
cargo run --release --locked -p lumen-bench --features directml --example image_gpu -- `
  D:\Proyectos\lumen\.cache\t006\embeddinggemma-2-ONNX `
  D:\Proyectos\lumen\.cache\t303\model `
  D:\Proyectos\lumen\target\t303-release\onnxruntime.dll `
  D:\Proyectos\lumen\.cache\t303\fixtures `
  D:\Proyectos\lumen\target\t213-image-gpu-mixed.json
```

The live report originally showed zero image chunks/vectors while text/PDF extraction
continued. T213 bounds preparation rounds and alternates them with vector slices;
tests prove same-page continuation/cancellation and non-starvation despite lower image
IDs and a large text backlog. The UI shows preparation/vector coverage separately:
successful vectors divided by currently discovered units cannot estimate remaining time.
Browser QA at 1280x720 observes both native progress elements, totals, long result names,
hybrid phase/device and skip/error labels without overlap. Native verification and final
gates are appended below when complete. Long driver/real-library soak remains open.
