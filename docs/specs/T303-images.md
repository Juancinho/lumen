# T303 — image metadata and local visual retrieval

## 0. Implementation status (2026-10-09)

REVIEW by codex. Implemented from T302 commit 60befce; native user reviews are deferred.
ADR-041 and Windows release evidence are in `../benchmarks/t303/2026-10-09-joao-pc/`.

## Contract

- Ordinary root text, optionally `type:image`/`ext:jpg`, retrieves visual meaning after
  names. Existing file identity, Enter/Open, Ctrl+Enter/Reveal and Ctrl+K/Copy path remain.
  Image rows and Quick Look expose format/dimensions/EXIF orientation, never generated
  captions or inferred capture applications. OCR is T304; image queries/Similar T305/T306.
- Only content-enabled locations are automatically read. Offline/placeholders and
  names-only locations remain inventory-only. Local PNG/JPEG/WebP/BMP are supported;
  unsupported formats, malformed/oversized images have explicit coverage codes.
- One empty `image` chunk represents one image; it has no fabricated FTS text. Its vector
  uses the existing 256d q4 generation/ANN/fusion. Metadata/digest change invalidates it;
  unchanged moves and resume retain vectors. Schema migration preserves existing data.
- Source at most 16 MiB, 32 million decoded pixels, 16,384 px per side, decoder allocation
  limit 192 MiB. Cancellation checked around bounded synchronous decode/preprocessing;
  codec calls have no hard execution-time sandbox. One image in memory/inference at once.
- Default Gemma image processor: EXIF-oriented RGB, aspect-preserving bicubic resize,
  16 px patches, pooling 3, 280 soft-token budget, padded 2,520 patches with XY positions.
  No text prompt or filename enters image embedding. Validate outputs through Embedder.
- Optional pinned q4 vision files from the existing HF revision: 109 MB, Apache-2.0.
  Download requires explicit native consent, hash verification and atomic/resumable install.
  Missing assets leave image vectors pending, not failed; text search/indexing continues.
- Vision inference initially uses CPU with bounded threads and existing pause/power/hold
  controls. The text queue retains measured optional GPU acceleration; a text-only GPU
  compatibility probe does not establish visual-GPU compatibility. CPU queries unchanged.
  Encoders load lazily, share a CPU indexing backbone and unload when drained/paused;
  visual work waits on battery. No new hidden timer/process/WebView. Native calls are
  cooperative: holds/cancellation act at the next image boundary, not mid-inference.

## Validation

Codec/dimension/orientation/bounds, patch ordering/padding/aspect math, same-space normalized
vectors, pending/resume/moves/edit/deletion/consent, name/meaning filters and UI wire/privacy
tests. Actual Windows release visual inference/retrieval and memory/latency evidence with
synthetic or public licensed fixtures, alongside the untouched resident application.
Full gates, task/state/handoff/worklog and coherent T303 commit. Real-library/native tray
and keyboard review remains REVIEW until observable.

Actual two-photo q4 inference preserves the existing text space and queries after a
visual call; descriptions in English/Spanish rank the intended numeric-named photo.
CPU inference takes roughly 8–11 s/image here; loaded-machine samples are not a throughput
guarantee. Coverage exposes absent metadata without fabricated dimensions and separates
indexed/pending/skipped/failed files. Browser component QA covers long names and narrow
layouts; native DPI/material/consent/real-library checks remain in HANDOFF.
