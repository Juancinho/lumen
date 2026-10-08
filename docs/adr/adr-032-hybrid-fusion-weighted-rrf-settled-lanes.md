# ADR-032 — Hybrid root search: weighted RRF over three lanes, content and meaning on the settled query

**Status:** Accepted (T205) — weights provisional until a harder evaluation set exists
(T211). Amends ADR-025 (merge policy). Code: `lumen_search::{fuse, RRF_K}`,
`SearchService::start_with_settle`, `lumen_catalog::ContentProvider`,
`lumen_semantic::SemanticProvider`, `lumen_storage::FtsQuery::{content, two_of}`, shell
`search.rs`. Harness: `lumen-bench eval` over `fixtures/eval/` (48 synthetic documents in
English and Spanish, 56 judged queries in six categories). Evidence:
`docs/benchmarks/t205/2026-10-08-cloud-sandbox-eval-q4.json` (2 vCPU sandbox,
EmbeddingGemma 2 q4).

**Decision**

- **Three lanes.** Names (`lumen.catalog`, every keystroke), file contents
  (`lumen.content`, FTS5 over chunks) and meaning (`lumen.semantic`: query embedding with
  the warm query lane, ADR-030, then the active ANN generation, ADR-031). One result per
  file per lane; the content and semantic lanes put the best passage in the subtitle.
- **Settled query.** The search service re-runs a typing query as settled (same id,
  `typing = false`) when nothing newer arrives within 80 ms after its run; only then do the
  content and semantic lanes answer (content FTS is too slow per keystroke, T016; embedding
  is ~30–50 ms). The UI keeps its id-stable selection while the refined list arrives.
- **Fusion: weighted reciprocal-rank fusion** over each lane's own order,
  `Σ w_lane / (60 + rank)`, then two rules: results some lane matched **exactly** (query =
  name) or by deterministic intent rank first; and one row per entity — the copy from the
  lane contributing most, with the highest confidence and a snippet from any copy.
  Weights name / content / semantic = **1 / 1 / 1** (the spec's balanced start; every
  setting of the sweep scored the same on this set, see below). A zero weight drops a lane.
- **Content lane precision.** All content words must match (English/Spanish function words
  are dropped, `FtsQuery::content`); when that finds few files and the query has ≥ 3
  content words, passages with **at least two** of them fill up (`FtsQuery::two_of`).
  A first version that fell back to *any* single word put weak one-word matches ("cold",
  "file") at the top of the content lane, and fusion then ranked them level with the
  semantic lane's right answer.
- **Semantic lane hygiene.** Nothing for queries under 3 characters, no active generation,
  or a generation of another vector space; hits more than 0.15 below the best similarity are
  dropped; the model is warmed when the overlay is shown and unloaded after 10 idle minutes.

**Evidence (sandbox, 56 queries, k = 10)**

| configuration | top-1 | MRR@10 | NDCG@10 |
|---|---:|---:|---:|
| names only | 0.179 | 0.179 | 0.147 |
| contents only | 0.536 | 0.545 | 0.526 |
| meaning only | 0.964 | 0.975 | 0.970 |
| names + contents | 0.571 | 0.580 | 0.558 |
| **fused 1/1/1** | **0.982** | **0.991** | **0.986** |
| fused, any-word content fallback (rejected) | 0.804 | 0.893 | 0.913 |

Per category (fused): exact, lexical, code, paraphrase 1.000 top-1; ambiguous 1.000 top-1
(NDCG 0.91: several relevant files); multilingual 0.917 (the one miss is a Spanish budget
that literally contains "factura de la luz"). Lane latency: names 0.5 ms p50, contents
0.2 ms, meaning 48 / 57 ms p50/p95 (embedding dominated). Mock-backend runs are in
`cargo xtask bench` (`eval-mock`) so CI keeps the harness alive.

**Consequences**

- Fusion beats every lane alone, and the lexical lanes keep exact names, error strings and
  identifiers safe when the semantic lane is wrong or missing (no model, no generation).
- The set is **saturated**: all 20 weight settings of the sweep (content 0.5–2, meaning
  0.5–3) give NDCG 0.986, and meaning alone already reaches 0.97 — 48 distinct documents
  are easy for the model. T211 adds a harder set (hundreds of documents, near-duplicates,
  real-looking folder noise, longer documents) before weights are tuned.
- Name-lane matches on function words (`de`, `the`) and folder results are left to the
  name ranking (ADR-022); the evaluation counts folders as relevant only where judged.
- Windows: the same harness runs with the T006 model (`scripts/t205/run-windows-eval.ps1`).
