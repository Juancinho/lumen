# RELEASE_AND_LICENSING.md

## Release principles

- reproducible Windows release builds;
- signed binaries before broad distribution;
- automatic update path only after rollback/integrity design;
- no surprise network behavior;
- clear offline capability.

## Dependency audit

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
whole-product distribution audit. T302 rendering needs its own engine/license review.

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
