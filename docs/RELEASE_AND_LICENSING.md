# RELEASE_AND_LICENSING.md

## Release principles

- reproducible Windows release builds;
- signed binaries before broad distribution;
- automatic update path only after rollback/integrity design;
- no surprise network behavior;
- clear offline capability.

## Dependency audit

T304 uses OS-serviced Windows.Media.Ocr through the already pinned MIT/Apache Windows
bindings. No new locked package, native OCR DLL, trained weights, Python or OS language
package is redistributed. Language availability is checked; no OS installation. Microsoft
supports desktop use with package identity; portable runtime capability/failure and local
synthetic proof are explicit in ADR-043. No signing/packaging change is introduced.

Before release, audit licenses/distribution terms for:

- model/runtime;
- PDF engine;
- OCR APIs/components;
- media codecs/FFmpeg if used;
- icon/font packages;
- ANN/database crates;
- updater.

Do not assume a development dependency is safe to redistribute commercially.

T301 PDF text backend: pinned `lopdf 0.45.0`, MIT, default features disabled; statically
linked Rust parser, no PDF renderer/DLL. ADR-039 records the source/license review and
limits. Ship `docs/licenses/pdf-extractor-notices.txt` with the app alongside existing
model/runtime notices. `scripts/t301/pdf-notices.ps1` regenerates the exact 28 introduced
package notices offline from the locked registry packages; this does not replace the
whole-product distribution audit.

T302 raster backend: Windows.Data.Pdf, an OS-serviced Windows component through the
already pinned MIT/Apache Windows bindings. No OS renderer DLL, PDFium/PDF.js package,
Sumatra executable/GPL engine or new Cargo package is redistributed. Sumatra is an
optional user's registered viewer; separate arguments use its documented page transport.
base64 0.22.1 and futures-channel 0.3.34 were already locked transitive MIT/Apache packages.
ADR-040 records the API/license scope and native proof; broad distribution still requires
the whole-product notice/signing audit.

T303 codecs: pinned `image 0.25.10` (MIT/Apache-2.0), default features disabled, only
JPEG/PNG/WebP/BMP. Eight new locked Rust packages have scoped notices in
`docs/licenses/image-codec-notices.txt`, regenerated offline by
`scripts/t303/image-notices.ps1`; ship as ImageCodecNotices.txt. No codec DLL/Python
production runtime is required. Optional vision graph/data and Apache-2.0 model card use
the existing pinned EmbeddingGemma 2 revision, separate consented 109 MB installation
and removal. ADR-041 records the processor/runtime contract. Public CC0 photo fixtures
are development cache only; attribution/hashes are in the benchmark README, no photo
assets are packaged. This scoped review does not replace the whole-product audit.

## Model

Record exact model/runtime version and license in third-party notices. Keep model download/version migration explicit.

## Data compatibility

Version:

- SQLite migrations;
- index generations;
- model preprocessing;
- workflow schema;
- future extension API.

Updates must not invalidate all search synchronously. Build replacement index generations in background where possible.
