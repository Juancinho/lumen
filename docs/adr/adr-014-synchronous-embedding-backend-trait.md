# ADR-014 — Synchronous embedding backend trait; shared correctness layer in `Embedder`


**Status:** Accepted (T005)

`EmbeddingBackend` (crate `lumen-embedding`) is a synchronous `Send + Sync` trait. Backends only
map fully formatted strings to raw native-dimension vectors (row-major `Vec<f32>`). Everything
that must be identical across runtimes lives in `Embedder`: versioned task prompts
(`PromptFormat`), batching to `max_batch`, cancellation between batches, output validation
(shape, NaN/inf, zero norm), Matryoshka truncation to the profile dimension with L2
renormalization (f64 accumulation), and the `EmbeddingSpace` key stored with index generations.

Reasons:

- inference is CPU/accelerator-bound; an async trait would add boxing/runtime coupling without
  concurrency gains, and most candidate runtimes expose blocking APIs;
- query-over-index prioritization (docs/ARCHITECTURE.md §13) needs dedicated worker lanes, which
  the query service (T204) and index queue (T202) own; the trait stays scheduling-free;
- one implementation of prompts/truncation/normalization prevents runtimes from silently
  producing incompatible spaces.

`EmbeddingSpace` = model id + weights revision (incl. quantization) + preprocessing version +
prompt id/version + dimension + normalization. It excludes backend and execution target (same
weights on CPU/NPU are one space). Prompt strings are the EmbeddingGemma v1 retrieval prompts and
MUST be verified against the EmbeddingGemma 2 model card in T006 (new `PromptFormat` version if
they differ). Image/audio/video entry points are added later as provided trait methods.
