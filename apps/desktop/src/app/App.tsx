import { useEffect, useRef, useState } from "react";

import { SearchField } from "../features/root-search/SearchField";
import { hideOverlay, onOverlayShown, overlayReady } from "../ipc";

function reportIpcError(action: string) {
  return (error: unknown) => {
    console.error(`lumen: ${action} failed`, error);
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
 * - Escape dismisses (ignored while an IME composition is active).
 * Results (T101+) and the premium surface (T103) build on this.
 */
export function App() {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    focusQuery(inputRef.current);
    overlayReady().catch(reportIpcError("overlay_ready"));
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    onOverlayShown(() => {
      focusQuery(inputRef.current);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    }, reportIpcError("listen overlay-shown"));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

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
