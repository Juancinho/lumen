# T111 — Indexed locations and exclusions (user-configurable)

> Task spec. Summarized in `SEARCH_AND_INDEXING.md` §7 and `ARCHITECTURE.md` §17 (T015);
> this file stays the detailed spec until T111 is DONE.

## 0. Implementation status (2026-10-09)

T111 is built and remains REVIEW for native Windows checks. T112 extends its existing
setting/tray/actions with manual exact-file and literal-extension exclusions; see
`T112-manual-exclusions.md` and ADR-027's dated amendment. The historical T111 scope below
does not limit that explicitly requested extension.

## Problem

The catalog only inventories the user's standard folders on C: (Desktop, Documents,
Downloads, Pictures, Music, Videos), hard-coded in `apps/desktop/src-tauri/src/catalog.rs`.
Files on another drive (e.g. `D:\Proyectos`) never appear, and there is no way to leave
noisy subtrees (`node_modules`, `.git`, build output) out of results.

## What already exists (reuse, do not rebuild)

- `lumen_indexer::ScanOptions { roots, exclusions, identity }`: several roots, nested and
  duplicate roots merged, a root that is a link is followed (the user picked it).
- `lumen_indexer::Exclusions { system_defaults, user_paths, user_names }`: subtree by
  absolute path, entry name anywhere, OS defaults. Every exclusion is reported with its
  rule (ADR-018 coverage guarantee: nothing is left out silently).
- Identity includes the volume, so equal file ids on C: and D: do not collide (T009 checked
  both drives on joao-pc).
- `lumen_catalog::sync_files` must receive the **complete** root set; items of roots no
  longer listed are removed. Items under a directory or root that failed to list are kept,
  and nothing is removed after a cancelled pass.
- App-data settings in `lumen.db` (`Store::{setting, set_setting, remove_setting}`, T003).

## Behaviour

### Locations (roots)

- A location is a folder or a whole drive (`D:\`). The user can add and remove them.
- First run: the current standard folders, shown in the list as normal entries the user
  can remove. Once the user edits the list, the saved list is the only source of truth
  (same rule as the shortcut: never silently replaced).
- Each location shows a state:
  - **ok** — last pass listed it;
  - **not available** — drive disconnected, network share offline, folder missing. Its
    items stay searchable (marked as not available) and are never deleted for this reason;
  - **partial** — some directories could not be listed (count + "show why").
- Removing a location removes its items from the catalog on the next pass (immediately
  triggered), except subtrees that are also covered by another location.
- Adding a whole system drive (`C:\`) pre-fills path exclusions for `Windows`,
  `Program Files`, `Program Files (x86)`, `ProgramData` and `%LOCALAPPDATA%\Temp`, shown
  to the user and removable. Apps keep coming from the Start menu, not from these folders.
- Removable and network drives are allowed; they simply go "not available" when absent.

### Exclusions

Three kinds, all visible and editable:

1. **Folder** — absolute path, excludes the subtree (`D:\Juegos`).
2. **Name anywhere** — exact entry name, case-insensitive (`node_modules`). No globs in v1.
3. **Defaults** — OS set (already in `SYSTEM_EXCLUSIONS`) plus a *developer noise* set,
   each entry toggleable: `node_modules`, `.git`, `.venv`, `venv` (only when it contains
   `pyvenv.cfg`), `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.gradle`, `.next`,
   `.nuxt`, `.turbo`.
   Build folders (`target`, `build`, `dist`, `bin`, `obj`) are excluded only next to a
   project marker (`Cargo.toml`, `package.json`, `*.csproj`, `pom.xml`, `build.gradle*`,
   `CMakeLists.txt`) — never by name alone, since users have real folders called `build`.

- User exclusions always win over locations and defaults.
- Adding an exclusion removes the matching items on the next pass (triggered immediately);
  removing one rescans that subtree.
- Excluded subtrees are counted per rule and visible ("12,430 entries excluded by
  node_modules"), consistent with ADR-018.

### Where the user does it

No settings window exists yet. Minimum UI for v1:

- Tray → **Indexed locations** submenu: list with state, "Add folder or drive…" (native
  folder picker), "Remove" per entry.
- Tray → **Exclusions** submenu: defaults with checks, "Exclude a folder…" (picker), list
  of user exclusions with "Remove".
- Action Panel on a file/folder result: **Exclude this folder from Lumen** and, for a
  folder outside every location, **Add to indexed locations** (risk class: safe, reversible).
- A full settings surface replaces the tray submenus later (onboarding/settings task); the
  settings model below must not change for that.

### Settings model

One typed, versioned value in `lumen.db` settings (key `index.locations`, JSON):

```json
{
  "version": 1,
  "locations": [{ "path": "D:\\Proyectos", "added_ms": 0, "content": "names" }],
  "exclude_paths": ["D:\\Juegos"],
  "exclude_names": ["node_modules"],
  "default_rules": { "dev_noise": true, "build_next_to_marker": true, "disabled": [".git"] }
}
```

`content` is reserved for M2 (`names` now; `names+content` when T201/T202 exist) so a
location can be catalogued by name without embedding its contents. Unknown fields are kept.

### Sync

- Any change to locations/exclusions cancels the running pass and starts a new one.
- One pass still covers all roots (required by `sync_files`), but `lumen:catalog-changed`
  is emitted per root as it finishes so results from a small root are not held back by a
  big drive.
- Diagnostics record entries, excluded and duration per root.

## Scale check (part of the task)

A whole drive can be 1–3 M entries (catalog measured so far: ~250 k). Measure on joao-pc
with a whole-drive location and record in `docs/benchmarks/t111/`:

- first sync duration and entries/s; DB size;
- keystroke p95 at that size vs the T102 measurement (7.9 ms at 247 k);
- periodic resync (every 30 min until T207) duration and CPU.

If keystroke p95 regresses beyond the T102 budget, or a resync takes longer than a few
minutes, open a follow-up (e.g. resync interval per location size, or bring T207's USN
journal path forward). Do not tune by intuition.

## Acceptance

- Add `D:\Proyectos` from the tray → its files are searchable after the pass, without
  restarting; removing it removes them.
- Disconnect a USB drive that is a location → state "not available", items kept; reconnect
  → ok, nothing re-inserted as new (identity unchanged).
- `node_modules` hidden by default; untick it → its files appear after the pass.
- `build` folder without a project marker is indexed; next to `Cargo.toml` it is not.
- "Exclude this folder" from the Action Panel removes the folder's results immediately.
- Settings survive restart; an old/unknown settings value never wipes the list.
- Tests: settings model round-trip + migration, marker-based rule, root-state mapping from
  `ScanReport`, removal vs not-available; Windows interactive check for picker and USB.

## Out of scope

Change watching (T207), content indexing per location (M2), glob patterns, per-location
schedules, a full settings window.

## Open questions (user) — resolved by default in ADR-027, revisit if you disagree

- Allow `C:\` as a whole location? **Yes**, with the system folders pre-excluded (removable).
- Developer-noise defaults on for everyone? **Yes**, each one toggleable in tray → Exclusions.
