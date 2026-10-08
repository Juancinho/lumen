// Wire types for the Rust shell boundary. Each type mirrors a DTO in
// `src-tauri/src/dto.rs`; the Rust side has a test guarding the JSON shape.
// Keep this file free of runtime code.

/** Mirrors `CoreInfoDto`. */
export interface CoreInfo {
  productName: string;
  version: string;
}

/** Mirrors `AppearanceDto`: the window surface the UI paints over (T004). */
export interface Appearance {
  /** `acrylic`/`mica`: a system backdrop shows through a tint; `solid`: paint opaque. */
  material: "acrylic" | "mica" | "solid";
  /** `round`: Windows rounds and shadows the window; `square`: Windows 10 style. */
  corners: "round" | "square";
}
