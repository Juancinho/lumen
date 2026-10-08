# ADR-028 — Retrieval chunks: ~128-token target, heuristic structure, bounded decoding

**Status:** Accepted (T201). Code: `crates/lumen-extract`. Evidence:
`docs/benchmarks/t201/2026-10-08-cloud-sandbox-repo-{code,docs}.json` (`lumen-bench chunk
--tokenizer`, EmbeddingGemma 2 tokenizer).

**Decision**

- **Size:** target 128 tokens, hard max 192 (ADR-015: ~128-token chunks roughly double CPU
  throughput vs ~260). Chunks carry byte offsets into the extracted text, a kind
  (`text`/`code`), a symbol name for code and a heading path or parent symbol as context.
- **Token counts** come from a `TokenCount`; the default `EstimateTokens` (one token per
  started 5 letters/digits of a word, one per other visible character) is slightly
  conservative against the real tokenizer — estimate/real p50 1.12 (code) / 1.15 (Markdown),
  p95 1.35 / 1.45; 6 of 2,309 code chunks and 2 of 785 Markdown chunks exceed 192 real
  tokens. T202 may pass the real tokenizer when exact sizes matter.
- **Structure by kind:** prose = paragraphs → sentences → words; Markdown = heading sections
  (heading-only sections travel with the next one), fences atomic up to the max; code =
  top-level regions (blank line + base indentation, closers excluded), large classes split
  into members one level deeper, long bodies into line windows with 2 lines of overlap; data
  = packed lines. Code chunks name their definition in 63 % of cases on this repository.
- **No Tree-sitter yet:** heuristics cover the supported languages without native grammars in
  the build (Windows MSVC builds stay simple). T209 revisits with relevance evidence.
- **Decoding:** BOM UTF-8/UTF-16LE/BE, else strict UTF-8, else Windows-1252 (legacy Spanish/
  Western text); NUL in the first 8 KiB = binary; files above 4 MiB skipped. Newlines
  normalized to `\n`. Every skip has a reason (unsupported, too large, binary, unreadable) —
  ADR-018's coverage guarantee extends to content.

**Consequences**

- T202 writes chunks with `start_offset`/`end_offset` (columns exist; `NewChunk` gains them
  then) and embeds `title: <name> | text: <chunk>` per ADR-015's prompt.
- Real chunks average ~80–100 tokens (estimate is conservative): if T014/T202 show the model
  is as fast at 160 real tokens, raise the target with evidence.
- Extraction + chunking runs at ~3 MiB/s in the sandbox, three orders of magnitude above
  embedding speed; it is never the bottleneck.
