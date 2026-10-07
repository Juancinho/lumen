# PERFORMANCE.md — latency and resource budgets

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

When enabled and memory budget allows, retain the text embedding backend/model in warm state. Vision/audio encoders are not needed for normal text queries and may be loaded lazily.

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
# Refinement — shell and full-index budgets

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

