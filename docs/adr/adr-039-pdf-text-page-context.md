# ADR-039 — bounded PDF text extraction and matched page context

**Status:** Accepted for T301 (2026-10-09); native overlay review remains.

## Decision and evidence

Use pinned `lopdf 0.45.0`, `default-features = false`, in the shell-agnostic
`lumen-extract` crate. Its bounded load/text APIs cover object/xref streams, total
page content and font ToUnicode streams. No Rayon pool, native DLL, Python, renderer,
OCR, external resource lookup or network is introduced. The library cannot execute
PDF JavaScript. Existing single writer/background content pass remains (ADR-025/029).

Primary sources: [crate metadata/API](https://docs.rs/crate/lopdf/0.45.0),
[MIT license](https://github.com/J-F-Liu/lopdf/blob/main/LICENSE).
Inspected the pinned registry sources for LoadOptions, bounded extraction and page-tree
recursion/reference guards; synthetic compressed page/font/xref tests exercise the
actual bounded APIs. `pdf-extract 0.10.0` was considered, but its older lopdf dependency
does not expose these load/extraction guards through its convenience API; a PDF renderer
would add packaging and an unnecessary T302 concern. Text layout/font support remains
best effort: complex columns and font encodings may have imperfect reading order.

Limits: 16 MiB source file (including a bounded growing-file read), 512 physical pages,
4 MiB per eager object/xref stream, per page content and per font CMap, 4 MiB combined
normalized text. Five-second cooperative deadline and cancellation checked around
loading and every page/chunking step. These are admission/output/per-stream bounds,
**not** a hard wall-clock or total parser-heap sandbox: a library call cannot be forcibly
interrupted, and multiple fonts/object streams have aggregate allocation overhead.
No new process isolation without evidence (TX02). One parser at a time; the existing
32-file write batch holds at most the same 4 MiB text per file as ordinary content.

## Data and result behavior

Add PDFs to existing consent-scoped content candidates. Extract/chunk each page alone,
then concatenate normalized page text with two newline separators. Chunk offsets refer
to this combined text; ordinals are unique per file and `chunks.page_number` is the
physical one-based page (blank pages still count). No schema migration: the column was
already present. Existing text/code chunks and `EXTRACTOR_VERSION = 1` stay unchanged,
preserving model/generation identity and all previous vectors. New PDF text uses the
same document prompts, pending queue, CPU/dedicated-GPU indexing policy and CPU queries.

Keep one row per file, as in ADR-032/036. The best lexical/semantic passage supplies
`Payload::Pdf(PdfTarget)` with trusted path, nonzero page and bounded indexed excerpt.
Fusion keeps that page and passage paired even if the name lane wins; exact filename
navigation retains its File presentation. UI DTOs expose only the page number and
existing display snippet, not executable paths or payloads. Page-prefix labels remain
visible when long filenames truncate. Alt+Enter displays indexed text with its page;
Enter opens the registered handler, Ctrl+Enter reveals, Ctrl+K offers existing file
actions. Thumbnails, rendered page previews and direct page navigation remain T302.

Prerequisite fix: the existing frontend event decoder only admitted the original four
result kinds and dropped code context. Admit/validate code and PDF display context at
that boundary, including positive integer PDF pages; strip extra payload/path fields.
Tests exercise real decoded messages, beyond component-only mocked rows.

Cancellation commits no partial PDF and leaves it pending. Unchanged encrypted PDFs
(including empty reader passwords), malformed/decode failures, missing text and limit
violations get stable `pdf:*` coverage skip codes and are retried after change. I/O
errors follow existing retry behavior. This avoids repeatedly parsing immutable bad
PDFs. No attempt to OCR scanned pages or index visual content: T303/T304 own that.

Shared indexed extraction also verifies ambiguous move/write notifications. Unchanged
PDF renames preserve chunks/vectors; actual edits invalidate stale content atomically.
Scope, cloud-placeholder exclusion and raw OS path handling remain existing behavior.

## Validation and licensing

Tests cover physical blank pages, Unicode maps/WinAnsi text, no cross-page chunks,
offsets/token limits, encryption, malformed inputs, input/page/text/time limits, compressed
page/font/xref limits, mid-file cancellation, consent, persistent queue/restart,
rename/edit vector safety, lexical/semantic page context, fusion and wire projection.
Windows release evidence and limitations: [T301 measurement](../benchmarks/t301/2026-10-09-joao-pc/README.md).

`scripts/t301/pdf-notices.ps1` reproduces notices for the exact 28 packages introduced
by this dependency from locked offline Cargo metadata. MIT/Apache/BSD declarations and
actual license files were inspected. Ship `docs/licenses/pdf-extractor-notices.txt`
alongside existing model/runtime notices. `alloc-stdlib 0.2.4` omits the repository-root
BSD file in its crate archive: the local copy was verified against its exact packaged
[VCS commit license](https://raw.githubusercontent.com/dropbox/rust-alloc-no-stdlib/ae42d22078b98549e987d2f03d12df7b984fde47/LICENSE).
This is a scoped dependency notice update, not a full distribution/signing audit.
