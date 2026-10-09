# T112 — Manual file and extension exclusions

## 0. Implementation status (2026-10-09)

Built on T111 locations/exclusions and T108/T109 actions; native review pending.
User priority before T304. ADR-027 dated amendment; no new dependency, database migration,
model generation, query syntax, design tokens, WebView or settings window.

## 1. User paths

- Root file result → Ctrl+K → **Exclude this file from Lumen** or **Exclude all .js files**
  (label uses its actual extension). Arrow keys select; Enter applies; Escape cancels.
  Code, PDF and image hits retain these actions on the same file identity. Applications
  do not offer them; folders keep the existing Exclude folder action. Extensionless files
  offer only exact-file exclusion. Ordinary Enter still opens the file.
- Tray → **Exclusions → File types**: independent checks for `.js`, `.json`, `.log`, plus
  active custom types. All are initially off. **Exclude another file type…** opens a
  native picker whose title says to choose an example file and exclude all of its type.
  It reads only the path. A file without an extension produces a short native notice.
- Tray → **Exclusions → Exclude files…** supports multiple exact files via the native
  picker. The existing path list's **Include again** removes an exact-file/folder rule;
  unchecking a file type removes its extension rule. File/folder/extension rules combine
  with OR: removing one leaves any other applicable exclusion active.
- The native tray/pickers remain OS keyboard accessible. Ctrl+K uses the established
  listbox, query-field focus and authorized ids-only action path (ADR-026).

## 2. Rule semantics and persistence

`index.locations` advances to JSON v3 with `exclude_extensions: ["js", "json", "log"]`.
The existing `exclude_paths` now also receives selected individual file paths. Unknown
fields survive saving; future versions and invalid values retain T111's read-only policy.
v1 upgrades names-only defaults to content as before; v2 preserves explicit names-only
choices. Rules save only after a user edit, never change defaults automatically.

Extensions are literal last filename suffixes: surrounding whitespace is trimmed and
`.JS`/`js` normalize to `js` using ASCII case folding. They exclude files, links and unknown non-directory entries, never a folder
named `folder.js` or its unrelated children. `.js` does not exclude `.jsx`; `.json` does
not exclude `.jsonl`; `.gz` matches `archive.tar.gz`. Globs, paths, embedded whitespace, compound
suffixes and Windows-invalid filename characters are refused. This task introduces no
regular expressions or patterns. Path rules remain component bounded and case insensitive
on Windows; they follow the stored path, not later moves of the disk file. Existing name
rules apply inside selected roots, matching inventory; ancestors above an explicit root
cannot accidentally exclude its catalog/content during cleanup.

## 3. Catalog and indexing

Every edit uses the existing settings save/cancel/recovery pass. Before that pass's full
file inventory, the existing writer prunes explicitly excluded file-source catalog rows
in keyset pages of at most 512 paths and transactions of at most 512 deletions. Only
path/kind metadata is read. This also removes excluded entries from offline/unreadable
roots where ordinary inventory correctly preserves unrelated stale entries.

Existing foreign keys remove owned chunks, FTS rows, vectors, usage and pins. ANN candidates
continue to be validated against SQLite. Startup, recovery inventory and watcher hints
all obey the rules; content eligibility also refuses excluded paths/types. No filesystem
file is deleted, opened, renamed or rewritten by exclusion. The application catalog is
unaffected. Unrelated file identities/chunks/vectors and the active generation stay intact.

Cancellation applies between pages and at existing indexing boundaries; an in-flight
native embedding call must finish first. Shown search refreshes on catalog change; hidden
UI stays quiet. Exclusion therefore takes effect on writer processing, not synchronously
on the menu thread. Including again inventories and extracts/embeds the affected files;
offline locations restore when available. It does not re-embed unrelated files.

## 4. Privacy, performance and validation

Works fully offline with local settings/catalog paths only. No content read, network call,
telemetry or cloud dependency. Same single root/action surface and native tray.
The performance budget remains existing interactive provider budgets; cleanup is bounded
background work on the writer, cancellation between pages, with no hidden polling.

Tests cover literal full/watcher scans and restoration; v1/v2/v3/future settings;
offline cleanup across multiple pages, cascades, directories, application preservation,
cancel/idempotence, disk safety, unrelated identity preservation and ids/capability policy;
frontend Ctrl+K/arrow/Enter uses only result/action ids.
Release synthetic 100k cleanup and concurrent retained-name queries:
`docs/benchmarks/t112/2026-10-10-joao-pc/README.md`.
Native multi-picker/tray persistence, live removal/restoration and DPI/keyboard review
remain in HANDOFF, deferred until the user chooses to switch the resident application.
