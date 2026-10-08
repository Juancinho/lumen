import { invoke } from "@tauri-apps/api/core";

import { toAppearance } from "./events";
import type { Appearance, CoreInfo } from "./types";

/** Identity of the linked Rust core. Command: `core_info`. */
export function getCoreInfo(): Promise<CoreInfo> {
  return invoke<CoreInfo>("core_info");
}

/** Hides the overlay window (kept alive for instant re-show). Command: `hide_overlay`. */
export function hideOverlay(): Promise<void> {
  return invoke("hide_overlay");
}

/**
 * Tells the shell the UI has rendered; the first show waits for this to avoid a blank
 * first frame. Idempotent. Command: `overlay_ready`.
 */
export function overlayReady(): Promise<void> {
  return invoke("overlay_ready");
}

/**
 * Timing diagnostics: the frame after show `seq` was painted. Only sent when the shell
 * provided a `seq` (diagnostics on). Command: `overlay_painted`.
 */
export function overlayPainted(seq: number): Promise<void> {
  return invoke("overlay_painted", { seq });
}

/**
 * The surface to paint (window material and corners). Call before `overlayReady` so the
 * first frame uses it; changes arrive as `lumen:appearance`. Command: `overlay_appearance`.
 */
export async function getAppearance(): Promise<Appearance> {
  return toAppearance(await invoke<unknown>("overlay_appearance"));
}
