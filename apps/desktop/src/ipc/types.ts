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
  kind: "application" | "file" | "folder" | "command" | "code" | "pdf-page" | "image";
  title: string;
  detail: string | null;
  /** Passage that matched when found by contents or meaning (T206), else `null`. */
  snippet: string | null;
  extension: string | null;
  code?: CodeContext | null;
  pdf?: { pageNumber: number } | null;
  image?: ImageContext | null;
  /** Action id Enter runs (`lumen.open`, `lumen.launch`). */
  primaryAction: string;
  /** Development diagnostics (`LUMEN_DIAGNOSTICS=1`), otherwise `null`. */
  diagnostics: ResultDiagnostics | null;
}

/** Mirrors CodeContextDto: display labels, never executor payloads. */
export interface CodeContext {
  symbol: string | null;
  language: string;
  repository: string | null;
}

/** Local display metadata only; pixel buffers, hashes and action paths stay in Rust. */
export interface ImageContext {
  width: number | null;
  height: number | null;
  orientation: number | null;
  format: string | null;
  reason?: string | null;
  visualState: "not-indexed" | "skipped" | "pending" | "indexed" | "failed";
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

/** Mirrors `SizeDto`: logical px. */
export interface Size {
  width: number;
  height: number;
}

/** Mirrors `PreviewDto` (Quick Look, T105). */
export interface Preview {
  title: string;
  kind: "application" | "file" | "folder" | "command" | "code" | "pdf-page" | "image";
  location: string | null;
  sizeBytes: number | null;
  modifiedMs: number | null;
  /** Start of a text file, or the indexed passage for a code match. */
  text: string | null;
  truncated: boolean;
  pageNumber?: number | null;
  image?: ImageContext | null;
  imageOcr?: {
    state: "off" | "pending" | "indexed" | "empty" | "skipped" | "failed" | "unavailable";
    language: string | null;
    reason: string | null;
  } | null;
}

export interface PdfPreview {
  pageNumber: number;
  pageCount: number | null;
  width: number | null;
  height: number | null;
  image: string | null;
  unavailable: string | null;
}
