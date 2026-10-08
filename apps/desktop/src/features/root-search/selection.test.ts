import { describe, expect, it } from "vitest";

import type { ResultRowModel } from "./model";
import { INITIAL_SELECTION, moveSelection, selectedIndex, selectIndex } from "./selection";

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
});
