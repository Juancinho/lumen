# ADR-027 — Indexed locations: one versioned setting, visible default exclusions

**Status:** Accepted (T111), spec `docs/specs/T111-indexed-locations.md`. Code:
`lumen_catalog::locations`, `lumen_indexer::Exclusions::{default_names,
build_dirs_next_to_markers}`, shell `catalog.rs` + tray. Evidence:
`docs/benchmarks/t111/2026-10-08-cloud-sandbox-whole-fs.json`.

**Decision**

- **One setting** `index.locations` (JSON, `version: 1`): locations (folders or whole drives,
  `content: "names"` reserved for M2), excluded folders, excluded names, default rules.
  Unknown fields survive a save; a value from a newer Lumen is used read-only (the standard
  folders are indexed and nothing is written); a broken value never wipes the list.
- **First run** = the standard folders, not saved until the user edits (same rule as the
  shortcut, T003). After that the saved list is the only source of truth.
- **Defaults, visible and toggleable:** OS folders (`SYSTEM_EXCLUSIONS`), developer noise
  (`node_modules`, `.git`, `.venv`, `venv` only with `pyvenv.cfg`, `__pycache__`,
  `.pytest_cache`, `.mypy_cache`, `.gradle`, `.next`, `.nuxt`, `.turbo`) — directories only —
  and build folders (`target`, `build`, `dist`, `bin`, `obj`) only next to a project marker
  (`Cargo.toml`, `package.json`, `*.csproj`, `pom.xml`, `build.gradle*`, `CMakeLists.txt`).
  Every exclusion is reported by rule (ADR-018). Open questions resolved by default: the
  developer-noise set is on for everyone (toggleable); a whole system drive is allowed and
  pre-excludes `Windows`, `Program Files*`, `ProgramData`, `%LOCALAPPDATA%\Temp` (removable).
- **States per location** from the pass: ok / not available (root did not open — items kept)
  / partial (n folders unreadable — their items kept).
- **Sync:** any edit cancels the running pass (a cancelled pass removes nothing) and starts a
  new one; `lumen:catalog-changed` is also sent during a pass (≥ 750 ms apart) when new
  entries were written, so small locations are searchable before a big drive finishes.
- **UI until a settings window exists:** tray → Indexed locations (add via the native folder
  picker, remove) and Exclusions (defaults as checks, add/remove folders); Action Panel →
  "Exclude folder from Lumen" on folder results (`lumen.exclude-folder`, safe-reversible).

**Consequences**

- Sandbox, whole filesystem with defaults: 308,839 entries, first sync 38 s, resync 12.5 s,
  keystroke p95 7.1 ms, 1,751 developer-noise subtrees excluded. A multi-million-entry drive
  must be measured on Windows (`scripts/t111/run-windows-locations.ps1`) before the 30-min
  full resync stays the default for big locations (T207 replaces it with change journals).
- "Add to indexed locations" from the Action Panel waits for a provider that returns
  results outside the locations.

## 2026-10-09 amendment — T112 manual files and file types

The user needs to suppress already indexed `.js`, `.json` and `.log` results manually.
Reuse the same setting/tray/action model: JSON v3 adds literal `exclude_extensions`,
normalized without a leading dot; `exclude_paths` also accepts individual file paths.
v1 content migration stays; v2 names-only choices and unknown fields survive. Future
settings remain read-only. No schema migration, dependency, model or public API is added.

Ctrl+K file results offer reversible exact-file/type exclusions derived from their trusted
payload; the UI still sends only ids. Tray Exclusions adds file-type checks (js/json/log
off by default plus saved custom types), an example-file picker for another extension,
and a multiple-file picker. Existing Include again and unchecked types undo rules.
Literal suffixes match non-directories only, not folder names or globs.

Explicit exclusions represent user intent even when a location is offline. Before a full
inventory, prune matching file-source rows on the existing writer in 512-row keyset pages
and deletion transactions; cancellation between pages prevents further cleanup, but
previously committed explicit exclusions remain applied. Ordinary cancelled/unavailable
inventory still preserves unrelated rows. Cascades remove only derived catalog/chunks/
vectors/usage, never disk files; re-including restores affected files on inventory/content
passes. Current native inference finishes cooperatively first; unrelated vectors and the
active generation survive. No UI-thread disk walk or hidden query polling.

Evidence/contracts: `docs/specs/T112-manual-exclusions.md`, model-free Windows release
100k fixture in `docs/benchmarks/t112/2026-10-10-joao-pc/`. Native review pending.
