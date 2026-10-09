import { listen } from "@tauri-apps/api/event";
import { describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_CHANGED,
  onAppearanceChanged,
  onOverlayShown,
  OVERLAY_SHOWN,
  toAppearance,
  toResultsUpdate,
} from "./events";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

describe("onOverlayShown", () => {
  it("listens to the shell event and passes a normalized payload", async () => {
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValueOnce(unlisten);
    const handler = vi.fn();

    await expect(onOverlayShown(handler)).resolves.toBe(unlisten);
    expect(OVERLAY_SHOWN).toBe("lumen:overlay-shown");
    expect(listen).toHaveBeenCalledWith(OVERLAY_SHOWN, expect.any(Function));

    const callback = vi.mocked(listen).mock.calls[0]?.[1] as (event: unknown) => void;
    callback({ event: OVERLAY_SHOWN, id: 1, payload: null });
    expect(handler).toHaveBeenLastCalledWith({ seq: null });
    callback({ event: OVERLAY_SHOWN, id: 2, payload: { seq: 7 } });
    expect(handler).toHaveBeenLastCalledWith({ seq: 7 });
    callback({ event: OVERLAY_SHOWN, id: 3, payload: { seq: "7" } });
    expect(handler).toHaveBeenLastCalledWith({ seq: null });
  });
});

describe("appearance", () => {
  it("normalizes payloads, defaulting to the opaque surface", () => {
    expect(toAppearance({ material: "acrylic", corners: "round" })).toEqual({
      material: "acrylic",
      corners: "round",
    });
    expect(toAppearance({ material: "mica", corners: "square" })).toEqual({
      material: "mica",
      corners: "square",
    });
    expect(toAppearance(null)).toEqual({ material: "solid", corners: "square" });
    expect(toAppearance({ material: "blur", corners: 1 })).toEqual({
      material: "solid",
      corners: "square",
    });
  });

  it("listens to the shell event", async () => {
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValueOnce(unlisten);
    const handler = vi.fn();

    await expect(onAppearanceChanged(handler)).resolves.toBe(unlisten);
    expect(APPEARANCE_CHANGED).toBe("lumen:appearance");
    const callback = vi.mocked(listen).mock.calls[0]?.[1] as (event: unknown) => void;
    callback({ event: APPEARANCE_CHANGED, id: 1, payload: { material: "mica", corners: "round" } });
    expect(handler).toHaveBeenLastCalledWith({ material: "mica", corners: "round" });
  });
});

describe("results", () => {
  it("keeps typed PDF/code context through the actual event decoder", () => {
    const update = toResultsUpdate({
      queryId: 7,
      done: true,
      results: [
        {
          id: "item:1",
          kind: "pdf-page",
          title: "guide.pdf",
          pdf: { pageNumber: 7, path: "untrusted" },
        },
        {
          id: "item:2",
          kind: "code",
          title: "client.py",
          code: { symbol: "retry", language: "python", repository: "lumen", path: "untrusted" },
        },
        { id: "item:3", kind: "file", title: "exact.pdf", pdf: { pageNumber: 3 } },
      ],
    });
    expect(update?.results[0]).toMatchObject({ kind: "pdf-page", pdf: { pageNumber: 7 } });
    expect(update?.results[0]?.pdf).toEqual({ pageNumber: 7 });
    expect(update?.results[1]).toMatchObject({
      kind: "code",
      code: { symbol: "retry", language: "python", repository: "lumen" },
    });
    expect(update?.results[1]?.code).toEqual({
      symbol: "retry",
      language: "python",
      repository: "lumen",
    });
    expect(update?.results[2]).toMatchObject({ kind: "file", pdf: { pageNumber: 3 } });
  });
  it("rejects invalid physical PDF page metadata", () => {
    for (const pageNumber of [0, -1, 1.5, "7", NaN, Infinity, 0x100000000, null]) {
      const update = toResultsUpdate({
        queryId: 7,
        results: [{ id: "item:1", kind: "pdf-page", title: "guide.pdf", pdf: { pageNumber } }],
      });
      expect(update?.results[0]?.pdf).toBeUndefined();
    }
  });
  it("validates updates and drops malformed rows", () => {
    expect(toResultsUpdate(null)).toBeNull();
    expect(toResultsUpdate({ queryId: "1", results: [] })).toBeNull();
    expect(
      toResultsUpdate({
        queryId: 4,
        done: true,
        results: [
          { id: "item:1", kind: "application", title: "Calculator", detail: "Application" },
          { id: 2, title: "bad" },
          { id: "item:3", kind: "weird", title: "x.txt", detail: null, extension: "txt" },
        ],
      }),
    ).toEqual({
      queryId: 4,
      done: true,
      results: [
        {
          id: "item:1",
          kind: "application",
          title: "Calculator",
          detail: "Application",
          snippet: null,
          extension: null,
          primaryAction: "",
          diagnostics: null,
        },
        {
          id: "item:3",
          kind: "file",
          title: "x.txt",
          detail: null,
          snippet: null,
          extension: "txt",
          primaryAction: "",
          diagnostics: null,
        },
      ],
      diagnostics: null,
    });
  });
});

describe("diagnostics", () => {
  it("passes development diagnostics through when present", () => {
    const u = toResultsUpdate({
      queryId: 1,
      done: true,
      results: [
        {
          id: "item:1",
          kind: "file",
          title: "a",
          diagnostics: { provider: "lumen.catalog", matchKind: "prefix", confidence: 0.5 },
        },
      ],
      diagnostics: { elapsedMs: 1.5, failed: ["x.y"] },
    });
    expect(u?.results[0]?.diagnostics).toEqual({
      provider: "lumen.catalog",
      matchKind: "prefix",
      confidence: 0.5,
    });
    expect(u?.diagnostics).toEqual({ elapsedMs: 1.5, failed: ["x.y"] });
  });
});
