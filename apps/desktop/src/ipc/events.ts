import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `overlay::EVENT_SHOWN` in `src-tauri/src/overlay/mod.rs`. */
export const OVERLAY_SHOWN = "lumen:overlay-shown";

export type { UnlistenFn };

/** Fires every time the overlay is shown or re-focused by the shell. */
export function onOverlayShown(handler: () => void): Promise<UnlistenFn> {
  return listen(OVERLAY_SHOWN, () => {
    handler();
  });
}
