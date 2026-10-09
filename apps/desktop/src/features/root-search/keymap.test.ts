import { describe, expect, it } from "vitest";

import { commandFor, type KeyInput } from "./keymap";

const key = (k: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  key: k,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  isComposing: false,
  keyCode: 0,
  ...mods,
});

describe("keymap", () => {
  it("maps the launcher keys", () => {
    expect(commandFor(key("ArrowDown"))).toEqual({ type: "move", delta: 1 });
    expect(commandFor(key("ArrowUp"))).toEqual({ type: "move", delta: -1 });
    expect(commandFor(key("PageDown"))).toEqual({ type: "page", direction: 1 });
    expect(commandFor(key("PageDown", { altKey: true }))).toEqual({
      type: "previewPage",
      direction: 1,
    });
    expect(commandFor(key("PageUp", { altKey: true }))).toEqual({
      type: "previewPage",
      direction: -1,
    });
    expect(commandFor(key("Enter"))).toEqual({ type: "primary" });
    expect(commandFor(key("Enter", { ctrlKey: true }))).toEqual({ type: "reveal" });
    expect(commandFor(key("Enter", { altKey: true }))).toEqual({ type: "details" });
    expect(commandFor(key("k", { ctrlKey: true }))).toEqual({ type: "actions" });
    expect(commandFor(key("L", { ctrlKey: true }))).toEqual({ type: "focusQuery" });
    expect(commandFor(key("Escape"))).toEqual({ type: "dismiss" });
  });

  it("leaves text editing and IME composition alone", () => {
    for (const k of ["Home", "End", "a", "ArrowLeft", "Backspace", "Tab"]) {
      expect(commandFor(key(k))).toBeNull();
    }
    expect(commandFor(key("ArrowDown", { shiftKey: true }))).toBeNull();
    expect(commandFor(key("a", { ctrlKey: true }))).toBeNull();
    expect(commandFor(key("Enter", { isComposing: true }))).toBeNull();
    expect(commandFor(key("Escape", { keyCode: 229 }))).toBeNull();
    expect(commandFor(key("Enter", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });
});
