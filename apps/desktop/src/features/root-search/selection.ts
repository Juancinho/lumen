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

/**
 * Rows to show for a refined answer (T206, DESIGN_SYSTEM "Semantic refinement"): once the
 * user has moved the selection, the selected result keeps its position on screen and the
 * other rows flow around it, so a late content/meaning update never moves the row under
 * the cursor. Before that, the refined order is shown as it is.
 */
export function stabilize(
  rows: readonly ResultRowModel[],
  sel: Selection,
): readonly ResultRowModel[] {
  if (!sel.moved || sel.id === null || rows.length === 0) return rows;
  const at = rows.findIndex((r) => r.id === sel.id);
  const target = Math.min(Math.max(sel.index, 0), rows.length - 1);
  const row = rows[at];
  if (at < 0 || at === target || row === undefined) return rows;
  const out = rows.filter((r) => r.id !== sel.id);
  out.splice(target, 0, row);
  return out;
}
