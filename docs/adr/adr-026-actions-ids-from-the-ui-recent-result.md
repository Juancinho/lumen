# ADR-026 — Actions: ids from the UI, recent-result lookup, core policy, shell executors


**Status:** Accepted (T108/T109). Code: `lumen_search::{available, prepare}`,
`SearchService::lookup`, `lumen_catalog::usage::record_action`, shell `actions.rs`, UI
`features/root-search/{useActions.ts, ActionPanel.tsx}`.

**Decision**

- The UI sends `(queryId, resultId, actionId, invocation)` only. The shell looks the result
  up among the last 4 queries' results the search service delivered (the user acts on what
  they saw, even a query behind), then `prepare` = registry lookup + `ExecutionContext::
  authorize` (offered, primary-only for Enter, capabilities, confirmation). Unknown or stale
  results are refused, never resolved from UI-supplied paths.
- Action Panel order: primary first, then offered actions by group (Primary, Common,
  Navigation, Advanced, Destructive), provider order inside a group; actions whose
  capabilities the result lacks are hidden.
- Executors (shell): open = ShellExecute (`tauri-plugin-opener::open_path`, Rust API only — no
  JS permission granted), launch = ShellExecute of `shell:AppsFolder\<id>` or the shortcut,
  reveal = `SHOpenFolderAndSelectItems` (`reveal_item_in_dir`), copy path = `arboard`.
- After success: record the use (ADR-023 kinds and learned query key) through the settings
  writer, then hide the overlay. Failure: a short "Couldn't do that" on the row; the reason
  goes to logs only.
- Keys: Enter primary, Ctrl+Enter reveal, Ctrl+K panel (↑/↓ + Enter inside; Escape or Ctrl+K
  closes it before Escape dismisses), click runs the primary action.

**Consequences**

- Pin/favorite and open-with (also in T109's title) need UI for a second level (pin list,
  app chooser); deferred to T409/T8xx with their settings. "Copy value" arrives with
  providers that have values (calculator T402).
- Linux smoke: Ctrl+K panel renders and runs Copy path (105 ms incl. X11 clipboard).
