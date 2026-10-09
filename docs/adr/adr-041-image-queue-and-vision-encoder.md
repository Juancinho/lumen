# ADR-041 — image units in the persistent queue and optional q4 vision encoder

**Status:** Accepted 2026-10-09 (T303 Windows release evidence); native product review pending.

## Evidence and decision

The pinned onnx-community EmbeddingGemma 2 revision
`daa72c51243991dfcaf9f9137d2c573d8f7790c0` exports `vision_encoder_q4.onnx` (159,400 bytes)
and weights (108,957,696 bytes). Its model card and processor describe 280 soft tokens,
16 px patches, pooling 3, RGB rescale 1/255 and aspect-preserving bicubic resize. The text
graph already consumes `image_features` [tokens,512], currently empty in Lumen.
The visual output feeds that same q4 graph, with image token blocks and no document
prompt. Queries keep the existing CPU text path and compatible normalized 256d space.

Sources: [pinned model card](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/blob/daa72c51243991dfcaf9f9137d2c573d8f7790c0/README.md),
[processor](https://github.com/huggingface/transformers.js/blob/511bb61a7c46b73e33f1af3fb308fbd1afe98c02/packages/transformers/src/models/gemma4/image_processing_gemma4.js),
[multimodal transport](https://github.com/huggingface/transformers.js/blob/511bb61a7c46b73e33f1af3fb308fbd1afe98c02/packages/transformers/src/models/embedding_gemma2/modeling_embedding_gemma2.js).
Development fetched metadata through the HF API because web rendering was unavailable;
asset hashes match its LFS metadata. No user content was used in verification.

Use a shell-independent Rust image adapter for bounded decode, orientation and preprocessing.
Append schema v5 image metadata without replacing text/code/PDF chunks or vectors. Images
produce one empty `image` chunk, with no caption/name embedding or OCR FTS. Extend the
existing persistent queue to dispatch modalities; absent vision capability defers image
jobs while text drains. Same writer, preemption, pause and generation rules remain.
Store a bounded source digest for ambiguous rename verification and changed-source checks.
Dimensions/format/orientation are typed domain context on the same file identity.

The optional vision component uses T210's consent/verification/install/remove boundary.
It is separate from the current text install and does not silently download/replace it.
Vision sessions run on CPU initially: T212's measured probe is text-only, and granting its
GPU choice to a different graph would violate ADR-019. Text GPU indexing remains available.
Report image CPU work accurately and benchmark before future visual GPU approval.

Windows optimized q4 proof: public CC0 cat/beach photos with numeric filenames, no name
or caption embedding. Correct top photo for English/Spanish descriptions; independent
CPU query lane and existing type/ext filters retrieve the same file targets. Source
digest/EXIF/codecs, missing-capability deferral, same-space vectors/text fidelity, queue
resume/moves/edit/deletion and v4→v5 vector preservation have automated tests. Native
vision integration passes with the bundled DirectML 1.24.4 library using CPU EP; repeated
image and post-image text-query cosine >0.99999. No visual GPU inference is tested/approved.

Initial CPU 1.30 sample: 11.36 s first/9.32 s next image, 18.20 s two-image queue pass,
resident snapshot 940 MiB with a separate CPU query session. CPU indexing now reuses
its text backbone: ~107 MiB less retained than the initial duplicated-backbone sample.
This is a snapshot comparison on a loaded machine, not peak/private memory or causal
latency evidence. Bundled-runtime CPU sample: 8.60/7.76 s, concurrent independent query
p50/p95 53.76/63.14 ms, 30/30 calls starting while image inference was still running.
Evidence/licensed fixture attribution/reproduction:
[T303 benchmark README](../benchmarks/t303/2026-10-09-joao-pc/README.md).

## Consequences and limits

PNG/JPEG/WebP/BMP first; other formats remain named inventory with visible skip codes.
16 MiB compressed, 32M pixels, 16,384 px/side and 192 MiB decoder allocation admission.
One image per inference; no unbounded decoded batch. Cancellation/deadlines and queue
holds do not hard-interrupt codecs/native inference, acting at image boundaries. The
query lane has its own CPU session and lexical results never await that native call.
Visual work waits on battery. Encoders are lazy/unloaded, no process split without
TX02 evidence. Local content consent and placeholder checks precede reads. Persist no
GPS/camera history, thumbnails, captions, queries or source paths in benchmark evidence.
OCR/Drop/Similar and image raster preview remain separate tasks. Production requires no
Python runtime. Pinned image 0.25.10 uses only JPEG/PNG/WebP/BMP features; eight new locked
codec packages have scoped offline-generated license notices, shipped with the bundle.

Preserve text EmbeddingSpace and chunker version: adding the model's native image pathway
does not alter text vectors. Pin the vision/preprocessing contract on image metadata; any
future incompatible change must invalidate only image units with explicit migration and
evidence, or build a new generation if shared text weights change.
