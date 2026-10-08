/**
 * What a result row shows (T103). UI-side view model: T107 maps the shell's result DTOs
 * into it, tests and previews build it directly. Never carries scores (DESIGN_SYSTEM §10).
 */
export type ResultKind = "application" | "file" | "folder" | "command";

export interface ResultRowModel {
  /** Stable result id (`item:42`); React key and selection identity. */
  id: string;
  kind: ResultKind;
  title: string;
  /** Location (folder path) or short description; middle-truncated when it is a path. */
  detail: string | null;
  /** Lowercase file extension without the dot, for the file glyph badge. */
  extension: string | null;
  /** Action id Enter runs. */
  primaryAction: string;
  /** Development diagnostics line (T110), e.g. `lumen.catalog · prefix · 0.82`. */
  diagnostics?: string | null;
}

/** Right-hand label of a row (low-emphasis, never colourful). */
export function kindLabel(row: Pick<ResultRowModel, "kind" | "extension">): string {
  switch (row.kind) {
    case "application":
      return "Application";
    case "folder":
      return "Folder";
    case "command":
      return "Command";
    case "file":
      return row.extension ? row.extension.toUpperCase() : "File";
  }
}

/**
 * Splits a path for middle truncation: `head` may be ellipsized by CSS, `tail` (the last
 * folder, or the last two when the last is very short, with its leading separator) stays
 * visible, so a cut reads `C:\Users\Jo…\lumen`.
 * `C:\Users\Joao\Proyectos\lumen` → head `C:\Users\Joao\Proyectos`, tail `\lumen`.
 */
export function splitPath(path: string): { head: string; tail: string } {
  const sep = /[\\/]/;
  const parts = path.split(sep);
  if (parts.length < 2) return { head: "", tail: path };
  let keep = 1;
  const last = parts[parts.length - 1] ?? "";
  if (last.length < 4 && parts.length > 2) keep = 2;
  // Find where the kept tail begins in the original string (separators may be mixed).
  let cut = path.length;
  for (let seen = 0; cut > 0; cut--) {
    if (sep.test(path.charAt(cut - 1))) {
      seen++;
      if (seen === keep) break;
    }
  }
  // Leave the separator with the tail; a path that is only a root stays whole.
  if (cut <= 1 || cut === path.length) return { head: "", tail: path };
  return { head: path.slice(0, cut - 1), tail: path.slice(cut - 1) };
}

/** True when `detail` looks like a filesystem path (and so is middle-truncated). */
export function isPath(detail: string): boolean {
  return /^([a-z]:[\\/]|\\\\|\/|~[\\/])/i.test(detail);
}

/** Id of the result listbox (combobox `aria-controls`). */
export const RESULT_LIST_ID = "lumen-results";

/** DOM id of the row at `index` (for `aria-activedescendant`). */
export function rowDomId(index: number): string {
  return `${RESULT_LIST_ID}-${String(index)}`;
}

/** Id of the Action Panel listbox. */
export const ACTION_LIST_ID = "lumen-actions";

/** DOM id of action `index` (for the combobox's `aria-activedescendant`). */
export function actionDomId(index: number): string {
  return `${ACTION_LIST_ID}-${String(index)}`;
}
