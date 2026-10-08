import type { ResultRowModel } from "./model";

/**
 * Selection that stays on the same result while results stream in (T104,
 * DESIGN_SYSTEM "Semantic refinement"): until the user moves it, the top row is selected;
 * once moved, it follows that result's id, and only falls back to the same index (clamped)
 * when the result disappeared.
 */
export interface Selection {
  /** The user moved the selection (arrows, hover) since the query changed. */
  moved: boolean;
  id: string | null;
  index: number;
}

export const INITIAL_SELECTION: Selection = { moved: false, id: null, index: 0 };

/** Index of the selected row in `rows` (0 when there are none). */
export function selectedIndex(sel: Selection, rows: readonly ResultRowModel[]): number {
  if (rows.length === 0 || !sel.moved) return 0;
  const byId = sel.id === null ? -1 : rows.findIndex((r) => r.id === sel.id);
  if (byId >= 0) return byId;
  return Math.min(Math.max(sel.index, 0), rows.length - 1);
}

/** Selection on row `index` (clamped; no wrap-around). */
export function selectIndex(rows: readonly ResultRowModel[], index: number): Selection {
  if (rows.length === 0) return INITIAL_SELECTION;
  const i = Math.min(Math.max(index, 0), rows.length - 1);
  return { moved: true, id: rows[i]?.id ?? null, index: i };
}

/** Selection moved by `delta` rows from the current one. */
export function moveSelection(
  sel: Selection,
  rows: readonly ResultRowModel[],
  delta: number,
): Selection {
  return selectIndex(rows, selectedIndex(sel, rows) + delta);
}
