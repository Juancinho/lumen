import { listen } from "@tauri-apps/api/event";
import { describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_CHANGED,
  onAppearanceChanged,
  onOverlayShown,
  OVERLAY_SHOWN,
  toAppearance,
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
