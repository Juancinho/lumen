import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { getCoreInfo } from "../ipc";
import { App } from "./App";

vi.mock("../ipc", () => ({ getCoreInfo: vi.fn() }));

describe("App", () => {
  it("shows a status while connecting, then the core version", async () => {
    vi.mocked(getCoreInfo).mockResolvedValueOnce({ productName: "Lumen", version: "0.1.0" });

    render(<App />);

    expect(screen.getByRole("heading", { name: "Lumen" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Connecting to core…");
    expect(await screen.findByText("Core 0.1.0")).toBeInTheDocument();
  });

  it("reports an unavailable core as an alert instead of failing silently", async () => {
    vi.mocked(getCoreInfo).mockRejectedValueOnce(new Error("ipc down"));

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Core unavailable: Error: ipc down");
  });
});
