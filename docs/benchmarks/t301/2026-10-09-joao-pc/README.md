# T301 Windows release PDF text measurement (2026-10-09)

Ryzen 5 5600H, 6 cores/12 threads, 16 GB RAM, Windows 11 x64; GTX 1650 present.
The resident `target/t212-release/lumen.exe` (PID 7404) continued indexing throughout;
builds also ran. Loaded-machine evidence, not an idle acceptance result or real-library ETA.

Reproduce from the repository root:

```powershell
cargo run --release -p lumen-bench --example pdf_text --locked -j 2 -- target/t301-pdf-text.json
```

`pdf-text.json` measures the actual bounded parser/chunker, content pass, durable FTS
and a no-change resume using self-generated standard-font PDFs and a temporary DB.
Twenty timed warm extraction samples after five warmups; 100 settled FTS samples after
five warmups. EstimateTokens, no model inference, GPU work or network. The user's DB
and resident executable were not opened for writing/stopped/restarted.

| Operation | Result |
|---|---:|
| 3-page / 15-chunk PDF extraction p95 | 0.484 ms |
| 32-page / 160-chunk PDF extraction p95 | 3.889 ms |
| 128-page / 640-chunk PDF extraction p95 | 14.848 ms |
| Content pass, 100 three-page PDFs / 600 chunks | 54.166 ms |
| Content pass, unchanged resume | 0 files processed |
| Settled `coral reefs ext:pdf`, 30 hits on page 3, p95 | 4.002 ms |
| Process working set before/after content pass | 10.00 / 10.88 MiB |

The corpus has repeated short text, compressible streams and simple fonts; it does not
establish throughput/memory for complex real PDFs, charts, uncommon encodings, scans or
adversarial aggregate parser allocations. Working-set snapshots are not peak/private WS.
The parser deadline is cooperative. Embedding remains the slower existing queue; these
figures do not estimate total semantic indexing time.

Browser visual check of the real RootSearch/PreviewPane components at 1280×720 with
synthetic page 7/123 results: page prefix survives long-name truncation, the matching
passage and page label appear in Quick Look, accessible selected-row context remains.
Temporary fixture/server were removed/stopped. Native window/material, actual file
handler/reveal, keyboard/IME, narrow preview and real PDFs remain in HANDOFF REVIEW.
