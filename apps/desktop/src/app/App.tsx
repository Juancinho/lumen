import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { commandFor } from "../features/root-search/keymap";
import {
  COMPACT_WIDTH,
  DOCKED_MIN_WIDTH,
  MAX_VISIBLE_ROWS,
  overlaySize,
  rootSearchHeight,
} from "../features/root-search/layout";
import { PreviewPane } from "../features/root-search/PreviewPane";
import { RootSearch } from "../features/root-search/RootSearch";
import {
  INITIAL_SELECTION,
  moveSelection,
  stabilize,
  selectedIndex,
  selectIndex,
} from "../features/root-search/selection";
import { REVEAL_ACTION, useActions } from "../features/root-search/useActions";
import { usePreview } from "../features/root-search/usePreview";
import { useResults } from "../features/root-search/useResults";
import {
  getAppearance,
  hideOverlay,
  onAppearanceChanged,
  onOverlayShown,
  overlayPainted,
  overlayReady,
  resizeOverlay,
  type Invocation,
} from "../ipc";
import { subscribe } from "../lib/subscribe";
import { applyAppearance } from "./appearance";

function reportIpcError(action: string) {
  return (error: unknown) => {
    console.error(`lumen: ${action} failed`, error);
  };
}

/**
 * Diagnostics: report after the next painted frame (double rAF: the first callback runs
 * before that frame is painted, the second after it was presented).
 */
function reportPainted(seq: number) {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      overlayPainted(seq).catch(reportIpcError("overlay_painted"));
    });
  });
}

/**
 * Overlay entrance (DESIGN_SYSTEM §11): the content fades in; the window and its system
 * backdrop appear natively, so nothing scales. Skipped under reduced motion.
 */
function playEntrance(element: HTMLElement | null) {
  if (!element || typeof element.animate !== "function") return;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  element.animate([{ opacity: 0 }, { opacity: 1 }], {
    duration: 150,
    easing: "cubic-bezier(0.2, 0, 0, 1)",
  });
}

/** Focus the query and select it, so typing replaces the previous query. */
function focusQuery(input: HTMLInputElement | null) {
  input?.focus();
  input?.select();
}

/**
 * Overlay root (T002): one search surface, keyboard-first.
 * - focus is placed in the query on mount and every time the shell shows the overlay;
 * - Escape dismisses (ignored while an IME composition is active);
 * - the window material (T004) is applied before the UI reports ready, so the first
 *   frame already paints the right surface.
 * - the window height follows the content (T103): the shell resizes it, top edge fixed.
 * - keyboard (T104): arrows/PageUp/PageDown move a selection that stays on its result
 *   while results stream in. Text-editing keys stay with the query field.
 * - actions (T108/T109): Enter / click runs the primary action, Ctrl+Enter reveals,
 *   Ctrl+K opens the Action Panel (arrows + Enter inside, Escape/Ctrl+K close it).
 * - Quick Look (T105): Alt+Enter toggles a preview that follows the selection; the window
 *   widens to the right for it (or the preview covers the list on narrow monitors).
 *   Escape closes the panel, then the preview, then the overlay.
 */
export function App() {
  const [query, setQuery] = useState("");
  const [selection, setSelection] = useState(INITIAL_SELECTION);
  const inputRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const answer = useResults(query);
  // A refinement never moves the selected row once the user navigated (T206).
  const rows = useMemo(() => stabilize(answer.rows, selection), [answer.rows, selection]);
  const results = useMemo(() => ({ ...answer, rows }), [answer, rows]);
  const actions = useActions();
  const height = rootSearchHeight(query, results, actions.panel?.actions.length ?? 0);
  const selected = selectedIndex(selection, results.rows);
  const selectedRow = results.rows[selected];
  const preview = usePreview(results.queryId, selectedRow);
  const want = overlaySize(height, preview.open);
  const [applied, setApplied] = useState({ width: COMPACT_WIDTH, height: want.height });

  useEffect(() => {
    resizeOverlay(want.width, want.height).then(setApplied, reportIpcError("resize_overlay"));
  }, [want.width, want.height]);

  useEffect(() => {
    focusQuery(inputRef.current);
    // The first show waits for `overlay_ready`, so paint the right surface before it.
    getAppearance()
      .then((appearance) => {
        applyAppearance(appearance);
      }, reportIpcError("overlay_appearance"))
      .finally(() => {
        overlayReady().catch(reportIpcError("overlay_ready"));
      });
  }, []);

  useEffect(
    () =>
      subscribe(
        () =>
          onOverlayShown(({ seq }) => {
            focusQuery(inputRef.current);
            playEntrance(contentRef.current);
            if (seq !== null) reportPainted(seq);
          }),
        reportIpcError("listen overlay-shown"),
      ),
    [],
  );

  useEffect(
    () =>
      subscribe(
        () =>
          onAppearanceChanged((appearance) => {
            applyAppearance(appearance);
          }),
        reportIpcError("listen appearance"),
      ),
    [],
  );

  const changeQuery = (next: string) => {
    setQuery(next);
    setSelection(INITIAL_SELECTION);
    actions.closePanel();
    actions.clearNotice();
  };

  const select = (index: number) => {
    setSelection(selectIndex(results.rows, index));
    actions.clearNotice();
  };

  const runOn = (row: typeof selectedRow, actionId: string | undefined, how: Invocation) => {
    if (row && actionId && results.queryId !== null) {
      actions.run(results.queryId, row, actionId, how);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const command = commandFor({
      key: event.key,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      shiftKey: event.shiftKey,
      metaKey: event.metaKey,
      isComposing: event.nativeEvent.isComposing,
      // eslint-disable-next-line @typescript-eslint/no-deprecated -- intentional IME guard
      keyCode: event.keyCode,
    });
    if (!command) return;
    event.preventDefault();
    if (actions.panel) {
      switch (command.type) {
        case "move":
          actions.movePanel(command.delta);
          return;
        case "primary":
          actions.runPanel();
          return;
        case "dismiss":
        case "actions":
          actions.closePanel();
          return;
        case "details":
          actions.closePanel();
          break;
        default:
          actions.closePanel();
      }
    }
    switch (command.type) {
      case "dismiss":
        if (preview.open) preview.close();
        else hideOverlay().catch(reportIpcError("hide_overlay"));
        return;
      case "move":
        setSelection(moveSelection(selection, results.rows, command.delta));
        actions.clearNotice();
        return;
      case "page":
        setSelection(moveSelection(selection, results.rows, command.direction * MAX_VISIBLE_ROWS));
        actions.clearNotice();
        return;
      case "focusQuery":
        focusQuery(inputRef.current);
        return;
      case "primary":
        runOn(selectedRow, selectedRow?.primaryAction, "primary");
        return;
      case "reveal":
        runOn(selectedRow, REVEAL_ACTION, "shortcut");
        return;
      case "actions":
        if (selectedRow && results.queryId !== null) {
          actions.openPanel(results.queryId, selectedRow);
        }
        return;
      case "details":
        if (preview.open || selectedRow) preview.toggle();
        return;
    }
  };

  return (
    <main className="overlay">
      <div className="overlay__content" ref={contentRef}>
        <RootSearch
          query={query}
          onQueryChange={changeQuery}
          inputRef={inputRef}
          results={results}
          selectedIndex={selected}
          onSelect={select}
          onActivate={(index) => {
            select(index);
            const row = results.rows[index];
            runOn(row, row?.primaryAction, "primary");
          }}
          onKeyDown={onKeyDown}
          notice={actions.noticeFor === selectedRow?.id ? actions.notice : null}
          preview={
            preview.open && (
              <PreviewPane data={preview.data} docked={applied.width >= DOCKED_MIN_WIDTH} />
            )
          }
          panel={
            actions.panel && {
              subject: actions.panel.row.title,
              actions: actions.panel.actions,
              selectedIndex: actions.panel.index,
              onSelect: actions.selectPanel,
              onRun: actions.runPanel,
            }
          }
        />
      </div>
    </main>
  );
}
