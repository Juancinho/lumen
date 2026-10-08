# ADR-030 — Query lane: its own runtime session, latest wins, preempts indexing at single-chunk boundaries

**Status:** Proposed (T204) — accepted after the Windows run of
`scripts/t204/run-windows-query-lane.ps1`. Code: `crates/lumen-semantic` (`QueryEmbedder`),
`lumen_content::Control::{hold, mark_interactive, interactive_within}`, shell
`indexing::on_overlay_shown`. Benchmark: `lumen-bench query-lane`. Evidence:
`docs/benchmarks/t204/2026-10-08-cloud-sandbox-query-lane-*.json` (2 vCPU sandbox, ORT q4).

**Context.** The settled query must be embedded within the semantic budget
(PERFORMANCE.md §3: warm query p50 ≤ 40 ms / p95 ≤ 80 ms on the reference machine) while
background indexing (ADR-029) keeps the CPU busy. A query that waits behind an indexing
batch, or that shares a runtime session with it, pays the batch's latency.

**Decision**

- **Own session, own thread.** `QueryEmbedder` owns a query-lane `Embedder` (a second ORT
  session, separate from indexing) on one worker thread; callers block in `embed` from the
  settled-query lane. The model is created on first use or by `warm()` (overlay shown), and
  can be unloaded after an idle period or on memory pressure (`unload`).
- **Latest wins.** A request that has not started is superseded by a newer one
  (`QueryError::Superseded`); a request already inside the runtime finishes (one short input
  cannot be interrupted) but its caller can stop waiting (cancellation token). Typing never
  queues work per keystroke.
- **Cache.** The last 64 query vectors are reused, in memory only; `clear_cache` forgets
  them (privacy: clearing history).
- **Preemption.** While queries arrive, and for a 1.5 s linger after the last one, the query
  lane holds the indexing queue (`Control::hold`): indexing stops at its next batch
  boundary.
- **Single-chunk batches near interactive use.** For 10 s after the overlay is shown or a
  hold is taken/released, the queue embeds one chunk per call instead of 8, so a query
  waits for at most one chunk. Outside that window batches of 8 keep indexing throughput.

**Evidence (sandbox, 2 vCPU, q4, 80 ms between queries)**

| setting | queries alone p50 / p95 | with indexing p50 / p95 | preempted p50 / p95 | indexing while querying |
|---|---:|---:|---:|---:|
| query 1 thread, index 1 thread (no CPU contention) | 88 / 119 ms | 91 / 109 ms | 87 / 100 ms | 4.6 → 1.2 chunks/s |
| query 2, index 2, batch 8 | 50 / 62 ms | 144 / 189 ms | 54 / **184** ms | 3.5 → 1.2 chunks/s |
| query 2, index 2, batch 1 | 52 / 60 ms | 134 / 193 ms | 51 / **70** ms | 3.0 → 0.2 chunks/s |

Contention for cores nearly triples query latency; holding indexing restores the median but
the p95 stays at one 8-chunk batch (~180 ms) until batches near interactive use shrink to
one chunk (p95 70 ms, within ~10 ms of queries alone).

**Consequences**

- Two sessions cost a second copy of the model and runtime working memory (per-session
  RSS in the T006 reports); the query session is the one unloaded first under memory
  pressure.
- Indexing nearly stops while the user types; it resumes 1.5 s after the last query.
- Not yet wired into the shell's search: the settled-query lane (T205) creates the
  `QueryEmbedder` and calls `warm()` on overlay show; until then only the single-chunk
  window is active in the app.
- To revisit on Windows: thread counts for both sessions on 6–16-core machines, and
  whether the preempted p95 meets the 80 ms budget.
