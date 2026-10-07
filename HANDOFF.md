# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main`. Commits: spec-pack baseline → T001 → T011.

## Active task

None claimed. T001 and T011 are DONE.

## T011 — implemented behavior

Universal command model in `crates/lumen-core/src/` (summary in `docs/COMMAND_MODEL.md` §0):

- `ids.rs`: `ProviderId`/`ActionId` = namespaced names (`segment(.segment)+`, `[a-z0-9][a-z0-9_-]*`,
  ≤64 B). Built-ins: `const X: ActionId = ActionId::from_static("lumen.open");` — invalid literal is
  a compile error (compile_fail doctest). `lumen.` reserved for built-ins. `ResultId` (`Arc<str>`)
  = identity of the *entity*, equal across providers/batches; convention `<kind>:<stable key>`
  (`ResultId::from_parts`). `QueryId(u64)` ≤ 2^53−1, `is_stale(latest)`.
- `result.rs`: `ResultItem { id, provider, kind, title, subtitle, detail, icon, score, capabilities,
  primary_action, secondary_actions, payload }`; `ResultKind` {File, Folder, Application, Command}
  (`non_exhaustive`, add variants per task); `IconRef` {KindDefault, FileExtension, Native};
  `Confidence` ∈ [0,1], total order, -0.0 normalized; `MatchKind`; `Payload` {Path, Text,
  ProviderKey} — Rust-only.
- `capability.rs`: `CapabilitySet` u32 bitset {LocalPath, Launchable, TextValue, Pinnable}.
- `action.rs`: `ActionDescriptor { id, title, safety, group, requires }`; `ActionSafety`
  {SafeRead, SafeReversible, Privileged, Destructive, ExternalData} with `requires_confirmation`
  (Destructive|Privileged) and `allowed_as_primary` (not those two); `ActionGroup` ordered for the
  Action Panel; `ActionLookup` trait (slice/array/HashMap).
- `contract.rs`: `validate_result(&item, &lookup) -> Vec<ContractViolation>` (empty title, unknown,
  duplicate, unsafe primary, missing capabilities, inconsistent descriptor). Test fixtures in
  `contract::fixtures` (crate-private, cfg(test)).
- `execution.rs`: `ActionRequest` (ids only + `Invocation` + `confirmed`) →
  `ExecutionContext::authorize(req, &item, &descriptor, token)`; checks result/descriptor identity,
  offered by result, `Invocation::Primary` ⇒ primary action, capabilities, confirmation.
  Actions on older queries are intentionally allowed. `CancellationToken` (Arc<AtomicBool>).
- ADR-013 in `docs/DECISIONS.md`: no serde in core; shell projects DTOs; Payload/confidence never
  reach the UI.

## Files changed (T011)

`crates/lumen-core/src/{lib,ids,result,capability,action,contract,execution}.rs`,
`docs/DECISIONS.md` (ADR-013), `docs/COMMAND_MODEL.md` (§0), `docs/DEVELOPMENT.md`, `TASKS.md`,
`PROJECT_STATE.md`, `WORKLOG.md`, this file.

## Validation (Linux sandbox, Rust 1.97.0)

All passed:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace     # core 32 unit + 3 doc (1 compile_fail), shell 2, xtask 8
cargo xtask arch           # OK - lumen-core
cargo doc -p lumen-core --no-deps   # no warnings
cd apps/desktop && npm run check    # unchanged frontend, 3 tests
```

Not run on Windows for T011 (pure Rust, no platform code); `cargo test --workspace` there is a
cheap confirmation.

## Exact next steps

1. **T002 overlay prototype** (needs Windows for interactive verification): borderless,
   always-on-top, hidden at start, show/hide/focus, Escape hides, hide on focus loss, tray icon
   with Show/Quit; keep one WebView, no work while hidden. Window/tray code stays in the shell
   (or a future `crates/lumen-windows` adapter with no Tauri deps).
2. Parallel-safe alternatives: T007 (SQLite/FTS → new `crates/lumen-storage`), T008 (USearch
   bench), T009 (file identity → `ResultId` key choice), T010 (CI), T005 (`EmbeddingBackend`).
3. First provider task (T101) adds the provider trait; reuse `QueryId`, `CancellationToken`,
   `ResultItem`, `validate_result` — do not create a parallel model.

## Known issues / notes

- `ActionRequest.confirmed` is trusted from the UI (same app); it prevents accidental execution,
  not a hostile UI.
- Capability-derived actions beyond `secondary_actions` (T108) will need `authorize` widened to
  accept registry-offered actions; keep the "UI sends ids only" rule.
- Plain `cargo run` of `lumen-desktop` loads the dev URL; use `npm run tauri dev|build`.
- If Windows git reports "dubious ownership": `git config --global --add safe.directory D:/Proyectos/lumen`.

## Unresolved evidence-based decisions

- production EmbeddingGemma runtime (T006);
- exact native backdrop path (T004);
- vector scalar profile (T008);
- FastFrame/egui comparative shell spike timing (TX01);
- TS binding generation (revisit per ADR-013 when DTOs > ~10).
