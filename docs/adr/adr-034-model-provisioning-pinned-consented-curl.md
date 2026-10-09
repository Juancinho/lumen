# ADR-034 — Model and runtime provisioning: pinned files, explicit consent, system curl, verified atomic install

**Status:** Accepted (T210). Code: `crates/lumen-provision` (`manifest`, `fetch`,
`install`), shell `provisioning.rs`, tray → *Semantic search*. Verified in the sandbox: the
pinned model (207 MB, 15 s) and runtime wheel (14 MB) downloaded through `curl`, hashed and
installed by the real code (`cargo test -p lumen-provision -- --ignored`).

**Context.** Search by meaning needs EmbeddingGemma 2 (ADR-015: q4 ONNX, 174 MB of weights
+ 32 MB tokenizer) and ONNX Runtime. Neither is in the repository or a development build,
and until now they came from environment variables. PRIVACY_SECURITY said "no network use
in the app"; this is the first, deliberate exception.

**Decision**

- **Pinned components, nothing resolved at run time.** `EMBEDDING_MODEL`: `tokenizer.json`,
  `onnx/model_q4.onnx`, `onnx/model_q4.onnx_data` and the model card (`README.md`, license
  metadata) from `onnx-community/embeddinggemma-2-ONNX` at commit `daa72c5…` (Apache-2.0;
  base model `google/embeddinggemma-2`, Apache-2.0, not gated). `INFERENCE_RUNTIME`: the
  official `onnxruntime` 1.30.0 cp312 `win_amd64` wheel from PyPI (MIT), of which only
  `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `LICENSE` and
  `ThirdPartyNotices.txt` are kept. Every file and every extracted member has a fixed size
  and SHA-256; a changed revision is a code change (and a new install folder).
- **Explicit consent.** Nothing is fetched until the user picks tray → *Semantic search* →
  *Download…* and confirms a dialog naming the total size, each component's host and
  license, and that nothing about their files or searches is sent.
- **Transport: the system `curl`** (`%SystemRoot%\System32\curl.exe`, shipped with Windows
  since 1803; Schannel TLS and the Windows certificate store), `--proto =https`, resumable
  (`--continue-at -`), killed on cancel. Lumen links no TLS stack and opens no socket.
  `DirFetch` installs the same pinned files from a local folder (offline machines, tests).
- **Install:** each component is staged in `<app data>/<id>/<version>.partial/`, every
  download hashed before use and every archive member hashed after extraction (minimal zip
  reader: stored/deflate, CRC + size checked, no ZIP64); the marker `installed.json` is
  written last and the folder renamed into `<app data>/<id>/<version>/`. A cancel, a crash
  or a bad file keeps what was verified; the next *Download…* resumes. Older versions are
  removed after a successful install.
- **Resolution order** (model and runtime separately): environment variables (development),
  `onnxruntime.dll` beside `lumen.exe` (packaged builds, ADR-015), then the installed
  components. The query lane exists from start-up and loads lazily; after an install the
  indexing thread and the query lane are told to retry, so semantic search starts without a
  restart.
- **Removal** (*Remove…*, confirmed): unloads both sessions and deletes the model; the
  vectors already computed stay (a later download does not re-index). The runtime stays — 18
  MB, and Windows keeps a loaded DLL locked until Lumen exits.

**Consequences**

- First-run semantic search is one click and ~222 MB; lexical search works without it.
- curl does not read the WinINET proxy settings (only `HTTPS_PROXY` / `ALL_PROXY`):
  corporate proxies may block the download — the error is shown in the tray and `DirFetch`
  is the fallback. A WinHTTP fetcher is the upgrade if this matters.
- Only Windows x64 gets a runtime download; elsewhere `LUMEN_ORT_DYLIB` is required.
- License notices are installed with each component (`README.md`, `LICENSE`,
  `ThirdPartyNotices.txt`); an About screen that shows them is UI work (T503-class).

## 2026-10-09 — T212 optional DirectML runtime

ADR-038 adds the separate pinned GPU_RUNTIME manifest (1.24.4 Windows x64, 26 MB,
files.pythonhosted.org, MIT). It reuses consent, cancellation/resume, wheel/member
SHA-256 and atomic install; the CPU default/model manifest is unchanged. ORT cannot
switch loaded DLLs: runtime selection is pinned for each process and an optional GPU
installation requires restart. On restart an enabled preference selects its installed
DirectML library before a CPU library beside the exe. Development overrides still win.
Both CPU query and GPU indexing sessions use that one runtime, preserving index identity.
