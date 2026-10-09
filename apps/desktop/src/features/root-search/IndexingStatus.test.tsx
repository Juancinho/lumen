import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { IndexingStatus } from "./IndexingStatus";
import { toIndexProgress } from "../../ipc/progress";

describe("index coverage", () => {
  it("separates files read from vectors ready, with bounded accessible percentages", () => {
    render(
      <IndexingStatus
        status={toIndexProgress({
          known: true,
          phase: "Embedding images",
          device: "GPU",
          filesTotal: 10,
          filesRead: 6,
          passagesTotal: 100,
          passagesReady: 25,
          imagesTotal: 4,
          imagesReady: 1,
          imagesPending: 3,
          filesSkipped: 2,
          filesFailed: 1,
        })}
      />,
    );
    expect(screen.getByText("Embedding images")).toBeInTheDocument();
    expect(screen.getByText("GPU")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Files read" })).toHaveAttribute("value", "60");
    expect(screen.getByRole("progressbar", { name: "Vectors ready" })).toHaveAttribute(
      "value",
      "25",
    );
    expect(screen.getByText(/1 visual \/ 4 files/)).toBeInTheDocument();
    expect(screen.getByText(/2 skipped · 1 errors/)).toBeInTheDocument();
  });
  it("does not invent a percentage before inventory counts are known", () => {
    render(<IndexingStatus status={null} />);
    expect(screen.getByText(/Counting files/)).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    const invalid = toIndexProgress({
      filesTotal: -1,
      filesRead: NaN,
      device: "unknown",
      path: "private",
    });
    expect(invalid.filesTotal).toBe(0);
    expect(invalid.filesRead).toBe(0);
    expect(invalid.device).toBe("");
    expect(invalid).not.toHaveProperty("path");
  });
});
