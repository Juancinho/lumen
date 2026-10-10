import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Appearance,
  QueryDiagnostics,
  ResultDiagnostics,
  ResultsUpdate,
  ResultView,
  Preview,
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

export function onOverlayHidden(handler: () => void): Promise<UnlistenFn> {
  return listen("lumen:overlay-hidden", handler);
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

const KINDS = new Set(["application", "file", "folder", "command", "code", "pdf-page", "image"]);

/** Bounded display-only preview; reject malformed OCR status/text at the IPC boundary. */
export function toPreview(payload: unknown): Preview {
  const p = payload as Partial<Record<keyof Preview, unknown>> | null;
  const nullableText = (value: unknown): value is string | null =>
    value === null || typeof value === "string";
  const nullableNumber = (value: unknown): value is number | null =>
    value === null || (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
  if (
    !p ||
    typeof p.title !== "string" ||
    typeof p.kind !== "string" ||
    !KINDS.has(p.kind) ||
    !nullableText(p.location) ||
    !nullableText(p.text) ||
    !nullableNumber(p.sizeBytes) ||
    !nullableNumber(p.modifiedMs) ||
    typeof p.truncated !== "boolean" ||
    (p.pageNumber !== undefined &&
      p.pageNumber !== null &&
      (typeof p.pageNumber !== "number" ||
        !Number.isInteger(p.pageNumber) ||
        p.pageNumber < 1 ||
        p.pageNumber > 512))
  )
    throw new Error("Invalid preview");
  const image = p.image ? toImageContext(p.image) : null;
  if (p.image && !image) throw new Error("Invalid image preview");
  let imageOcr: Preview["imageOcr"] = null;
  if (p.imageOcr !== undefined && p.imageOcr !== null) {
    const o = p.imageOcr as Partial<Record<keyof NonNullable<Preview["imageOcr"]>, unknown>>;
    const states = ["off", "pending", "indexed", "empty", "skipped", "failed", "unavailable"];
    if (
      p.kind !== "image" ||
      typeof o.state !== "string" ||
      !states.includes(o.state) ||
      !nullableText(o.language) ||
      (o.language !== null && !/^[a-zA-Z0-9-]{1,80}$/.test(o.language)) ||
      !nullableText(o.reason) ||
      (o.state === "indexed"
        ? typeof p.text !== "string" || p.text.trim().length === 0
        : p.text !== null) ||
      (typeof p.text === "string" &&
        (p.text.includes("\0") || new TextEncoder().encode(p.text).length > 16 * 1024))
    )
      throw new Error("Invalid image text preview");
    const reasons = [
      "ocr:pixel_limit",
      "ocr:text_limit",
      "ocr:timeout",
      "ocr:recognition",
      "image:unsupported",
      "image:source_limit",
      "image:pixel_limit",
      "image:decode",
      "image:io",
      "image:placeholder",
      "image:changed",
    ];
    imageOcr = {
      state: o.state as NonNullable<Preview["imageOcr"]>["state"],
      language: o.language,
      reason: o.reason !== null && reasons.includes(o.reason) ? o.reason : null,
    };
  }
  return {
    title: p.title,
    kind: p.kind as Preview["kind"],
    location: p.location,
    text: p.text,
    sizeBytes: p.sizeBytes,
    modifiedMs: p.modifiedMs,
    truncated: p.truncated,
    pageNumber: p.pageNumber ?? null,
    image: image ?? null,
    imageOcr,
  };
}

function toImageContext(raw: unknown): ResultView["image"] {
  const image = raw as Partial<Record<keyof NonNullable<ResultView["image"]>, unknown>> | null;
  const dimension = (n: unknown): n is number =>
    typeof n === "number" && Number.isInteger(n) && n >= 1 && n <= 16384;
  if (
    !image ||
    (image.format !== null && typeof image.format !== "string") ||
    (image.width !== null && !dimension(image.width)) ||
    (image.height !== null && !dimension(image.height)) ||
    (image.width === null) !== (image.height === null) ||
    (image.orientation !== null &&
      (typeof image.orientation !== "number" ||
        !Number.isInteger(image.orientation) ||
        image.orientation < 1 ||
        image.orientation > 8)) ||
    (image.visualState !== "pending" &&
      image.visualState !== "indexed" &&
      image.visualState !== "failed" &&
      image.visualState !== "skipped" &&
      image.visualState !== "not-indexed")
  )
    return null;
  return {
    width: image.width,
    height: image.height,
    orientation: image.orientation,
    format: image.format,
    visualState: image.visualState,
    ...(typeof image.reason === "string" &&
    [
      "image:unsupported",
      "image:source_limit",
      "image:pixel_limit",
      "image:decode",
      "image:placeholder",
      "image:io",
    ].includes(image.reason)
      ? { reason: image.reason }
      : {}),
  };
}

function toCodeContext(raw: unknown): ResultView["code"] {
  const code = raw as { symbol?: unknown; language?: unknown; repository?: unknown } | null;
  if (typeof code?.language !== "string") return null;
  return {
    symbol: typeof code.symbol === "string" ? code.symbol : null,
    language: code.language,
    repository: typeof code.repository === "string" ? code.repository : null,
  };
}

function toPdfContext(raw: unknown): ResultView["pdf"] {
  const pdf = raw as { pageNumber?: unknown } | null;
  if (
    typeof pdf?.pageNumber !== "number" ||
    !Number.isInteger(pdf.pageNumber) ||
    pdf.pageNumber < 1 ||
    pdf.pageNumber > 0xffffffff
  )
    return null;
  return { pageNumber: pdf.pageNumber };
}

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
  const code = toCodeContext(r.code);
  const pdf = toPdfContext(r.pdf);
  const image = toImageContext(r.image);
  return {
    id: r.id,
    kind: kind as ResultView["kind"],
    title: r.title,
    detail: typeof r.detail === "string" ? r.detail : null,
    snippet: typeof r.snippet === "string" ? r.snippet : null,
    extension: typeof r.extension === "string" ? r.extension : null,
    ...(code ? { code } : {}),
    ...(pdf ? { pdf } : {}),
    ...(image ? { image } : {}),
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
