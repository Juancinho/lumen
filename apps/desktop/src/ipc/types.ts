// Wire types for the Rust shell boundary. Each type mirrors a DTO in
// `src-tauri/src/dto.rs`; the Rust side has a test guarding the JSON shape.
// Keep this file free of runtime code.

/** Mirrors `CoreInfoDto`. */
export interface CoreInfo {
  productName: string;
  version: string;
}
