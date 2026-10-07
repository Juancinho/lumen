// The only module UI code may import to reach the native shell (enforced by ESLint).
export { getCoreInfo } from "./commands";
export type { CoreInfo } from "./types";
