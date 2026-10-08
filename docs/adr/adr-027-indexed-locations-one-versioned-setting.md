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
