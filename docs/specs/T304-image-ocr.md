# T304 — optional local image OCR

## 0. Implementation status

Built 2026-10-10, REVIEW (ADR-043). Native es-ES synthetic recognition and root FTS
passed on this portable Windows 11 build. Microsoft documents package identity as its
supported desktop contract: portable availability is checked, not promised everywhere.
No OS language installation, packaging change, model download or network OCR.

Content indexing → **Index text in images (OCR)** is off by default. Its cached line
shows processed/prepared/percentage, text/empty/pending/skipped/failed counts and profile
language when checked. Its denominator is prepared/admitted image units; the existing
root files-read meter separately covers the whole discovered library. Root uses the
existing settled FTS lane and same image ID/file actions;
`"ERROR 42" type:image ext:png` finds indexed words. Alt+Enter shows full bounded OCR
text and distinct off/pending/indexed/empty/skipped/failed/unavailable coverage.

## Scope and acceptance

- Off by default, native Content indexing opt-in; content-enabled locations/exclusions
  and cloud-placeholder admission remain authoritative.
- Supported PNG/JPEG/WebP/BMP get bounded exact visible text; no screenshot capture or
  retrospective app/window metadata. One image at a time on the existing background writer.
- Normal root content search, `type:image` and quoted exact text use the same file identity.
  Open/Reveal/Copy path remain; Alt+Enter exposes indexed text and honest OCR coverage.
- Keep existing visual/text/code/PDF chunks and vectors, same model/generation; enrich
  image text independently so OCR updates cannot invalidate expensive visual vectors.
- Offline recognition, installed language only, no required Python/native third-party
  distribution. Missing engine/language and empty/oversized/changed/cancelled work are
  distinct; cancellation and a bounded cooperative native timeout preserve pending work.
- Bound source/pixels/text/chunks/rounds. Interactive holds and pause/battery policy
  preempt new recognition work. No UI-thread inference or hidden polling.
- Verify native synthetic text/empty recognition and resource timing, persistence/FTS,
  type filters, source change/move/delete, resume/cancellation, opt-in/off and vector
  preservation, typed preview/decoder, keyboard path, build/lint/tests and docs/ADR.

## Bounds and persistence

Existing 16 MiB source/placeholder/EXIF codec admission; 4,096 px per side and 8M pixels
checked before full decode, 16 KiB UTF-8 text, one native image at a time, four images /
two-second cooperative writer slice, five-second native operation deadline. CPU OCR
does not consume the ONNX GPU/query session. Pause/interactive holds/battery/ADR-019
memory threshold defer new work through the existing scheduler. No engine when off or
drained, no hidden UI polling. Source and consent are rechecked before text publication.
OCR composites alpha over white so invisible RGB is not recognized. Visual preprocessing
and its version remain unchanged. Preview verifies the current canonical path against
the trusted remembered result as well as the stored digest/version; reused or moved IDs
cannot supply another image's text. The path remains in Rust.

Schema v6 `image_ocr` tracks source digest, version, language and indexed/empty/skipped/
failed state independently. OCR updates the existing image chunk's text/FTS; chunk id,
visual vector/ANN sequence and generation remain. Failed work retries in the next catalog
round; cancellation leaves the current candidate eligible. Unchanged moves retain OCR;
edits/deletions invalidate it with the image unit. Off cleanup is 512 rows/transaction on
the writer, including while paused; it resumes after a crash and retains image vectors.
The cached menu says removal is in progress until those pages finish. A v5 executable
refuses a v6 DB: any future scheduler rollback must retain schema support.

## Validation / native review

Native release reproduction/limits: `../benchmarks/t304/2026-10-10-joao-pc/README.md`.
Domain tests cover FTS phrase/type/ext and same-ID actions, reopen, consent denial,
vector/sequence preservation, off/re-enable, move/edit/delete, empty/size/cancel/read
failure. Shell/UI tests cover policy retry, preview text suppression when off and
whitelisted/bounded decoder. Browser actual PreviewPane in 640/340 px panels covers
long names, Unicode text/scroll and all seven states, light theme.

Remaining human REVIEW: native checkbox persistence and Alt+Enter/Escape/Ctrl+K in real
libraries, dark/high contrast/high DPI, unplugged/battery, no installed OCR language,
complex multilingual/small-font photos and long resource/driver soak. Scanned PDFs,
OCR semantic embeddings, raster image preview, Semantic Drop and Similar are separate.
