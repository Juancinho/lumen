# ADR-023 — Usage signals: aggregates only, decayed frecency, learned query choices, pins


**Status:** Accepted (T106). Code: `lumen_storage::usage`, `lumen_catalog::rank::usage_prior`.

**Decision**

- **No raw event log** (docs/PRIVACY_SECURITY.md §2, least retention): the `usage_events`
  table is replaced (pre-release 0001 edit) by aggregates — `usage_stats` (uses, last use,
  decayed score), `query_choices` (folded query prefix ≤ 32 chars → item, uses, last use),
  `pins` (never expire). `prune_usage(cutoff)` = retention, `clear_usage()` = delete history
  (pins stay); all cascade when an item disappears.
- **Frecency:** exponential decay, half-life 14 days; stored as `rank_key = ln(score) + λt`
  (log-sum-exp updates), so ordering by recent use is an index scan with no periodic rewrite.
  Weights: primary action 1.0, reveal 0.5, copy path 0.3.
- **Learned results:** a primary action after typing `q` records every prefix of `q`; the
  provider adds those items as candidates (even without a name match, base 0.6).
- **Priors (bounded):** pinned +0.05; frecency +min(0.08, 0.025·ln(1+f)); learned choice for
  the exact current query +min(0.35, 0.12 + 0.08·ln(uses)) — a habit (≈8 picks) can lift
  "Visual Studio Code" above a folder literally named `code`; one pick cannot.
- **Empty query** returns suggestions: pins (oldest first), then highest frecency.

**Consequences**

- Recording happens when actions execute (T109: `Store::record_use(item, kind, query_key, now)`).
- Settings (T8xx) must expose retention and "clear history"; default retention TBD (90 days
  proposed).
- Sandbox: usage lookups add ~0.3 ms p50 per keystroke (head of 50 candidates).
