# ADR-020 — Hidden WebView: trim to low memory after 30 s hidden


**Status:** Accepted (T012), refines ADR-010. Evidence:
`docs/benchmarks/t012/2026-10-08-joao-pc/webview-lifecycle-4modes.json` (Ryzen 5 5600H,
Windows 11, WebView2 154.0.4258.62, release `lumen.exe`, 20 quick show/hide cycles + 3 shows
after 8 s hidden per mode). Code: `apps/desktop/src-tauri/src/lifecycle.rs`.

| mode | show → painted p50/p95 | after 8 s hidden | private WS hidden | commit |
|---|---:|---:|---:|---:|
| keep (window hide only) | 22.6 / 26.0 ms | 11.7 ms | 72 MiB | 146 MiB |
| invisible (`SetIsVisible(false)`) | 17.6 / 21.5 ms | 26.5 ms | 73 MiB | 146 MiB |
| low-memory (+ `MemoryUsageTargetLevel=Low`) | 27.1 / 32.0 ms | 26.7 ms | **7.4 MiB** | 149 MiB |
| suspend (+ `TrySuspend` after 5 s) | 28.6 / 31.0 ms | 30.5 ms | 94–160 MiB | 276–283 MiB |

Start-up to UI ready: 380–523 ms wall (in-process 367–408 ms). Shell process alone: 3–4 MiB.

**Decision**

- Default `idle-low-memory`: nothing on hide; after 30 s hidden, `SetIsVisible(false)` +
  `MemoryUsageTargetLevel=Low`; on show, Normal + visible. Quick re-opens get `keep` latency,
  an idle resident Lumen shows ~7 MiB in Task Manager. User-approved 2026-10-08.
- `suspend` rejected (more memory, an extra process); `invisible` gives nothing.
- Delayed trims re-check on the UI thread that no show happened since (no trim while visible).
- `LUMEN_WEBVIEW_HIDDEN` overrides the mode; `LUMEN_DIAG_LOG` records timings; second launches
  accept `--show/--hide/--toggle/--quit`.

**Consequences**

- Memory budget (<400 MB idle) met with a wide margin; commit (~150 MiB) is the real reserve.
- Low-memory trims page out, they do not free commit; re-show after a trim costs ~27 ms.
  T103 (real result list) must re-run `scripts/t012/run-windows-webview.ps1`; if show→paint
  after a trim exceeds the 35 ms p50 budget, raise `IDLE_TRIM_AFTER` or use `keep`.
- The composed `idle-low-memory` mode was not measured as such yet:
  `run-windows-webview.ps1 -Modes idle-low-memory` validates it.
- Show latency excludes hotkey delivery (measured from the shell receiving the request).
