import { useEffect, useRef, useState } from "react";

import { SearchField } from "../features/root-search/SearchField";
import {
  getAppearance,
  hideOverlay,
  onAppearanceChanged,
  onOverlayShown,
  overlayPainted,
  overlayReady,
  type UnlistenFn,
} from "../ipc";
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

/** Subscribes in an effect; unsubscribes on cleanup even if the listener resolves late. */
function subscribe(start: () => Promise<UnlistenFn>, what: string) {
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  start().then(
    (fn) => {
      if (disposed) fn();
      else unlisten = fn;
    },
    reportIpcError(`listen ${what}`),
  );
  return () => {
    disposed = true;
    unlisten?.();
  };
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
 * Results (T101+) and the premium surface (T103) build on this.
 */
export function App() {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

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
            if (seq !== null) reportPainted(seq);
          }),
        "overlay-shown",
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
        "appearance",
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

  return (
    <main className="overlay">
      <SearchField value={query} onChange={setQuery} inputRef={inputRef} />
    </main>
  );
}
