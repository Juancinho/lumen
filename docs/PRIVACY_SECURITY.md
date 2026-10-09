# PRIVACY_SECURITY.md

## 0. Implementation status (2026-10-09)

- Stores on device only: `%APPDATA%\dev.lumen.desktop\lumen.db` (catalog, settings, usage).
  Usage is aggregates only — frecency, learned query prefix → item, pins; no raw event log;
  retention/clear functions exist without UI yet (ADR-023).
- The UI can only act on results Lumen produced: requests carry ids, payloads stay in Rust,
  the core policy checks every action (ADR-013/026). The opener plugin is used from Rust only;
  no file-system or shell permission is granted to the WebView (CSP + Tauri capabilities).
- For user corpora, benchmarks/scripts store counts/timings, never names or queries; material
  screenshots stay in git-ignored `target/t004/`; diag logs are opt-in (`LUMEN_DIAG_LOG`).
- Network: none, except the user-initiated, confirmed download of the embedding model and
  inference runtime (tray → Semantic search → Download…; pinned URLs on huggingface.co and
  files.pythonhosted.org, SHA-256 verified, via the system `curl`; ADR-034). Nothing about
  files, queries or usage is sent. Not built yet: clipboard history, Rewind.
- Optional dedicated-GPU acceleration (T212/ADR-038) uses a consented, pinned DirectML
  runtime download from files.pythonhosted.org. Its isolated compatibility probe reads
  synthetic inputs only, opens no catalog DB and exits before Tauri startup. Local settings
  retain hardware/model/runtime identities and counts/timings, never indexed content.
- T302 PDF Quick Look is an explicit local read, including filename-only/scan PDFs;
  it adds no indexed content or network path. Only a bounded OS-produced PNG reaches the
  WebView; raw PDF/viewer targets stay in Rust. Memory cache clears on hide/new query;
  offline/recall placeholders are refused before reading (ADR-040).
- T303 reads image contents only in content-enabled locations after placeholder/admission
  checks. Dimensions, format, orientation and local source digest persist; RGB/pixels,
  EXIF GPS/camera history and generated captions do not. The UI receives only bounded
  metadata and whitelisted coverage codes; paths/digests stay in Rust. Optional vision
  weights use the same explicit download consent and pinned HF hashes (ADR-041). Queries
  and image inference remain local/offline. T303 benchmark evidence contains only numeric
  fixture names and public licensed photos/synthetic queries, identified in its README.

## 1. Default posture

Core Lumen works offline. Local files, queries, embeddings, clipboard history, activity events and usage ranking stay on device by default.

Privacy is a product feature, not only a legal requirement.

## 2. Sensitive stores

Potentially sensitive:

- query history;
- clipboard history;
- Rewind/activity events;
- snippets;
- indexed content/snippets;
- thumbnails;
- workflow parameters;
- extension credentials.

Use least retention and least plaintext exposure practical.

## 3. Clipboard

Clipboard history is opt-in.

Required controls:

- pause capture;
- excluded applications where feasible;
- manual clear;
- retention period/size;
- pinned items separate from expiring history;
- best-effort sensitive-content detection may warn but must not claim perfect secret detection.

Never sync clipboard by default.

## 4. Rewind

Rewind is opt-in and configurable.

Default concept is metadata/event journaling, not full continuous screen capture.

User must be able to:

- see what categories are captured;
- pause;
- exclude apps/folders;
- set retention;
- delete history;
- disable entirely.

If future screen capture is ever explored, it requires a separate ADR/product decision and must never silently become default.

## 5. Index exclusions

Respect:

- user exclusions;
- obvious OS/system/cache noise defaults;
- application-specific sensitive paths if explicitly configured.

Provide a visible "Why is this result here?"/source path and easy exclude/reindex controls.

## 6. Actions

Classify actions by risk.

Destructive examples (delete, terminate process, overwrite file) require explicit confirmation unless user deliberately configures a safe exception.

Shell execution must never be triggered merely because semantic search thinks a query resembles a command.

## 7. Extensions

Future third-party extensions get capability-scoped permissions and network disclosure. Secrets live in OS credential storage where practical, never in plain config files.

## 8. Optional cloud/LLM features

If added:

- disabled by default;
- exact data boundary shown;
- explicit provider/model choice;
- no hidden upload of local search corpus;
- local-only mode remains fully functional.

## 9. Telemetry

Default to no content telemetry. If product telemetry is added, it must be opt-in/clearly documented and never contain file content, raw queries, clipboard content or embeddings without explicit extraordinary consent.
