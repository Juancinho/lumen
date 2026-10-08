import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Appearance,
  QueryDiagnostics,
  ResultDiagnostics,
  ResultsUpdate,
  ResultView,
} from "./types";

/** Mirrors `overlay::EVENT_SHOWN` in `src-tauri/src/overlay/mod.rs`. */
export const OVERLAY_SHOWN = "lumen:overlay-shown";

export type { UnlistenFn };

/** Mirrors `overlay::ShownPayload`. `seq` is set only when shell timing diagnostics are on. */
export interface OverlayShown {
  seq: number | null;
}

function toShown(payload: unknown): OverlayShown {
  const seq = (payload as { seq?: unknown } | null)?.seq;
  return { seq: typeof seq === "number" ? seq : null };
}

/** Fires every time the overlay is shown or re-focused by the shell. */
export function onOverlayShown(handler: (shown: OverlayShown) => void): Promise<UnlistenFn> {
  return listen(OVERLAY_SHOWN, (event) => {
    handler(toShown(event.payload));
  });
}

/** Mirrors `material::EVENT_APPEARANCE` in `src-tauri/src/material.rs`. */
export const APPEARANCE_CHANGED = "lumen:appearance";

/** Validates an `Appearance` payload; anything unexpected becomes the opaque surface. */
export function toAppearance(payload: unknown): Appearance {
  const p = payload as { material?: unknown; corners?: unknown } | null;
  const material =
    p?.material === "acrylic" || p?.material === "mica" || p?.material === "solid"
      ? p.material
      : "solid";
  const corners = p?.corners === "round" ? "round" : "square";
  return { material, corners };
}

/** Fires when the shell switches the window material (tray choice or system setting). */
export function onAppearanceChanged(
  handler: (appearance: Appearance) => void,
): Promise<UnlistenFn> {
  return listen(APPEARANCE_CHANGED, (event) => {
    handler(toAppearance(event.payload));
  });
}

/** Mirrors `search::EVENT_RESULTS` in `src-tauri/src/search.rs`. */
export const RESULTS = "lumen:results";

/** Mirrors `catalog::EVENT_CHANGED` in `src-tauri/src/catalog.rs`. */
export const CATALOG_CHANGED = "lumen:catalog-changed";

const KINDS = new Set(["application", "file", "folder", "command"]);

function toResultDiagnostics(raw: unknown): ResultDiagnostics | null {
  const d = raw as Partial<Record<keyof ResultDiagnostics, unknown>> | null | undefined;
  if (typeof d?.provider !== "string" || typeof d.matchKind !== "string") return null;
  return {
    provider: d.provider,
    matchKind: d.matchKind,
    confidence: typeof d.confidence === "number" ? d.confidence : 0,
  };
}

function toQueryDiagnostics(raw: unknown): QueryDiagnostics | null {
  const d = raw as { elapsedMs?: unknown; failed?: unknown } | null | undefined;
  if (typeof d?.elapsedMs !== "number") return null;
  const failed = Array.isArray(d.failed)
    ? d.failed.filter((f): f is string => typeof f === "string")
    : [];
  return { elapsedMs: d.elapsedMs, failed };
}

function toResult(raw: unknown): ResultView | null {
  const r = raw as Partial<Record<keyof ResultView, unknown>> | null;
  if (typeof r?.id !== "string" || typeof r.title !== "string") return null;
  const kind = typeof r.kind === "string" && KINDS.has(r.kind) ? r.kind : "file";
  return {
    id: r.id,
    kind: kind as ResultView["kind"],
    title: r.title,
    detail: typeof r.detail === "string" ? r.detail : null,
    extension: typeof r.extension === "string" ? r.extension : null,
    primaryAction: typeof r.primaryAction === "string" ? r.primaryAction : "",
    diagnostics: toResultDiagnostics(r.diagnostics),
  };
}

/** Validates a `lumen:results` payload; `null` when it is not one. */
export function toResultsUpdate(payload: unknown): ResultsUpdate | null {
  const p = payload as {
    queryId?: unknown;
    done?: unknown;
    results?: unknown;
    diagnostics?: unknown;
  } | null;
  if (typeof p?.queryId !== "number" || !Array.isArray(p.results)) return null;
  return {
    queryId: p.queryId,
    done: p.done === true,
    results: p.results.map(toResult).filter((r): r is ResultView => r !== null),
    diagnostics: toQueryDiagnostics(p.diagnostics),
  };
}

/** Fires for every result update the shell streams (any query; filter by `queryId`). */
export function onResults(handler: (update: ResultsUpdate) => void): Promise<UnlistenFn> {
  return listen(RESULTS, (event) => {
    const update = toResultsUpdate(event.payload);
    if (update) handler(update);
  });
}

/** Fires when a catalog sync changed what can be found. */
export function onCatalogChanged(handler: () => void): Promise<UnlistenFn> {
  return listen(CATALOG_CHANGED, () => {
    handler();
  });
}
