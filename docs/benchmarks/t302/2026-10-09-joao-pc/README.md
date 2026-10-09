# T302 — Windows on-demand PDF page preview

2026-10-09, joao-pc: Ryzen 5 5600H (6C/12T), GTX 1650, 16 GB RAM, Windows 11 Pro
build 26300. Optimized Rust release, one MTA renderer thread, Windows.Data.Pdf. The user's
resident `target/t212-release/lumen.exe` PID 7404/start 18:04:57 remained running while
indexing; no restart, live DB writes, model work or downloads in this measurement.

`pdf-preview.json` uses a temporary synthetic 128-page/36,087-byte standard-font PDF;
the document and directory are removed afterwards. First requested page is physical 7.
No user's file names/content enter the benchmark. Page raster is 678×960, 8,211-byte PNG.

| Operation | Samples | p50 | p95 |
|---|---:|---:|---:|
| First OS initialization/load/page | 1 | 333.138 ms | — |
| New page in loaded document (pages 8–37) | 30 | 22.679 ms | 39.169 ms |
| Cached page 7 (metadata check + Arc lookup) | 100 | 0.035 ms | 0.053 ms |

Process resident snapshots 5.23→50.70 MiB; not peak/private decoder heap or whole-app
memory. Actual cache: one source document, four/12 MiB raster pages; source 16 MiB,
512 pages, edge 960 px, PNG 6 MiB. Shell cache clears on hide/new query, with zero idle
polling. Timings exclude IPC/base64/paint and first-results latency. Loaded machine,
simple warm corpus, not a complex/scan-heavy PDF performance guarantee or indexing ETA.
Cold initialization is deferred behind metadata/text and does not block search.

Native automated renderer tests cover text/blank/image-only PDFs, rotation, Unicode
paths, edited source, cache identity, cancel/recovery, encryption, malformed input and
16 MiB/512-page limits. Queue/cancel/hidden admission, capability authorization, safe
viewer argument planning and DTO/IPC checks are separate from raster timing.

Browser component visual review (actual RootSearch/PreviewPane, synthetic data and the
actual Windows-rendered PNG): 1280×720, two-pane and 800 px covered-list layouts,
long names/page 123, next/Return to match, loading and unavailable states preserving
the indexed passage. Page field sizing was corrected to existing spacing tokens;
Return to match restores query focus before its button disappears. The fixture/server
were removed. Screenshot: ignored `target/t302-preview-qa.png`. This is browser QA,
not native Lumen material/DPI/focus/viewer verification. Light theme observed; native
dark/high contrast/DPI and real PDFs remain the HANDOFF checklist.

Reproduce from repository root:

```powershell
cargo run --release -p lumen-bench --example pdf_preview --locked -j 2 -- target/t302-pdf-preview.json
cargo test -p lumen-windows --test pdf_render --locked -j 2
```

The benchmark writes a synthetic sibling PNG for local visual inspection; production
previews have no disk cache. Sumatra integration requires that user's installed viewer
be the registered PDF handler; no installation/default-association changes were made.
