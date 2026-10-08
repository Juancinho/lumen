/**
 * Root-search keyboard model (DESIGN_SYSTEM §12, T104). Pure: maps a key event to a
 * command; the overlay decides what each command does. Text-editing keys (Home/End,
 * Ctrl+A/C/V/X/Z, Shift+arrows, Ctrl+arrows) are never claimed: the query field owns them.
 */
export type KeyCommand =
  | { type: "dismiss" }
  | { type: "move"; delta: number }
  | { type: "page"; direction: 1 | -1 }
  | { type: "primary" }
  | { type: "reveal" }
  | { type: "details" }
  | { type: "actions" }
  | { type: "focusQuery" };

export interface KeyInput {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  isComposing: boolean;
  /** Deprecated but needed: some IMEs signal composition only as keyCode 229. */
  keyCode: number;
}

export function commandFor(e: KeyInput): KeyCommand | null {
  if (e.isComposing || e.keyCode === 229) return null;
  const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
  const ctrlOnly = e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
  const altOnly = e.altKey && !e.ctrlKey && !e.shiftKey && !e.metaKey;
  switch (e.key) {
    case "Escape":
      return plain ? { type: "dismiss" } : null;
    case "ArrowDown":
      return plain ? { type: "move", delta: 1 } : null;
    case "ArrowUp":
      return plain ? { type: "move", delta: -1 } : null;
    case "PageDown":
      return plain ? { type: "page", direction: 1 } : null;
    case "PageUp":
      return plain ? { type: "page", direction: -1 } : null;
    case "Enter":
      if (plain) return { type: "primary" };
      if (ctrlOnly) return { type: "reveal" };
      if (altOnly) return { type: "details" };
      return null;
    case "k":
    case "K":
      return ctrlOnly ? { type: "actions" } : null;
    case "l":
    case "L":
      return ctrlOnly ? { type: "focusQuery" } : null;
    default:
      return null;
  }
}
