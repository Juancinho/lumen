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
export function listState(query: string, results: ResultsState): ListState {
  if (results.rows.length > 0) return { kind: "rows", count: results.rows.length };
  if (query.trim() !== "" && results.status === "done") return { kind: "message" };
  return { kind: "none" };
}

/** Window height (logical px) the root search needs. */
export function rootSearchHeight(query: string, results: ResultsState): number {
  return overlayHeight(listState(query, results));
}
