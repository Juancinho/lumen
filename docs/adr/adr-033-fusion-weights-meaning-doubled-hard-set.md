# ADR-033 — Fusion weights from the harder set: meaning ×2; findings for the lexical lanes

**Status:** Accepted (T211). Amends ADR-032 (weights). Generator:
`scripts/eval/make_hard_set.py` → `fixtures/eval-hard/` (162 documents, 49 queries, graded
`relevant` / `related`). Evidence:
`docs/benchmarks/t211/2026-10-08-cloud-sandbox-eval-hard-q4.json` (2 vCPU sandbox,
EmbeddingGemma 2 q4, 48 weight settings).

**Context.** ADR-032's set was saturated: every weight setting scored the same. The harder
set adds what makes retrieval hard: twelve monthly bills per utility (only the month
differs), meeting notes per client × topic, draft / v2 / final versions of one report,
long handbooks where one section answers, the same function in four programming languages,
and logs / CSV exports / configs / changelogs that share the queries' vocabulary — in
English and Spanish, crossing languages.

**Decision**

- App weights name / content / meaning = **1 / 1 / 2** (`search.rs`, the harness default).
- Keep all three lanes: meaning alone scores slightly higher on this synthetic set, but the
  lexical lanes are what answer when there is no model or generation yet (T210) and they
  carry exact tokens (error lines, identifiers) better.

**Evidence (49 queries, graded NDCG@10)**

| configuration | top-1 | MRR@10 | NDCG@10 | lexical NDCG | code NDCG |
|---|---:|---:|---:|---:|---:|
| names only | 0.102 | 0.112 | 0.114 | 0.000 | 0.000 |
| contents only | 0.531 | 0.555 | 0.551 | 0.911 | 0.617 |
| meaning only | 0.959 | 0.980 | 0.964 | 0.823 | 0.971 |
| fused 1/1/1 (ADR-032) | 0.918 | 0.952 | 0.954 | 0.925 | 0.868 |
| **fused 1/1/2** | **0.939** | **0.962** | **0.957** | 0.870 | 0.920 |
| best of sweep (0.5/0.5/3) | 0.939 | 0.969 | 0.959 | 0.823 | 0.960 |

The name weight changes nothing between 0.5 and 2 (names rarely match these queries; the
exact-match rule already decides name queries). Doubling meaning recovers code and series
queries; more than that only trades lexical quality for code.

**Findings for later work**

- **Content lane misses the language of code**: "exponential backoff python" ranks the
  TypeScript file first, because "python" is only in the path. Indexing name/path tokens
  with each chunk (or a name boost for content hits) is the fix to measure (T209).
- **`two_of` still admits noise from logs** ("retry http request in go" → log lines with
  "retry" and "request"). Candidate: skip `two_of` for files of kind log/data, or demote
  them; measure first.
- Cross-language month names ("factura de la luz de marzo" → the March bill) are found by
  meaning in 3 of 4 cases; there is no lexical bridge for them.
- Semantic lane latency 47 / 56 ms p50/p95 on the sandbox; contents 0.3 ms; names 0.5 ms.
