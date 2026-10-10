# T304 native OCR evidence — 2026-10-10

Windows 11 Pro 10.0.26300, AMD Ryzen 5 5600H, 12 logical CPUs; installed es-ES OCR.
Unpackaged optimized Rust executable, existing resident Lumen indexing concurrently.
Generated public 1200×300 bitmap, Arial 38 pt: `ERROR 42` and a short Spanish sentence.
No user photos, filenames, queries, extracted content or live DB in this evidence.

`native-ocr.json`: native engine creation 12.41 ms; five sequential full domain
decode/recognize/re-read digest/SQLite FTS commits 23.38, 7.69, 9.06, 14.05, 7.92 ms.
Five root quoted phrase/type/image/ext/png results; five existing synthetic vectors and
ANN sequences survive enrichment and off cleanup. Working set 8.90→15.95 MiB snapshots,
not private/peak memory, GPU usage or whole-app budgets. Synthetic preservation vectors
exercise storage identity; this does not measure or recalibrate visual retrieval quality.

`native-final.json` repeats the complete path after the alpha/path-validation fixes:
engine creation 4.57 ms; commits 19.21, 7.60, 7.43, 7.86, 7.71 ms; working-set snapshots
8.37→15.29 MiB. All five hits, vector/sequence preservation and off cleanup pass again.
Independent loaded-machine samples establish correctness, not a speed comparison.

`installed-startup-gpu.json` records synthetic metrics from the existing GPU recheck
after installing this bundle: CPU/GPU text 5.385/12.199 chunks/s, prior/hybrid image
cycles 18.967/15.689 s, minimum cosine 0.99999982. GPU query p95 319.224 ms versus CPU
25.639 ms supports the retained CPU query lane. Both indexing routes were admitted and
the live queue resumed; OCR stays off until explicit opt-in. These are existing T213
probe metrics, not GPU OCR, general model quality or whole-app latency claims.

Separate release `ocr_probe` also passes empty recognition on an unaligned 201 px width,
preflight cancellation and cancellation during recognition/result publication. The first
native text call took 18.1 ms, subsequent calls 5.7–11.2 ms in that run. This is small,
clear text, not a general OCR accuracy/latency or photo-library ETA claim. Microsoft's
supported desktop contract requires package identity; portable API availability is
checked at runtime (ADR-043), never assumed or fixed with a silent installation.

Reproduce on a Windows machine with a profile OCR language already installed:

```powershell
New-Item -ItemType Directory -Force target/t304 | Out-Null
powershell.exe -NoProfile -File scripts/t304/create-ocr-fixture.ps1 -Prefix target/t304/fixture
cargo run --release --locked -p lumen-bench --example image_ocr -- target/t304/fixture.png target/t304/ocr.json
cargo run --release --locked -p lumen-windows --example ocr_probe -- target/t304/fixture.rgb 1200 300
```

The PowerShell 5.1/System.Drawing generator is development-only; production requires no
Python, extra OCR model, redistributable renderer or language installation. Fixture/raw
pixels remain in ignored target output; only synthetic counts/timings are committed.

Validation: 382 Rust tests including 41 shell, two deliberate network tests ignored;
workspace fmt/clippy and 15-crate architecture check; frontend format/lint/types, 88 tests
and production build. Actual PreviewPane browser QA at 1280×720 and 800×420 with 640/340
px panels covers long names, Unicode/scroll and seven OCR states, light theme. Native
tray/persistence, real-photo accuracy, dark/DPI/high contrast and resource soak remain
human REVIEW. Existing interaction/resource budgets are unchanged.
