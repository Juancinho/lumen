import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { commandFor } from "../features/root-search/keymap";
import { MAX_VISIBLE_ROWS, rootSearchHeight } from "../features/root-search/layout";
import { RootSearch } from "../features/root-search/RootSearch";
import {
  INITIAL_SELECTION,
  moveSelection,
  selectedIndex,
  selectIndex,
} from "../features/root-search/selection";
import { useResults } from "../features/root-search/useResults";
import {
  getAppearance,
  hideOverlay,
  onAppearanceChanged,
  onOverlayShown,
  overlayPainted,
  overlayReady,
  resizeOverlay,
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
 *   while results stream in; Enter/Ctrl+Enter/Alt+Enter/Ctrl+K are claimed for the
 *   actions (T108/T109). Text-editing keys stay with the query field.
 */
export function App() {
  const [query, setQuery] = useState("");
  const [selection, setSelection] = useState(INITIAL_SELECTION);
  const inputRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const results = useResults(query);
  const height = rootSearchHeight(query, results);
  const selected = selectedIndex(selection, results.rows);

  useEffect(() => {
    resizeOverlay(height).catch(reportIpcError("resize_overlay"));
  }, [height]);

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
    switch (command.type) {
      case "dismiss":
        hideOverlay().catch(reportIpcError("hide_overlay"));
        return;
      case "move":
        setSelection(moveSelection(selection, results.rows, command.delta));
        return;
      case "page":
        setSelection(moveSelection(selection, results.rows, command.direction * MAX_VISIBLE_ROWS));
        return;
      case "focusQuery":
        focusQuery(inputRef.current);
        return;
      case "primary":
      case "reveal":
      case "details":
      case "actions":
        // Claimed now so they never type into the field; executed by T108/T109.
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
          onSelect={(index) => {
            setSelection(selectIndex(results.rows, index));
          }}
          onActivate={(index) => {
            setSelection(selectIndex(results.rows, index)); // primary action: T109
          }}
          onKeyDown={onKeyDown}
        />
      </div>
    </main>
  );
}
