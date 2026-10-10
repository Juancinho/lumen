import { invoke } from "@tauri-apps/api/core";

import { toAppearance, toPreview } from "./events";
import type {
  ActionView,
  Appearance,
  CoreInfo,
  Invocation,
  Preview,
  PdfPreview,
  Size,
} from "./types";

/** Identity of the linked Rust core. Command: `core_info`. */
export function getCoreInfo(): Promise<CoreInfo> {
  return invoke<CoreInfo>("core_info");
}

/** Hides the overlay window (kept alive for instant re-show). Command: `hide_overlay`. */
export function hideOverlay(): Promise<void> {
  return invoke("hide_overlay");
}

/**
 * Tells the shell the UI has rendered; the first show waits for this to avoid a blank
 * first frame. Idempotent. Command: `overlay_ready`.
 */
export function overlayReady(): Promise<void> {
  return invoke("overlay_ready");
}

/**
 * Timing diagnostics: the frame after show `seq` was painted. Only sent when the shell
 * provided a `seq` (diagnostics on). Command: `overlay_painted`.
 */
export function overlayPainted(seq: number): Promise<void> {
  return invoke("overlay_painted", { seq });
}

/**
 * The surface to paint (window material and corners). Call before `overlayReady` so the
 * first frame uses it; changes arrive as `lumen:appearance`. Command: `overlay_appearance`.
 */
export async function getAppearance(): Promise<Appearance> {
  return toAppearance(await invoke<unknown>("overlay_appearance"));
}

/**
 * Asks the shell to size the overlay (logical px; top edge fixed, left edge kept unless a
 * wider window would overflow). Returns the size applied after clamping to the monitor.
 * Command: `resize_overlay`.
 */
export function resizeOverlay(width: number, height: number): Promise<Size> {
  return invoke<Size>("resize_overlay", { width, height });
}

/** Quick Look data for a result of query `queryId`. Command: `preview_result`. */
export async function previewResult(queryId: number, resultId: string): Promise<Preview> {
  return toPreview(await invoke<unknown>("preview_result", { queryId, resultId }));
}

export function previewPdfPage(
  requestId: number,
  queryId: number,
  resultId: string,
  pageNumber: number,
): Promise<PdfPreview> {
  return invoke<PdfPreview>("preview_pdf_page", { requestId, queryId, resultId, pageNumber });
}

export function cancelPdfPreview(requestId: number): Promise<void> {
  return invoke("cancel_pdf_preview", { requestId });
}

/**
 * Starts root search `queryId` (increasing per query) for `text`. Results arrive as
 * `lumen:results`; resolves to whether the shell accepted it. Command: `search`.
 */
export function search(queryId: number, text: string): Promise<boolean> {
  return invoke<boolean>("search", { queryId, text });
}

/** Action Panel entries for a result of query `queryId`. Command: `list_actions`. */
export function listActions(queryId: number, resultId: string): Promise<ActionView[]> {
  return invoke<ActionView[]>("list_actions", { queryId, resultId });
}

/**
 * Runs an action on a result the user saw; the shell checks it is offered, executes it,
 * learns from it and hides the overlay. Returns true for an authorized local preview
 * fallback, which keeps the overlay open. Rejects with a short reason. Command: `run_action`.
 */
export function runAction(
  queryId: number,
  resultId: string,
  actionId: string,
  invocation: Invocation,
): Promise<boolean> {
  return invoke("run_action", { queryId, resultId, actionId, invocation });
}
