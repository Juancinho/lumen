# ADR-042 — interleaved indexing, cached coverage and hybrid image GPU

**Status:** Accepted 2026-10-10 (T213); real-library and driver soak review remains.

## Evidence

A live names/content-consented library had more than 1,600 Pictures files but zero
image chunks/vectors. The writer exhausted text/PDF extraction before preparing images
and running the embedding queue. Long native PDF calls also kept tray counts stale.
This was scheduling, not an absent vision model or GPU preference.

On the GTX 1650, the pinned q4 vision encoder fails DirectML at `node_view_1` (Reshape,
invalid parameter). Keeping that encoder on CPU and using DirectML only for the shared
q4 backbone succeeds. Two public CC0 photos retain CPU-reference vector cosine
>= 0.99999994 and post-image text fidelity >= 0.99999994. The measured mixed cycle
improves from 21.929 s to 18.401 s: 1.192x throughput, 16.1% less elapsed time.
Single-image timings are close; this is not a whole-library ETA or GPU encoder claim.
See [release evidence](../benchmarks/t213/2026-10-10-joao-pc/README.md).

## Decision

Amend ADR-029/038/041 without changing the schema, weights, embedding space or query
lane. Keep the existing writer. Each content round prepares at most eight images
(250 ms cooperative target), then reads at most eight text/PDF files (750 ms target).
Retain keyset cursors between rounds and alternate extraction with embedding slices.
Native I/O/parser/inference calls can overrun these targets; cancellation and holds take
effect at the next file/image boundary. Restart recovers from persistent file/chunk state.

Use independent text/image queue cursors. Text runs first, but an available image gets
a turn after eight text batches so a large text backlog cannot starve it. Missing vision
capability still leaves images pending while text drains. Same-generation commits,
source revalidation, interactive holds, battery deferral and failure recovery remain.

Extend the existing isolated, cached compatibility probe with synthetic image cycles.
An image route requires finite timings, >= 1.15x cycle speed and cosine >= 0.999. Cache
identity includes the vision assets and probe method; install/remove rechecks compatibility.
Warm each visual shape on all compared routes before timing, as the text probe already
does for document shapes. First-use compilation must not decide repeated library work.
The probe lifetime remains 120 s for text alone; with twelve visual calls it is bounded
at 240 s. A loaded-machine combined probe exceeded 120 s, so the prior text-only
deadline would incorrectly reject both routes during image validation. The overlay
remains responsive and shows compatibility checking while background inference is held.
Production keeps the vision encoder on CPU. A validated DirectML backbone shares the text indexing
session, avoiding unload/reload on each modality switch. Failed/slow/unvalidated routes
use the established CPU path. Queries remain on their independent CPU session.

Expose a cached, path-free typed snapshot to the existing overlay: phase, active device,
files read/total, vectors ready/total, image ready/queued counts, skips and failures.
Files read includes terminal skipped/failed files; vectors ready counts successful vectors.
These are distinct coverage percentages, not elapsed-time estimates. Totals can grow as
inventory/extraction discovers work; unknown/zero denominators show no invented 100%.
Metadata counts run on the writer, never in a UI command or under the shared status lock.

The root surface gains a quiet 80 px footer using existing colors/typography and native
accessible progress elements. Publish worker-driven updates only while visible, with
phase changes immediate and batch updates throttled to one second. Showing the overlay
fetches the latest cached snapshot. No hidden polling, animation or extra WebView.

## Limits

No OCR, captions, pixel retention, new cloud access, new runtime download, performance
budget change or whole-library speed promise. Long native calls are still cooperative.
GPU opt-in and runtime licensing from ADR-038 remain. The direct vision GPU failure is
specific to the tested graph/runtime/device; a future encoder route needs new evidence.
