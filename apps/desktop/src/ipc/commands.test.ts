import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";

import { getCoreInfo } from "./commands";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("getCoreInfo", () => {
  it("invokes the core_info command and returns its payload", async () => {
    const payload = { productName: "Lumen", version: "0.1.0" };
    vi.mocked(invoke).mockResolvedValueOnce(payload);

    await expect(getCoreInfo()).resolves.toEqual(payload);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("core_info");
  });
});
