# HANDOFF.md

> Rewrite this file at the end of every substantial agent session. Keep only the current handoff.

## Active branch

`main`. Commits: spec baseline → T001 → T011 → T002.

## Active task

**T002 — REVIEW (owner: claude).** Implemented and Linux-smoke-tested; needs the interactive
Windows checklist below, then set to `DONE`. T001 and T011 are DONE.

## T002 — implemented behavior

- `apps/desktop/src-tauri/src/overlay/mod.rs`: `toggle`/`show`/`hide`; places window on the monitor
  under the cursor (`placement.rs`: centered, top at 20% of work area, clamped; `to_physical` for
  DPI), then `show` + `set_focus` + emits `lumen:overlay-shown`. `policy.rs`: shortcut decision
  (hidden→Show, visible+unfocused→Focus, visible+focused→Hide).
- `shortcut.rs`: Alt+Space via `tauri-plugin-global-shortcut` 2.4.0; registration failure is logged
  and reflected in the tray tooltip, never fatal. Configurable shortcut + conflict UX is T003.
- `tray.rs`: tray icon (default window icon), left click = show, menu Show Lumen / Quit Lumen.
- `main.rs`: single-instance plugin first (second launch → show); `WindowEvent::Focused(false)` →
  hide; `CloseRequested` → prevent + hide; `ShowWhenReady` state: first show only after the UI calls
  `overlay_ready` (no blank frame, listener guaranteed); `--background` skips the first show.
- `tauri.conf.json` window: `visible:false, decorations:false, resizable:false, alwaysOnTop:true,
  skipTaskbar:true, shadow:true`, 800×64. Capabilities unchanged (`core:default`).
- UI: `features/root-search/SearchField.tsx` (labelled `type=search` input, role=search);
  `App.tsx` focuses/selects on mount and on `lumen:overlay-shown`, Escape → `hide_overlay` unless
  composing (`isComposing` or keyCode 229). IPC wrappers in `src/ipc/{commands,events}.ts`.

## Validation (Linux sandbox)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace     # core 32 + 3 doc, shell 7, xtask 8
cargo xtask arch           # OK
cd apps/desktop && npm run check   # 10 tests
cd apps/desktop && npx tauri build --debug --no-bundle  # + Xvfb/openbox smoke run
```

Xvfb smoke results: window at (560,216) for a 1920×1080 work area ✓; typing reaches the input ✓;
Escape hides ✓; Alt+Space owned by openbox → logged, app continued ✓. Not verifiable on Linux:
foreground/focus rules, tray, single instance (needs D-Bus), exact 64px height (WebKitGTK min 200px).

## Windows checklist (to close T002)

`cd apps\desktop; npm run tauri dev` (or `npm run tauri build` → `target\release\lumen.exe`):

1. Launch → overlay appears once, centered high on the monitor with the cursor, caret in the field,
   no white flash; no taskbar button; tray icon present.
2. Alt+Space hides; Alt+Space shows again with focus and previous text selected (type replaces it).
3. Click another window → overlay hides. Escape hides. Alt+F4 hides (app keeps running in tray).
4. With focus in another app (e.g. Explorer), Alt+Space → overlay gets keyboard focus immediately
   (no taskbar flashing). Repeat 10×.
5. Multi-monitor / mixed DPI (100% + 150%): show on each monitor; size looks identical.
6. IME (e.g. Japanese/Chinese): while composing, Escape cancels composition, not the overlay.
7. Launch a second `lumen.exe` → no second tray icon; the existing overlay shows.
8. Tray: left click shows; menu Quit exits the process.
9. If PowerToys Run/another launcher owns Alt+Space: tooltip reads "Alt+Space unavailable"; tray works.

Report failures with the step number. Known risk: step 7 focus — Windows may refuse foreground
to the first instance (taskbar flash); fix would be `AllowSetForegroundWindow` in a Windows adapter.

## Exact next steps

1. Run the Windows checklist; fix regressions; mark T002 DONE.
2. T012 (WebView lifecycle/RAM while hidden) and T004 (Mica/Acrylic, rounding) now unblocked;
   T003 (configurable shortcut + conflict UX) builds on `shortcut.rs`.
3. Parallel-safe: T007 SQLite/FTS, T008 USearch bench, T009 file identity, T010 CI, T005 embedding.

## Known issues / notes

- Window height 64 is a placeholder until T103 defines the results layout/resizing.
- Debug builds print `overlay shown in …` (native calls only; first-paint latency needs T010/T012).
- Plain `cargo run` of `lumen-desktop` loads the dev URL; use `npm run tauri dev|build`.
- `ActionRequest.confirmed` trusted from UI (T011 note); capability-derived actions need T108.

## Unresolved evidence-based decisions

- production EmbeddingGemma runtime (T006); native backdrop path (T004); vector scalar profile
  (T008); FastFrame/egui spike timing (TX01); TS binding generation (ADR-013 revisit).
