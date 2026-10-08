import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { Appearance } from "./types";

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

/** Mirrors `material::EVENT_APPEARANCE` in `src-tauri/src/material.rs`. */
export const APPEARANCE_CHANGED = "lumen:appearance";

/** Validates an `Appearance` payload; anything unexpected becomes the opaque surface. */
export function toAppearance(payload: unknown): Appearance {
  const p = payload as { material?: unknown; corners?: unknown } | null;
  const material =
    p?.material === "acrylic" || p?.material === "mica" || p?.material === "solid"
      ? p.material
      : "solid";
  const corners = p?.corners === "round" ? "round" : "square";
  return { material, corners };
}

/** Fires when the shell switches the window material (tray choice or system setting). */
export function onAppearanceChanged(
  handler: (appearance: Appearance) => void,
): Promise<UnlistenFn> {
  return listen(APPEARANCE_CHANGED, (event) => {
    handler(toAppearance(event.payload));
  });
}
