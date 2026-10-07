import { invoke } from "@tauri-apps/api/core";

import type { CoreInfo } from "./types";

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
