// The only module UI code may import to reach the native shell (enforced by ESLint).
export {
  getAppearance,
  getCoreInfo,
  hideOverlay,
  overlayPainted,
  overlayReady,
  resizeOverlay,
} from "./commands";
export {
  APPEARANCE_CHANGED,
  onAppearanceChanged,
  onOverlayShown,
  OVERLAY_SHOWN,
  toAppearance,
  type OverlayShown,
  type UnlistenFn,
} from "./events";
export type { Appearance, CoreInfo } from "./types";
