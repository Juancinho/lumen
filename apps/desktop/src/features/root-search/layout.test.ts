import { describe, expect, it } from "vitest";

import tokens from "../../design/tokens.css?raw";
import {
  LIST_PADDING,
  MAX_VISIBLE_ROWS,
  MESSAGE_HEIGHT,
  overlayHeight,
  ROW_HEIGHT,
  SEARCH_HEIGHT,
} from "./layout";

function token(name: string): number {
  const m = new RegExp(`${name}:\\s*(\\d+)px;`).exec(tokens);
  if (!m?.[1]) throw new Error(`token ${name} missing`);
  return Number(m[1]);
}

describe("overlay layout", () => {
  it("mirrors the CSS geometry tokens", () => {
    expect(token("--search-height")).toBe(SEARCH_HEIGHT);
    expect(token("--row-height")).toBe(ROW_HEIGHT);
    expect(token("--list-padding")).toBe(LIST_PADDING);
    expect(token("--message-height")).toBe(MESSAGE_HEIGHT);
  });

  it("is the bare search bar without a list", () => {
    expect(overlayHeight({ kind: "none" })).toBe(64);
    expect(overlayHeight({ kind: "rows", count: 0 })).toBe(64);
  });

  it("grows by one row height per result up to the visible maximum", () => {
    expect(overlayHeight({ kind: "rows", count: 1 })).toBe(64 + 1 + 12 + 52);
    expect(overlayHeight({ kind: "rows", count: 3 })).toBe(64 + 1 + 12 + 3 * 52);
    const max = overlayHeight({ kind: "rows", count: MAX_VISIBLE_ROWS });
    expect(overlayHeight({ kind: "rows", count: 50 })).toBe(max);
    expect(max).toBeLessThanOrEqual(520);
  });

  it("fits the no-results message", () => {
    expect(overlayHeight({ kind: "message" })).toBe(64 + 1 + 56);
  });
});
