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
