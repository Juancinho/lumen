import { invoke } from "@tauri-apps/api/core";

import type { CoreInfo } from "./types";

/** Identity of the linked Rust core. Command: `core_info`. */
export function getCoreInfo(): Promise<CoreInfo> {
  return invoke<CoreInfo>("core_info");
}
