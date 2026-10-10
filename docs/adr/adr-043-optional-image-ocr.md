# ADR-043 — optional local image text on the existing image unit

**Status:** Accepted (2026-10-10, T304); optional portable capability, native review pending.

## Decision

Windows.Media.Ocr reports an installed es-ES recognizer and a 10,000-pixel native
dimension cap on this machine from a standalone unpackaged Rust executable. Microsoft
[documents package identity as the supported desktop contract](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr).
Portable availability is therefore capability-tested, not promised across installations;
creation/recognition failures leave ordinary file/visual search working. No package or
OS language installation is performed. Respect installed profile languages.

Keep OCR off by default. Native Content indexing exposes opt-in and cached coverage.
Only content-enabled, admitted PNG/JPEG/WebP/BMP are read, offline, on the existing
writer. No OCR cloud, third-party runtime/weights, Python, screenshot capture or new
process. Cap recognition to 4,096 px/edge, 8M pixels, 16 KiB UTF-8 text; up to four
sequential images per round, two-second cooperative target and five-second native-operation deadline.
Existing image source/placeholder/orientation rules and interactive holds remain.
Native calls/allocation are not hard sandbox guarantees.
Composite transparency over white for OCR, leaving pinned visual preprocessing untouched.
The native engine is not created when off, drained or deferred; unload it on drain/off.
Pending OCR uses the existing 60-second policy retry on battery, interactive holds or
ADR-019's 768 MiB available-memory threshold. No UI polling or new idle timer is added.

## Consequences

Schema v6 adds independent image OCR coverage/digest/language/version. Put exact OCR
text into the existing image chunk, using its FTS update trigger. Preserve chunk id,
visual vector/sequence, generation and text/code/PDF content. OCR remains lexical
enrichment, not a second image/text embedding: exact visible words use root FTS while
ordinary visual semantics remain available. Source edit invalidation clears OCR;
unchanged moves preserve it. Off removes OCR text/coverage in bounded writer pages,
without deleting the image unit/vector. An old v5 executable cannot open a v6 store;
future scheduling rollback must retain the current schema support.

Root type:image/ext/quoted filters, same-row identity, existing Open/Reveal/Copy actions
and Alt+Enter text/coverage apply. Publish only bounded text/status/language in previews,
never pixels/digests/backend errors. Persist no source-derived benchmark evidence.
Preview also checks the current stored path against the remembered result to reject
deleted/reused/moved IDs; that validation path never enters the preview DTO.

## Evidence and limits

Windows 11 Pro 10.0.26300, Ryzen 5 5600H/12 logical CPUs, unpackaged release executable:
five generated 1200×300 text images produce five root phrase/type/ext hits. Engine
creation 12.41 ms; full decode/recognize/source verification/commit 7.69–23.38 ms.
Working-set snapshots 8.90→15.95 MiB, not private memory, peaks or whole-app budgets.
Native empty recognition, unaligned bitmap width and cooperative preemption pass.
Schema v5→v6, vectors/sequence, FTS/off, moves/edits/deletion, cancellation and typed
preview are covered; see `../benchmarks/t304/2026-10-10-joao-pc/README.md`.
Small clear text does not establish multilingual/photo/scanned-PDF accuracy or a library
ETA. OCR is exact lexical enrichment; scanned PDFs and image/text semantic OCR vectors
remain separate future work. Other Windows installations must pass runtime capability
checks; missing profile languages/engine are explicit without installing anything.
