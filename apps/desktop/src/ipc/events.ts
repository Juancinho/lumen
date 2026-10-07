import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `overlay::EVENT_SHOWN` in `src-tauri/src/overlay/mod.rs`. */
export const OVERLAY_SHOWN = "lumen:overlay-shown";

export type { UnlistenFn };

/** Mirrors `overlay::ShownPayload`. `seq` is set only when shell timing diagnostics are on. */
export interface OverlayShown {
  seq: number | null;
}

function toShown(payload: unknown): OverlayShown {
  const seq = (payload as { seq?: unknown } | null)?.seq;
  return { seq: typeof seq === "number" ? seq : null };
}

/** Fires every time the overlay is shown or re-focused by the shell. */
export function onOverlayShown(handler: (shown: OverlayShown) => void): Promise<UnlistenFn> {
  return listen(OVERLAY_SHOWN, (event) => {
    handler(toShown(event.payload));
  });
}
