# T302 — PDF Quick Look and physical-page navigation

## 0. Implementation status (2026-10-09)

Implemented; native overlay/registered-viewer review pending. T301 text/index targets,
T105 preview and T108 action authorization are reused. ADR-040 owns renderer bounds;
release evidence is in `docs/benchmarks/t302/2026-10-09-joao-pc/`.

## User contract

- Root search stays unchanged. Alt+Enter lazily renders the matched physical page;
  a filename-only PDF opens page 1, including scans without text. No OCR/vision indexing.
- Previous/next, a page-number field and Return to match live in the existing preview.
  Alt+PageUp/Down from the query navigates PDF pages; plain arrows/PageUp/Down continue
  navigating results. Tab reaches controls, Escape closes preview then overlay; Ctrl+L
  returns focus to the query. Return to match restores query focus; Escape/Alt+Enter also
  work from page controls. Refinements reset to their matching page without moving rows.
- Ctrl+K offers Open matched PDF page for trusted PDF targets. The registered SumatraPDF
  handler supports a documented page argument; unsupported viewers use Lumen's page
  preview. Enter/Open still opens the file with its normal handler; no viewer is installed
  or reassociated. Current preview navigation does not mutate the matched result target.
- Offline, local only: Windows.Data.Pdf renders raster PNGs on one background worker.
  UI receives ids/page counts/bounded images, never raw PDF, file URLs or viewer commands.
- One running + one latest pending request; cancellation on selection/close/hide. No
  pre-rendering every result/page, disk cache, new WebView, indexing/model/schema change.
  Source 16 MiB, 512 physical pages, image maximum edge 960 px, PNG 6 MiB, four/12 MiB
  cached rasters and one source document. Cache is memory only and clears on hide/new query.
- Five-second cooperative async deadline per load/render stage, OS cancellation requests; native decoder heap
  and cancellation completion are not hard sandbox guarantees. Unsupported/malformed,
  encrypted, over-limit or offline placeholder PDFs keep metadata/indexed-text preview.
  Cached preview target is the existing <50 ms median/<120 ms p95 budget; release evidence
  must report actual measured cold/warm/cache costs and corpus limitations.

## Required checks

Synthetic Windows rendering/page/rotation/blank-page/limits/cancel/cache invalidation,
capability authorization/structured viewer args, privacy DTO, latest-wins/cancel ordering,
frontend controls/refinement/late answers/focus/IME, browser visual states, full gates and
release timing. Native overlay/registered-viewer integration remains explicit REVIEW if
not observable without interrupting the user's resident indexing instance.
