import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";

import { getCoreInfo, hideOverlay, overlayPainted, overlayReady } from "./commands";

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
});
