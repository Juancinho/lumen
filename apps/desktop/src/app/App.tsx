import { useEffect, useRef, useState } from "react";

import { rootSearchHeight } from "../features/root-search/layout";
import { RootSearch } from "../features/root-search/RootSearch";
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
 * Result data arrives in T107 (`useResults`); actions in T109.
 */
export function App() {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const results = useResults(query);
  const height = rootSearchHeight(query, results);

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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // keyCode 229: some IMEs signal composition only this way (no isComposing).
      // eslint-disable-next-line @typescript-eslint/no-deprecated -- intentional IME guard
      const imeProcessing = event.keyCode === 229;
      if (event.key !== "Escape" || event.isComposing || imeProcessing) return;
      event.preventDefault();
      hideOverlay().catch(reportIpcError("hide_overlay"));
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const changeQuery = (next: string) => {
    setQuery(next);
    setSelectedIndex(0);
  };

  return (
    <main className="overlay">
      <div className="overlay__content" ref={contentRef}>
        <RootSearch
          query={query}
          onQueryChange={changeQuery}
          inputRef={inputRef}
          results={results}
          selectedIndex={Math.min(selectedIndex, Math.max(results.rows.length - 1, 0))}
          onSelect={setSelectedIndex}
          onActivate={setSelectedIndex /* the primary action is T109 */}
        />
      </div>
    </main>
  );
}
