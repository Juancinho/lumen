/**
 * Overlay height from its content (T103). Deterministic, so the window is resized once per
 * result change instead of following layout measurements. Values mirror
 * `src/design/tokens.css` (layout.test.ts checks they match).
 */
import type { ResultsState } from "./useResults";

export const SEARCH_HEIGHT = 64;
export const ROW_HEIGHT = 52;
export const LIST_PADDING = 6;
/** Divider between the search bar and the list. */
export const DIVIDER = 1;
/** Rows visible before the list scrolls (DESIGN_SYSTEM §3: 6–8). */
export const MAX_VISIBLE_ROWS = 8;
/** Height of a one-line status/empty message area. */
export const MESSAGE_HEIGHT = 56;
/** Activity footer, shared with --index-status-height. */
export const INDEX_STATUS_HEIGHT = 80;

export type ListState = { kind: "none" } | { kind: "rows"; count: number } | { kind: "message" };

/** Logical window height for `state` (before the shell clamps it to the monitor). */
export function overlayHeight(state: ListState): number {
  switch (state.kind) {
    case "none":
      return SEARCH_HEIGHT;
    case "message":
      return SEARCH_HEIGHT + DIVIDER + MESSAGE_HEIGHT;
    case "rows": {
      if (state.count <= 0) return SEARCH_HEIGHT;
      const rows = Math.min(state.count, MAX_VISIBLE_ROWS);
      return SEARCH_HEIGHT + DIVIDER + 2 * LIST_PADDING + rows * ROW_HEIGHT;
    }
  }
}

/** What the list area shows for a query and its results. */
export function listState(
  query: string,
  results: Pick<ResultsState, "rows" | "status">,
): ListState {
  if (results.rows.length > 0) return { kind: "rows", count: results.rows.length };
  if (query.trim() !== "" && results.status === "done") return { kind: "message" };
  return { kind: "none" };
}

/** Window height (logical px) the root search needs. */
export function rootSearchHeight(
  query: string,
  results: Pick<ResultsState, "rows" | "status">,
  panelActions = 0,
): number {
  const list = overlayHeight(listState(query, results));
  if (panelActions <= 0) return list + INDEX_STATUS_HEIGHT;
  return (
    Math.max(list, SEARCH_HEIGHT + DIVIDER + 2 * LIST_PADDING + panelHeight(panelActions)) +
    INDEX_STATUS_HEIGHT
  );
}

/** Action Panel entry height and chrome (mirrors `--action-height`, `--panel-chrome`). */
export const ACTION_HEIGHT = 36;
export const PANEL_CHROME = 40;

/** Height of an Action Panel listing `actions` entries. */
export function panelHeight(actions: number): number {
  return PANEL_CHROME + actions * ACTION_HEIGHT;
}

/** Window width with Quick Look docked beside the list (mirrors `--preview-width`). */
export const COMPACT_WIDTH = 800;
export const PREVIEW_WIDTH = 400;
/** Below this applied width the preview covers the list instead of docking beside it. */
export const DOCKED_MIN_WIDTH = COMPACT_WIDTH + 300;
/** Height the preview needs to be useful (search bar + divider + body). */
export const PREVIEW_MIN_HEIGHT = SEARCH_HEIGHT + DIVIDER + 360;

/** Window size to request: wider and at least `PREVIEW_MIN_HEIGHT` tall with a preview. */
export function overlaySize(height: number, preview: boolean): { width: number; height: number } {
  if (!preview) return { width: COMPACT_WIDTH, height };
  return { width: COMPACT_WIDTH + PREVIEW_WIDTH, height: Math.max(height, PREVIEW_MIN_HEIGHT) };
}
