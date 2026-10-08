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

/** Mirrors `ResultDto`: one result as the UI renders it (T107). */
export interface ResultView {
  id: string;
  kind: "application" | "file" | "folder" | "command";
  title: string;
  detail: string | null;
  extension: string | null;
  /** Action id Enter runs (`lumen.open`, `lumen.launch`). */
  primaryAction: string;
  /** Development diagnostics (`LUMEN_DIAGNOSTICS=1`), otherwise `null`. */
  diagnostics: ResultDiagnostics | null;
}

/** Mirrors `ResultDiagnosticsDto` (T110). */
export interface ResultDiagnostics {
  provider: string;
  matchKind: string;
  confidence: number;
}

/** Mirrors `QueryDiagnosticsDto` (T110). */
export interface QueryDiagnostics {
  elapsedMs: number;
  failed: string[];
}

/** Mirrors `ActionDto`: one Action Panel entry (T108). */
export interface ActionView {
  id: string;
  title: string;
  group: "primary" | "common" | "navigation" | "advanced" | "destructive";
  /** Keyboard hint such as `Enter` or `Ctrl+Enter`. */
  shortcut: string | null;
}

/** How an action was triggered (mirrors `actions::parse_invocation`). */
export type Invocation = "primary" | "panel" | "shortcut";

/** Mirrors `ResultsDto` (event `lumen:results`): merged results of query `queryId` so far. */
export interface ResultsUpdate {
  queryId: number;
  /** No further update follows for this query. */
  done: boolean;
  results: ResultView[];
  /** Diagnostics mode only, otherwise `null`. */
  diagnostics: QueryDiagnostics | null;
}
