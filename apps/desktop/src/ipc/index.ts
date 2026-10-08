// The only module UI code may import to reach the native shell (enforced by ESLint).
export {
  getAppearance,
  getCoreInfo,
  hideOverlay,
  listActions,
  overlayPainted,
  overlayReady,
  resizeOverlay,
  runAction,
  search,
} from "./commands";
export {
  APPEARANCE_CHANGED,
  CATALOG_CHANGED,
  onAppearanceChanged,
  onCatalogChanged,
  onOverlayShown,
  onResults,
  OVERLAY_SHOWN,
  RESULTS,
  toAppearance,
  toResultsUpdate,
  type OverlayShown,
  type UnlistenFn,
} from "./events";
export type {
  ActionView,
  Appearance,
  CoreInfo,
  Invocation,
  ResultsUpdate,
  ResultView,
} from "./types";
