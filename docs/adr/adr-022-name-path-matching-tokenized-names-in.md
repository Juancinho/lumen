# ADR-022 — Name/path matching: tokenized names in FTS5, Rust scoring, bounded stages


**Status:** Accepted (T102) — Linux relevance gate plus the Windows run of
`scripts/t101/run-windows-catalog.ps1` (keystroke p95 5.3 ms over 26.5k entries + 330 apps).
Evidence: `crates/lumen-catalog/tests/relevance.rs` over `fixtures/search/catalog-relevance.json`,
`docs/benchmarks/t102/2026-10-08-cloud-sandbox/catalog-usr-home.json`.

**Decision**

- **Tokens:** names split code-aware (`MyComponentName` → my component name, `HTTPServer` →
  http server, `q3budget2025` → q 3 budget 2025), folded (case, accents). `items.name_parts` =
  tokens + original words + initials of the stem (`vsc`, `upc`); `items.path_parts` = tokens of
  the 3 nearest parent folders. Both indexed by `names_fts` (FTS5 external content, prefix
  indexes 1–3; update trigger only fires when the token text changed).
- **Candidates (bounded):** (1) exact + prefix on `name_key` (index range, always);
  (2) FTS5 token-prefix with terms of 3+ characters, single term restricted to `name_parts`,
  best bm25 first, 300 rows, own 8 ms slice; (3) typo candidates with the same first two
  characters, 1,000 rows, own 4 ms slice, only while results are scarce. Stages 2–3 degrade to
  stage-1 results when their slice runs out.
- **Scoring (`lumen_catalog::rank`):** exact 1.0 > exact stem 0.97 > name prefix 0.80–0.92 >
  all tokens prefix name tokens in order 0.78–0.86 / any order 0.70–0.78 (initials included) >
  name + folder tokens 0.50–0.60 > typo (OSA distance ≤1 at 4–7 chars, ≤2 at 8+) 0.29–0.37.
  Priors: app +0.06, modified <3 days +0.04 / <30 days +0.02, hidden/system −0.15, depth
  beyond 4 levels −0.004/level (max −0.04). Ties: shorter name, then id.

**Evidence**

- Relevance set (12 apps, 30 files, 40 queries incl. typos, initials, code names, Spanish,
  folder+name, app-vs-file collisions): MRR@10 1.000, R@1 1.000 (CI test asserts MRR ≥ 0.95).
- 2 vCPU sandbox, 246,912 entries (/usr + home, worst case: 100k `lib*` names): keystroke
  p50 0.76 ms, p95 7.9 ms, max 10.4 ms (p95 by prefix length 1–2: 0.3 ms; 3–8: ~8 ms — the
  FTS slice). First sync 19.2 s (12.9k entries/s; FTS maintenance ~3× the T101 cost), resync
  10.2 s, 173 MiB database.

**Consequences**

- The relevance fixture is the regression gate for any ranking change (add cases, never
  lower the threshold silently). Usage/recency signals (T106) plug in as priors.
- Inventory throughput halved by the token index; acceptable for Pass 0 (1M entries ≈ 80 s in
  the background). Revisit if T207 incremental updates show it matters.
- 2026-10-09 T208 amendment: filters (`ext:`, `in:`, `type:`) are implemented without replacing
  these matching stages; filtered candidates obey predicates before LIMIT. Extension
  tokens still match plain queries. See `../specs/T208-query-syntax.md`.
