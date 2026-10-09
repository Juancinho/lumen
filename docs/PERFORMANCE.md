# PERFORMANCE.md — latency and resource budgets

## 0. Measured status (release builds, joao-pc unless noted)

| Budget | Target p50 / p95 | Measured | Evidence |
|---|---|---|---|
| Shortcut → painted overlay | 35 / 70 ms | 22.6 / 26.0 ms; ~22 ms with every window material | ADR-020, ADR-024 |
| Keystroke → name results | 16 / 40 ms | provider p50 1.1 / p95 5.3 ms (26.5k entries + 330 apps) | ADR-021/022 |
| Keystroke → content FTS | 16 / 40 ms | **13 / 68 ms at 100k chunks (sandbox)** → runs on the settled query | ADR-017 note (T016) |
| Warm text query embedding | 60 / 120 ms | 30.0 / 36.9 ms (CPU, q4) | ADR-015 |
| ANN search incl. validation (k=10) | — (≪ embedding) | sandbox 100k: 0.66 / 1.07 ms; 2.3 / 2.8 ms with a 10k delta; recall@10 ≥ 0.998 | ADR-031 |
| …while indexing runs | 60 / 120 ms | sandbox 2 vCPU: 144 / 189 ms unprotected → **51 / 70 ms** preempted with 1-chunk batches (alone 52 / 60) | ADR-030 — Windows run pending |
| Idle memory (§5 metric) | < 400 MB | ~7 MiB WebView + 3–4 MiB shell hidden; 168 MiB with the model warm | ADR-020, ADR-015 |
| Indexing throughput (§9) | ≥ 8 chunks/s @ ≤ 50 % CPU | **~7 chunks/s @ 100 % CPU** (128-token estimate); sandbox q4: 3.4 chunks/s per busy core | ADR-015 — top risk, T014 run pending |

Semantic results after settle (ADR-032, sandbox): 80 ms settle + meaning lane 48 / 57 ms
p50/p95 (query embedding dominated) + contents 0.2 ms + fusion; Windows timing pending.
Not measured yet: keystroke → painted results end to end, arrow-key response, Quick Look
cached preview.

T302 Windows release: 128-page synthetic standard-font PDF, first OS load/page 333 ms;
uncached pages 22.68/39.17 ms p50/p95, cached raster lookup 0.035/0.053 ms. Metadata/text
arrive before rasterization. This excludes IPC/visible paint, complex PDFs and native heap
peaks; no interaction budget changes. Evidence: `benchmarks/t302/2026-10-09-joao-pc/`.

T209 Windows (ADR-036): 162 synthetic documents / 177 chunks / 49 queries; contents
0.85 / 2.32 ms p50/p95, meaning 33.52 / 65.67 ms (CPU q4, six threads). These are small
fixture timings, not 100k search evidence. A 100k-code-chunk v3→v4 upgrade took 1.10 s;
its separate metadata backfill took 1.60 s with every vector preserved. Migrations run on
a startup worker, and first show waits for schema readiness; this one-time cold-upgrade
cost does not recur on normal launches. Existing budgets are unchanged.

T303 Windows release CPU 1.30 q4 visual inference: two public CC0 photos, two threads, first
11.36 s and subsequent 9.32 s; two-image persistent queue pass 18.20 s. One sample/photo,
not p50/p95 or a fixed library ETA. Resident snapshots with a separate CPU query session
reach 940 MiB (not private/peak/whole-app memory); sharing the CPU indexing backbone
removed ~107 MiB retained in the initial sample. Vision unloads on drain/pause and waits
on battery. Native calls finish before holds/cancellation take effect at the next image
boundary; lexical results and the separate query session do not wait on that call.
Bundled ORT 1.24.4 on CPU: first/next image 8.60/7.76 s, independent uncached query
p50/p95 53.76/63.14 ms during an in-flight image call (30 samples). Loaded-machine
samples do not establish a causal runtime speed comparison or native painted latency.
Evidence, hot-name regression and runtime/query-under-image checks:
`benchmarks/t303/2026-10-09-joao-pc/README.md`. Existing budgets are unchanged; whole-app
private memory and long real-library soak are not established.

T213 Windows release: CPU vision plus shared DirectML text/image backbone took 18.401 s
for two public-photo image→text cycles versus 21.929 s with GPU text and separate CPU
image backbone (1.192x throughput). Cosine >= 0.99999994, unchanged text space. DirectML
vision encoder failed at Reshape on GTX 1650; it remains CPU. Image acceleration requires
its own isolated speed/fidelity probe. Loaded-machine samples, not a library ETA; no
interaction budget changes. See `benchmarks/t213/2026-10-10-joao-pc/README.md` and ADR-042.

## 1. Principle

Performance is not a later optimization pass. Lumen's product value depends on invoking it reflexively. If opening or searching feels slower than opening Explorer, the product loses its reason to exist.

Measure release builds on real Windows hardware. Development-mode timings are not acceptance evidence.

## 2. User-perceived latency budgets

Initial targets, subject to hardware classes:

| Interaction | Target p50 | Target p95 | Notes |
|---|---:|---:|---|
| Global shortcut → first painted overlay | <35 ms | <70 ms | resident/warm process |
| Keystroke → lexical result update | <16 ms | <40 ms | no semantic wait |
| Warm text query embedding | <60 ms | <120 ms | broad CPU target; accelerator should be faster |
| Keystroke stable → semantic results shown | <100 ms | <170 ms | warm model, typical corpus |
| Arrow-key selection response | <8 ms | <16 ms | one frame |
| Dismiss overlay | <16 ms | <35 ms | visual disappearance |
| Quick Look cached preview | <50 ms | <120 ms | file-type dependent |

Google's published Windows benchmark for EmbeddingGemma 2 on a Dell XPS 16 (Intel Core Ultra Series 3) reports approximately 71.7 ms text latency on CPU, 19.2 ms on GPU and 13.3 ms on Intel OpenVINO NPU. These are reference points, not promises for every PC.

## 3. Perceived-speed strategy

### 3.1 Keep shell resident

Tray mode keeps the process and minimal search state alive. The overlay itself is hidden, not reconstructed from zero on every invocation.

### 3.2 Keep the text query path warm

When enabled and memory budget allows, retain the text embedding backend/model in warm state.
Built (T204, ADR-030): `lumen_semantic::QueryEmbedder` — its own runtime session and thread,
`warm()` on overlay show, `unload()` on idle/memory pressure, 64-entry in-memory cache. Vision/audio encoders are not needed for normal text queries and may be loaded lazily.

### 3.3 Progressive search

Never gate first paint or first results on semantic inference.

Timeline target:

```text
0 ms      input changes
0–20 ms   filename/FTS results paint
50–90 ms  semantic request triggered
~? ms     query embedding finishes
+ few ms  ANN + fusion
<170 ms   refined list paints on broad target hardware
```

### 3.4 Cancellation

If query changes from `trans` → `transformer`, stale embedding/search work must be cancelable or ignorable. Do not enqueue every keystroke indefinitely.

Built (ADR-030): a query that has not started is superseded by the newer one; the one inside
the runtime finishes but its caller can stop waiting. While queries arrive (+1.5 s linger)
indexing is held at its next batch boundary, and for 10 s after the overlay is shown
indexing uses one-chunk batches so that boundary is at most one chunk away.

## 4. UI performance rules

- no synchronous filesystem calls from React/UI thread;
- virtualize long result lists;
- render only visible thumbnails;
- decode large images off the UI thread;
- avoid layout thrash during result updates;
- memoize stable row subtrees where profiling justifies it;
- use CSS transforms/opacity for animation where possible;
- batch semantic reorder updates;
- no spinner for operations expected below ~100ms;
- test at 60 Hz and higher refresh rates.

## 5. Memory budgets

Initial goals, not hard guarantees across runtimes:

### The memory metric

One metric for every memory budget: **private working set of `lumen.exe` plus its WebView2
process tree** (what `scripts/t012/run-windows-webview.ps1` reports, and the sum Task
Manager shows for Lumen). Commit charge is reported next to it as the reserve indicator,
never as the budget figure; VRAM is reported separately for accelerated inference (ADR-019).

### Idle/tray, text search ready

Target roughly **<400 MB private working set** on a typical machine after warm stabilization, with a lower stretch goal. The exact backend determines feasibility.

### Active text+image indexing on CPU/NPU

Prefer staying **well below 1 GB** application-private memory if the selected runtime permits it.

Avoid default GPU inference if it consumes disproportionate VRAM merely to save tens of milliseconds. Google publishes ~822 MB GPU memory for text and ~1.77 GB for text+vision on its cited Windows benchmark, versus much lower CPU/NPU memory figures. Default backend selection should consider system pressure, not latency alone.

### Encoders

Unload vision/audio after an idle timeout if runtime reload cost is acceptable and no corresponding indexing jobs remain.

## 6. Vector storage budget

Raw vector payload at 256d:

- f32: 1024 bytes/vector;
- f16: 512 bytes/vector;
- i8: 256 bytes/vector.

Actual ANN storage is larger because HNSW graph links and metadata add overhead.

Use f16 as the first compact candidate, validated against retrieval metrics.

Example order of magnitude for 1,000,000 chunks:

- raw f16 vectors: ~512 MB;
- plus HNSW graph/labels: implementation-dependent;
- plus SQLite metadata/FTS/snippets;
- plus bounded thumbnails.

A practical total of roughly sub-GB to a few GB is plausible depending on corpus and caches; measure instead of advertising a fixed number.

## 7. ANN budgets

Measured so far: ADR-016 (index alone, 100k/1M) and ADR-031 (the persistent generation on
SQLite: build 4k vectors/s single-threaded, mmap open 7 ms, file + delta + validation
≤ 3 ms at 100k on the sandbox). `lumen-bench ann-gen` is in `cargo xtask bench`.

Benchmark at:

- 10k;
- 100k;
- 1M;
- optional 5M vectors.

For each:

- index build throughput;
- search p50/p95;
- recall@10 against exact search;
- RSS/mapped bytes;
- on-disk size;
- load/mmap time;
- delete/update behavior.

Tune HNSW `M`, construction expansion and search expansion from data. Do not copy benchmark defaults blindly.

## 8. SQLite budgets

- WAL enabled;
- short read transactions;
- prepared statements;
- indexes for path/name/time/kind filters;
- FTS queries capped to candidate counts needed by fusion;
- background writes batched;
- UI/search reads must not wait behind long index transactions.

Run `EXPLAIN QUERY PLAN` for hot queries.

## 9. Indexing throughput

Initial bulk indexing is allowed to take time; it is not allowed to make the computer unpleasant to use.

### Budget (proposed in T015; confirm with T014/T202 evidence)

Throughput is stated as **chunks/s at a CPU share**, for ~128-token text chunks on the
reference machine (Ryzen 5 5600H, 6C/12T) in the Balanced profile:

- **≥ 8 chunks/s while using ≤ 50 % of logical cores** for embedding, with the user active;
- Turbo (user-chosen) may use all cores and an approved accelerator (ADR-019);
- on battery or under user activity the scheduler may drop below the budget, never above
  the CPU share.

Measured today: ~3–4 chunks/s of ~260 tokens at ~100 % CPU (ADR-015) ≈ 7 chunks/s of 128
tokens at 100 % — **the budget is not met**; T014 (runtimes) and T202 (chunk size,
thread caps, value-priority) must close the gap or revise it with evidence.

T014 instrumentation (2026-10-08): `lumen-bench embed` reports the CPU time of the process
doing the work per batch size (`throughput[].cpu`: busy cores and share of the machine;
`--cpu-pid` for an external `llama-server`). Cloud sandbox, ORT q4, 100-word (~128-token)
chunks: **3.3–3.4 chunks/s per busy core, linear from 1 to 2 threads, and batching does not
help on CPU** (b1 ≈ b8 ≈ b16). Throughput is compute-bound (~125 M non-embedding
parameters per token), so the budget is a question of how many cores scale on a given CPU
— joao-pc's earlier ~7 chunks/s on 12 threads suggests poor scaling past the physical
cores. `scripts/t014/run-windows-throughput.ps1` sweeps 1/2/4/cores/threads and llama.cpp
builds to draw the chunks/s-at-CPU-% curve; evidence in `docs/benchmarks/t014/`.

T202 (ADR-029): the queue adds 0.026 ms per chunk (≈10⁻⁴ of embedding time). A duty cycle
holds a CPU share exactly (2 threads at duty 0.5 → 50 %) but yields ~20 % fewer chunks per
CPU than lowering the thread count (1 thread → 50 %, 2.8 vs 2.3 chunks/s): cap threads
first, use the duty cycle for the remainder.

T212/ADR-038 (Windows release, 2026-10-09): optional GTX 1650 indexing with CPU queries.
Warmed same-runtime synthetic probe: CPU 3.84 / GPU 9.11 chunks/s (2.37×), stable cosine
0.99999982, 94.43% offload. Real persistent queue bulk: 6.80 chunks/s; CPU query-lane
p50/p95 53.68/64.70 ms with indexing preemption. The resident CPU indexer was also active;
this small loaded-machine run does not settle the ≥8 chunks/s / ≤50% CPU budget or T014's
broader runtime/thread verdict. Per-process GPU usage 906 MiB sampled after inference is
not a whole-run peak. Evidence/reproduction: `docs/benchmarks/t212/2026-10-09-joao-pc/`.

Track:

- files/s discovered;
- MB/s extracted;
- chunks/s embedded by modality;
- queue depth;
- failed/retried jobs;
- CPU time;
- power state;
- temperature/throttle only if an OS-safe signal is available.

User-facing ETA should be omitted unless it is reasonably stable.

## 10. Power policy

Balanced default:

- reduce background concurrency on battery;
- pause expensive media indexing on battery unless user opts in;
- keep interactive query path available;
- consider Windows EcoQoS / below-normal priority for background workers after a dedicated spike.

## 11. Startup and cold start

Measure separately:

- process cold start;
- overlay warm start;
- model cold load;
- vector index map/load;
- SQLite open/migrations.

The normal user path after login should be warm. Launch-at-login can initialize low-priority resources without showing a window.

## 12. Performance regression suite

Every release candidate should output machine-readable metrics for:

- overlay show time;
- 100 representative lexical queries;
- 100 semantic queries;
- mixed hybrid queries;
- 100k/1M ANN synthetic benchmark;
- initial indexing sample;
- incremental update sample;
- memory after warm idle;
- memory during image indexing.

Store baselines in the repo or CI artifacts; fail only on meaningful, stable regressions to avoid flaky performance CI.

## 13. Profiling tools

Use appropriate tools rather than guessing:

- Rust tracing/flamegraph/profilers;
- Windows Performance Recorder/Analyzer where needed;
- browser/React profiler for UI;
- SQLite query plans;
- backend-specific inference profiling.

Optimization PRs should say what measurement improved.
## 14. WebView discipline

Tauri/React is acceptable only if the shell remains lightweight.

Rules:

- target exactly one WebView;
- no background animation while hidden;
- no unnecessary JS timers/polling while hidden;
- no filesystem/model/indexing work in React;
- virtualize long results;
- lazy preview/thumbnail loading;
- measure WebView memory separately from total process memory;
- evaluate hidden-state suspension/low-memory APIs where integration permits;
- settings/onboarding should reuse the shell unless there is measured reason not to.

## 15. Memory profiles

Treat memory as a product profile, not an accident.

Possible policy:

- Performance: keep text embedding path warm longer;
- Balanced: unload after idle threshold if reload cost is acceptable;
- Low memory: lazy-load model and aggressively release modality-specific encoders.

Vision/audio should not remain resident solely because the product supports those modalities.

## 16. Initial indexing UX budget

For large corpora, optimize time-to-use rather than only time-to-complete.

Engineering targets:

- inventory/search-by-name available within minutes on typical SSD corpus;
- high-priority semantic content starts appearing continuously;
- index job state persisted frequently enough to make restart cheap;
- background scheduler reduces pressure when user is active/on battery;
- no unbounded extraction/embedding queues.

For ~1 TB of real documents/code/images/media, use a planning envelope of ~20–60 hours CPU-effective Smart indexing on mainstream i5-class hardware, highly corpus/runtime dependent. High-quality/exhaustive media may take several days. Do not display these as fixed ETAs.

## 17. Shell comparison trigger

Do not migrate from Tauri because "native must be faster". Trigger `TX01` only if M1 measurements miss important startup/RAM/jank targets or if strategic evidence warrants it.

The comparison must include:

- cold start;
- warm hotkey → first frame;
- hidden/active RAM;
- idle CPU;
- typing/update p95;
- packaging size;
- visual implementation cost;
- accessibility/maintenance implications.
