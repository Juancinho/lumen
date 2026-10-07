// The only module UI code may import to reach the native shell (enforced by ESLint).
export { getCoreInfo, hideOverlay, overlayPainted, overlayReady } from "./commands";
export { onOverlayShown, OVERLAY_SHOWN, type OverlayShown, type UnlistenFn } from "./events";
export type { CoreInfo } from "./types";
