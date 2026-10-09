# T301 — PDF text/page extraction and page-level hits

## 0. Implementation status (2026-10-09)

Implemented; native review pending. ADR-039 owns engine choice, resource bounds,
version compatibility and page/file identity. Text-bearing PDFs now feed the existing
content FTS and semantic embedding queue in locations with content indexing enabled.
No OCR, images, thumbnails or direct page navigation in this task.

## User contract

- Root search: ordinary words/meaning and `ext:pdf` / `type:document`; no new mode.
- Matching passage row: `Page 7 · guide.pdf`, stable file ID and selected position.
- Keyboard: Enter opens the PDF file, Ctrl+Enter reveals it, Ctrl+K uses existing
  file actions; Alt+Enter shows the indexed passage and physical page number.
- Offline: lexical PDF text works without a model; meaning uses the installed local
  model once its chunks are embedded. CPU queries and optional GPU indexing are unchanged.
- Consent/privacy: only enabled content locations; placeholder/exclusion rules persist.
  Source text/pages/vectors stay local; no PDF rendering service or model download added.
- Performance: extraction runs on the existing indexing thread, never the UI or
  first-results path. The ordinary typing lane remains names-only. Bound PDFs as in ADR-039;
  retain 128/192-token chunking and one parser at a time.

## Extraction and coverage

Chunk each physical page separately, including correct numbering through blank pages.
Store text chunks in existing tables/FTS, with offsets in concatenated normalized text.
The embedding queue resumes after restart and uses the active generation unchanged.
Encrypted, missing-text and malformed/unsupported text layers are reported with short
coverage codes. Unchanged skips wait for a file edit; transient I/O failures retry.
Mid-document cancellation leaves no partial indexed state. A scan-only PDF remains
searchable by its filename, but needs future OCR/vision for content retrieval.

Limits/partial coverage are explicit; PDFs above limits are skipped as a whole, not
silently truncated. Five-second deadline/cancellation are cooperative and cannot stop
an in-progress parser call. Reading order and uncommon font encodings remain limitations.

## Checks and continuation

Rust extraction/content/provider/fusion/projection tests plus frontend accessible row,
page-preview refresh and existing key/selection tests. Full repository format/lint/test/
architecture/build gates and Windows release extraction/query measurements are recorded
in HANDOFF and `docs/benchmarks/t301/2026-10-09-joao-pc/`.

T302 must reuse `PdfTarget`, existing actions and preview lifecycle for rendered previews
and supported-viewer page navigation; it must not redo extraction, replace the schema or
embed unchanged PDFs. Review rendering licenses/resources before choosing that engine.
