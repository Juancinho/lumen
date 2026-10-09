# ADR-040 — on-demand Windows PDF raster previews and supported page transport

**Status:** Accepted for T302 (2026-10-09); native overlay/viewer QA remains REVIEW.

## Decision and evidence

Use the Windows `Windows.Data.Pdf` WinRT component through the existing pinned
`windows 0.62.2` adapter in `lumen-windows`. Load bounded source bytes into an in-memory
stream; render one physical page to a PNG with explicit dimensions. The adapter exposes
plain Rust models, with no Tauri/React/WebView dependency. The desktop owns ids-only
commands and one preview worker. No new process, WebView, inference, extractor or schema.

Primary API sources: [loading from a stream](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfdocument.loadfromstreamasync),
[page render options](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfpage.rendertostreamasync),
[async cancellation](https://learn.microsoft.com/en-us/uwp/api/windows.foundation.iasyncinfo.cancel).
Pinned generated Rust API sources were inspected. Tests and release measurements run
successfully in this unpackaged Windows desktop environment; no UWP packaging required.

PDFium would add a separately redistributed native engine/DLL/notice and provisioning
surface; PDF.js would add renderer/worker assets in the WebView. Neither is needed to
meet this task after the actual Windows adapter works. T301's lopdf remains text-only.
Windows owns servicing its renderer. No OS DLL is copied or redistributed; no new Cargo
package enters the lockfile (base64/futures-channel were already transitive dependencies).
The existing Windows bindings are MIT/Apache; this is not a whole-product license audit.
No Sumatra executable or GPL engine code is bundled; it is an optional user's viewer.

## Resource and privacy contract

- One running job and one latest pending job, cancellation tokens and monotonic request
  ids, async oneshot replies. Replacing/closing a preview cancels its own request; older
  cleanup cannot cancel a newer request. Cancellation arriving before submission is honored.
- The native objects and balanced MTA apartment lifetime stay on their worker thread;
  `Renderer` cannot be sent/shared between threads. It initializes WinRT only on first use.
- Bounds: source 16 MiB including a growing-file read; at most 512 pages; PNG maximum edge
  960 px (≤921,600 pixels), 6 MiB encoded output. One source document and at most four/12 MiB
  raster pages. Memory cache only, never source-derived disk files. Source size/mtime/creation
  metadata invalidate cache; a new query also clears it, covering T207 same-metadata writes
  because visible catalog commits start a fresh query. No all-result thumbnail pass.
- Five-second cooperative deadline per OS load/render stage; cancellation requests reach
  the Windows async operation. Polling occurs only during that requested operation; the
  worker sleeps on a condition variable at idle. Native cancellation completion/decoder
  heap are not hard execution/resource sandbox guarantees; cancelled OS work may take time
  to settle. No isolation without TX02 evidence. Complex files need native soak review.
- Hide atomically rejects new jobs, cancels pending/running work and releases the cache
  on the worker. A hidden event closes the React pane and clears its image/text answers.
  Nothing pre-renders or polls while hidden. No query, index writer or UI thread renders.
- Explicit Quick Look may preview a filename-only/scan PDF without content indexing; this
  user-invoked read adds no indexed content, OCR or visual embeddings. Offline/recall cloud
  placeholders are refused before reading. PDF input never reaches JS; it receives only a
  bounded OS-produced PNG data URL, page metadata and existing indexed text. Rasterization
  executes no document links/actions in Lumen; no network/telemetry is added.

## Interaction and actions

Alt+Enter starts at T301's matched physical page or page 1 for filename-only results.
The existing preview has previous/next, page-number entry, Return to match and Open file.
Alt+PageUp/Down navigate pages; ordinary result arrows/PageUp/Down and Enter remain as
before. Tab accesses controls, Ctrl+L returns to search, Escape/Alt+Enter from controls
closes the preview and restores query focus. Return to match restores query focus before
its temporary button disappears. Indexed matching text remains accessible as a collapsible
excerpt labelled with its original page, even while another page is viewed or fails.
Loading/failure retain metadata/excerpt, and no late answer changes the selected file.

`Capability::PdfPage` offers `lumen.open-pdf-page` in the Action Panel, through the existing
core authorization policy. Enter/Open keeps the user's file handler, including exact-name
rows. The shell queries `.pdf`'s registered executable using
[AssocQueryStringW](https://learn.microsoft.com/en-us/windows/win32/api/shlwapi/nf-shlwapi-assocquerystringw)
and supports only the documented
[SumatraPDF page arguments](https://www.sumatrapdfreader.org/docs/Command-line-arguments).
Use an absolute executable/file and separate `Command` arguments (`-reuse-instance`,
`-page`, numeric page, trusted file); never execute a registry command template or shell
string. Unknown handlers return an authorized preview outcome; the UI opens the matched
page in Lumen, with ordinary Open file still available. This makes the spec's fallback
explicit without launching a viewer and immediately stealing its focus back. No guessed
Edge/Adobe transport, default-handler change, raw offset or UI-supplied path/command.

## Validation and consequences

Synthetic tests cover real Windows blank/text/image-only pages, rotation, Unicode paths,
source edits, PNG/cache identity, input/page limits, encryption/malformed data, cancellation
and recovery. Tests cover capability refusal, structured viewer plans, queue/cancel/hide
ordering, wire commands, refinement/late answers, controls/IME/focus and the existing
keyboard/selection behavior. Release measurements and limitations:
[T302 evidence](../benchmarks/t302/2026-10-09-joao-pc/README.md).

First OS load is deferred behind instant metadata and can be slower than a cached preview.
Images are thumbnails scaled to the pane with scrolling, not a full PDF editor/zoom UI.
No native user-window/registered Sumatra integration is claimed while the resident T212
app keeps indexing. The browser component check does not close those native REVIEW gates.
