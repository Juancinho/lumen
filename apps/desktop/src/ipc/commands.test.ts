import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";

import {
  getAppearance,
  getCoreInfo,
  hideOverlay,
  listActions,
  overlayPainted,
  overlayReady,
  previewResult,
  previewPdfPage,
  cancelPdfPreview,
  resizeOverlay,
  runAction,
  search,
} from "./commands";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("ipc commands", () => {
  it("getCoreInfo invokes core_info and returns its payload", async () => {
    const payload = { productName: "Lumen", version: "0.1.0" };
    vi.mocked(invoke).mockResolvedValueOnce(payload);

    await expect(getCoreInfo()).resolves.toEqual(payload);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("core_info");
  });

  it.each([
    ["hide_overlay", hideOverlay],
    ["overlay_ready", overlayReady],
  ] as const)("invokes %s", async (command, fn) => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await fn();
    expect(invoke).toHaveBeenCalledExactlyOnceWith(command);
  });

  it("overlayPainted sends the show sequence number", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await overlayPainted(3);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("overlay_painted", { seq: 3 });
  });

  it("getAppearance invokes overlay_appearance and normalizes the payload", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ material: "mica", corners: "round" });
    await expect(getAppearance()).resolves.toEqual({ material: "mica", corners: "round" });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("overlay_appearance");

    vi.mocked(invoke).mockResolvedValueOnce({ material: "glass" });
    await expect(getAppearance()).resolves.toEqual({ material: "solid", corners: "square" });
  });

  it("resizeOverlay sends the logical height and returns the applied one", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ width: 800, height: 400 });
    await expect(resizeOverlay(800, 497)).resolves.toEqual({ width: 800, height: 400 });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("resize_overlay", { width: 800, height: 497 });
  });

  it("search sends the query id and text", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(true);
    await expect(search(7, "notas")).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("search", { queryId: 7, text: "notas" });
  });

  it("listActions and runAction send ids only", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await listActions(2, "item:5");
    expect(invoke).toHaveBeenLastCalledWith("list_actions", { queryId: 2, resultId: "item:5" });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await runAction(2, "item:5", "lumen.open", "primary");
    expect(invoke).toHaveBeenLastCalledWith("run_action", {
      queryId: 2,
      resultId: "item:5",
      actionId: "lumen.open",
      invocation: "primary",
    });
  });

  it("previewResult sends ids", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({});
    await previewResult(3, "item:9");
    expect(invoke).toHaveBeenLastCalledWith("preview_result", { queryId: 3, resultId: "item:9" });
  });

  it("PDF preview sends only result ids, bounded page metadata and a cancellation id", async () => {
    await previewPdfPage(11, 3, "item:9", 7);
    expect(invoke).toHaveBeenLastCalledWith("preview_pdf_page", {
      requestId: 11,
      queryId: 3,
      resultId: "item:9",
      pageNumber: 7,
    });
    await cancelPdfPreview(11);
    expect(invoke).toHaveBeenLastCalledWith("cancel_pdf_preview", { requestId: 11 });
  });
});
