import { describe, expect, it } from "vitest";

import type { ResultRowModel } from "./model";
import {
  INITIAL_SELECTION,
  moveSelection,
  selectedIndex,
  selectIndex,
  stabilize,
} from "./selection";

const rows = (...ids: string[]): ResultRowModel[] =>
  ids.map((id) => ({
    id,
    kind: "file",
    title: id,
    detail: null,
    extension: null,
    primaryAction: "lumen.open",
  }));

describe("selection", () => {
  it("is the top row until the user moves it", () => {
    expect(selectedIndex(INITIAL_SELECTION, rows("a", "b"))).toBe(0);
    expect(selectedIndex(INITIAL_SELECTION, [])).toBe(0);
  });

  it("moves without wrapping", () => {
    const r = rows("a", "b", "c");
    let sel = moveSelection(INITIAL_SELECTION, r, 1);
    expect(selectedIndex(sel, r)).toBe(1);
    sel = moveSelection(sel, r, 5);
    expect(selectedIndex(sel, r)).toBe(2);
    sel = moveSelection(sel, r, -9);
    expect(selectedIndex(sel, r)).toBe(0);
    expect(moveSelection(INITIAL_SELECTION, [], 1)).toEqual(INITIAL_SELECTION);
  });

  it("follows the selected result when results re-order", () => {
    const sel = selectIndex(rows("a", "b", "c"), 1); // b
    expect(selectedIndex(sel, rows("x", "a", "c", "b"))).toBe(3);
  });

  it("keeps the position when the selected result disappears", () => {
    const sel = selectIndex(rows("a", "b", "c"), 2); // c
    expect(selectedIndex(sel, rows("a", "b"))).toBe(1);
    expect(selectedIndex(sel, rows("x", "y", "z", "w"))).toBe(2);
  });

  it("keeps the selected row in place when a refinement re-orders (T206)", () => {
    const ids = (r: readonly ResultRowModel[]) => r.map((x) => x.id);
    // Not moved yet: the refined order is shown as is.
    expect(ids(stabilize(rows("x", "a", "b"), INITIAL_SELECTION))).toEqual(["x", "a", "b"]);
    // The user went to row 1 (b); meaning results push b down to 3.
    const sel = selectIndex(rows("a", "b", "c"), 1);
    const shown = stabilize(rows("x", "y", "a", "b", "c"), sel);
    expect(ids(shown)).toEqual(["x", "b", "y", "a", "c"]);
    expect(selectedIndex(sel, shown)).toBe(1);
    // Fewer rows than the old position: it goes to the last one; gone: unchanged.
    expect(ids(stabilize(rows("b"), selectIndex(rows("a", "c", "b"), 2)))).toEqual(["b"]);
    expect(ids(stabilize(rows("x", "y"), sel))).toEqual(["x", "y"]);
  });
});
