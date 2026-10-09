# ADR-015 — EmbeddingGemma 2 runs on ONNX Runtime, CPU, q4 weights by default


**Status:** Accepted (T006). Evidence: `docs/benchmarks/t006/2026-10-07-joao-pc/` (Ryzen 5 5600H
6C/12T AVX2, 15 GB, GTX 1650 4 GB + Radeon Vega iGPU, Windows 11, AC power).

**Decision**

1. **Runtime:** ONNX Runtime, loaded dynamically from `onnxruntime.dll` shipped next to `lumen.exe`
   (`ort` 2.0.0-rc.13, API level 24, crate `lumen-embedding-ort`). Model: text graph of
   `onnx-community/embeddinggemma-2-ONNX`, tokenizer from its `tokenizer.json`. Rust output
   matches the Python reference exactly (fp32 min cosine 1.00000).
2. **Default device: CPU. Default weights: `model_q4` (174 MB).** Short query p50/p95
   30.0/36.9 ms, ~128-token input 133 ms, 168 MiB resident, min cosine 0.980 vs fp32 with
   identical top-1 on the fidelity corpus. Index space key
   `embeddinggemma-2@onnx-community-q4/pre1/embeddinggemma-retrieval@1/d256/l2`.
   - `fp32` stays available as the quality reference / high-quality profile (33.1 ms, 627 MiB):
     not default because of the idle memory budget (docs/PERFORMANCE.md §5).
   - `q8` rejected on AVX2 CPUs: 220.8 ms (8-bit `MatMulNBits` kernels); re-test on AVX-VNNI.
3. **No GPU by default in v1.** DirectML on the GTX 1650 was 9–17× slower than CPU for queries
   (293–572 ms p50): the graph is dispatch-bound (1,100–1,700 small nodes per inference) and the
   fp32/q4 graphs leave 48 `Gelu` nodes on CPU (copies every layer). Indexing improved only
   2–3× (8.4–9.7 vs 3.2–3.9 chunks/s) for 1.1–2.9 GB VRAM. fp16 produced zero-norm vectors
   non-deterministically (the model card warns fp16 can yield NaNs); the integrated Radeon hung
   the device (DXGI 887A0006). DirectML itself is frozen at ORT 1.24 and "legacy" in Windows ML.
   DirectML support stays compiled behind the `directml` feature for diagnostics and T014.
4. **One weight variant per index generation.** Device choice (T013) may move inference between
   devices but never changes weights without a new generation (ADR-014 space key).
5. **Prompts verified** against EmbeddingGemma 2's `config_sentence_transformers.json`
   (`SearchQuery` = `task: search result | query: `, `Document` = `title: none | text: `):
   `PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1` stays.

**Consequences**

- The 60/120 ms warm-query budget is met with margin on a 2021 mainstream laptop CPU; ADR-006
  (256d) unchanged.
- **Indexing throughput is the main risk:** ~3–4 chunks/s of ~260 tokens using all cores
  (100k chunks ≈ 8 h of full CPU). T202 must chunk smaller (~128 tokens ≈ 2× throughput),
  cap background threads, prioritize by value (ADR-011); T014 looks for faster paths.
- Packaging: `onnxruntime.dll` ≈ 18 MB (CPU build) + model 174 MB + tokenizer 32 MB.
- **Evidence gaps:** one machine. Untested: Intel CPUs, AVX-VNNI/AVX-512, NPUs, RTX 30+,
  Windows ML EPs (TensorRT-RTX, OpenVINO, VitisAI, QNN), WebGPU EP, LiteRT-LM, llama.cpp.
  `scripts/t006/run-windows-bench.ps1 -Download` lets any Windows PC contribute a run.

**Revisit when** T014 finds a backend/device that beats CPU q4 on latency or indexing at
acceptable memory; or T205 relevance evaluation shows q4 losing meaningfully against fp32
(switch weights with a new index generation).

## 2026-10-09 — T212 optional acceleration

ADR-038 adds a user-selected DirectML 1.24.4 runtime for dedicated-GPU indexing.
It hosts CPU query sessions as well; CPU q4 and the original provisioned CPU runtime
remain the default. Same weights, prompts, normalized 256d space and generations.
The bounded GTX 1650 recheck is recorded under docs/benchmarks/t014/2026-10-09-joao-pc-gpu-recheck/;
T014's broader runtime/thread verdict remains pending.
