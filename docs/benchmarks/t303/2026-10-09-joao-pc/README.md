# T303 — local image metadata and native visual meaning

2026-10-09, joao-pc: Ryzen 5 5600H (6C/12T), GTX 1650, 16 GB RAM, Windows 11 Pro
build 26300. Optimized Rust release, CPU EP, q4 backbone and q4 vision, two threads.
The user's resident T212 app PID 7404/start 18:04:57 continued indexing. No restart,
live settings/DB writes or user file content in measurement. Loaded-machine samples;
the GTX is not used for image inference in this task.

`images.json` and `images-before-backbone-sharing.json` used cached CPU ORT 1.30.
`images-interactive.json` uses the app bundle's DirectML ORT 1.24.4 library on **CPU EP**,
SHA-256 `302C69F9779D63EF4AB90316E59444C4ACBACA7FE3455020D79D10BCFCB00715`.
The same 256d normalized text space is retained:
`embeddinggemma-2@onnx-community-q4/pre1/embeddinggemma-retrieval@1/d256/l2`.

| Operation | CPU 1.30 sample | Bundled 1.24.4 CPU sample |
|---|---:|---:|
| First photo inference (includes lazy session load) | 11,355 ms | 8,597 ms |
| Second photo inference (sessions resident) | 9,320 ms | 7,761 ms |
| Two-photo persistent queue, 2 embedded / 0 failed | 18,195 ms | 15,879 ms |
| Resident snapshot after query + pipeline | 940 MiB | 941 MiB |

One sample per different photo, not p50/p95, controlled runtime comparison or fixed
library ETA. Inference includes patch preparation; separately measured bounded decoding
57/314 ms and preparation 88/68 ms in the initial shared-backbone sample. Weights load
lazily; CPU indexing reuses its text backbone rather than keeping a duplicate session.
The first implementation's retained snapshot was 1,047 MiB versus 940 MiB after sharing
(~107 MiB difference); not a peak/private/whole-app metric or a controlled latency gain.
Unload snapshot remains 273 MiB because the separate query lane is still resident.
Native decoder/ORT calls cannot be forcibly interrupted; cancellation/holds take effect
at image boundaries. One image per call and battery deferral bound background work.

The bundled-runtime sample also runs 30 uncached CPU QueryEmbedder requests on its
independent session during one already-running image invocation. All 30 started before
visual work finished: query p50/p95 **53.764/63.143 ms**, max 69.505 ms. Interleaved name
provider p50/p95 **0.836/1.247 ms** on the two-photo corpus. No runtime mutex contention
with the indexer, but no whole-app painted-result/large-catalog-under-image latency claim.
This isolates an in-flight native image call, not queue-hold timing/long GPU driver soak.

Visual retrieval (cosines from `images.json`; columns are numeric fixture names):

| Public query | 0001.jpg (cat) | 0002.jpg (beach) |
|---|---:|---:|
| a cat wrapped in a towel | 0.78044 | 0.57012 |
| a sandy beach beside the ocean | 0.57448 | 0.72753 |
| un gato envuelto en una toalla | 0.75140 | 0.54757 |
| una playa de arena junto al mar | 0.56599 | 0.70600 |

Actual persistent queue → SQLite f16 vectors → SemanticIndex → independent QueryEmbedder
→ SemanticProvider returns 0001 first for `cat type:image`, 0002 for `playa type:image`
and `ocean ext:jpg`, with no name hits. Exact `0001.jpg` remains a name hit; existing
fusion tests preserve exact navigation ahead of semantics. No filename/caption/prompt
is embedded with the photo. Two positive examples establish wiring, not broad relevance.

`queries-100k.json` reruns the existing T208 release example against final T303 metadata
projection: 100k synthetic items/chunks, 100 iterations/scenario, 10 warmups, all nonempty.
It retains its historical `task: T208` label. This measures selective hot provider queries,
not native paint/common-term worst cases or simultaneous image inference. Name unfiltered/
filtered p95 approximately 5.81/6.90 ms; exact values remain in the JSON. The image report's
sub-millisecond two-item name timings alone are not a large-catalog regression test.

## Fixtures and attribution

Downloaded only to ignored `.cache/t303/fixtures/`, not packaged/committed. Both author
file-description pages designate CC0; inspect those pages for full history/license.
Numeric local names deliberately omit visual meaning. No user's photos were read.

- 0001.jpg: [Cat in towel.jpg](https://commons.wikimedia.org/wiki/File:Cat_in_towel.jpg),
  OboeBlanket, own work, CC0. Original
  `https://upload.wikimedia.org/wikipedia/commons/8/8f/Cat_in_towel.jpg`,
  2,257,689 bytes, 3648×2736, SHA-256
  `269C18EB3E4F9E31E989C9764F06944B5ECE7C61825518D27D44DAFD40E60DD6`.
- 0002.jpg: [Mexico Beach, FL.jpg](https://commons.wikimedia.org/wiki/File:Mexico_Beach,_FL.jpg),
  BalonGreyjoy / Will Schlitzer; color correction by Succubus MacAstaroth, CC0. Original
  `https://upload.wikimedia.org/wikipedia/commons/d/d4/Mexico_Beach%2C_FL.jpg`,
  7,116,078 bytes, 3143×2095, SHA-256
  `11B809BC879621938C1590ABDA759745A95E0C17C79FB0A10698BA5845EE57A4`.

The model/vision assets are pinned to HF revision
`daa72c51243991dfcaf9f9137d2c573d8f7790c0`, Apache-2.0. Development hashes verified
against the primary HF LFS metadata; the installed component also hashes every file.
Graph 159,400 bytes, SHA-256
`7ea284226d4938f0ad921ab091f1d80a9ca699aa802984ef5cd5eec4f4761d96`;
external data 108,957,696 bytes, SHA-256
`0a9d6c927334f152a33dd90874f65d6ea5228999abe6a450d3f7813677fa704c`.
Model card is included in the optional 109,145,816-byte component. ADR-041 pins processor
sources, patch/token transport and limits. Native production requires no Python runtime.

## Reproduction

From repository root, with the pinned assets/public fixtures cached (no automatic user
corpus reads):

```powershell
cargo run --release -p lumen-bench --features ort --example images --locked -j 2 -- .cache/t303/model .cache/t303/model target/t212-release/onnxruntime.dll .cache/t303/fixtures target/t303-images.json
cargo run --release -p lumen-bench --example query_syntax --locked -j 2 -- target/t303-queries-100k.json
$env:LUMEN_EG2_MODEL_DIR="$PWD\.cache\t303\model"
$env:LUMEN_EG2_VISION_DIR=$env:LUMEN_EG2_MODEL_DIR
$env:LUMEN_ORT_DYLIB="$PWD\target\t212-release\onnxruntime.dll"
cargo test --release -p lumen-embedding-ort --test vision --locked -j 2 -- --nocapture
```

The native integration test explicitly checks 256d unit vectors, repeated-image cosine
>0.99999 and text-query cosine >0.99999 between a separate CPU session and the shared
post-image backbone. It exits without model work when those test-only variables are absent.
The benchmark creates/removes its own temporary SQLite/ANN directory and never writes
the resident app's store. The current example includes the concurrent-query sample;
the first two reports predate that measurement. Full gates: 366 Rust tests including
37 shell, two deliberate network tests ignored; 82 frontend tests; fmt, workspace/
DirectML clippy, 15-crate architecture guard and optimized desktop build.

Browser component visual check used actual ResultRow/PreviewPane: 1280×720 and 800×420,
long title, dimensions/EXIF orientation, pending/indexed/unsupported and unknown metadata.
Light theme observed; fixture/server/tab/viewport override removed. Native tray consent/
resume/removal, real-library actions, battery behavior, dark/high contrast/DPI and memory
soak remain the T303 REVIEW checklist. No raster image Quick Look, OCR, Drop or Similar.
