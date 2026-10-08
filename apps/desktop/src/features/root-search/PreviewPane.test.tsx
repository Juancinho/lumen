import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { formatSize } from "./format";
import { PreviewPane } from "./PreviewPane";

describe("PreviewPane", () => {
  it("formats sizes", () => {
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(25 * 1024 * 1024)).toBe("25 MB");
  });

  it("shows metadata and says when there is no excerpt", () => {
    render(
      <PreviewPane
        docked
        data={{
          title: "foto.jpg",
          kind: "file",
          location: "C:\\Users\\Joao\\Pictures",
          sizeBytes: 3_000_000,
          modifiedMs: null,
          text: null,
          truncated: false,
        }}
      />,
    );
    expect(screen.getByRole("complementary", { name: "Preview of foto.jpg" })).toBeInTheDocument();
    expect(screen.getByText("2.9 MB")).toBeInTheDocument();
    expect(screen.getByText("No preview for this type of file")).toBeInTheDocument();
  });

  it("marks a truncated excerpt and a loading state", () => {
    const { rerender } = render(<PreviewPane docked={false} data={null} />);
    expect(screen.getByRole("complementary", { name: "Preview" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    rerender(
      <PreviewPane
        docked={false}
        data={{
          title: "log.txt",
          kind: "file",
          location: null,
          sizeBytes: 10,
          modifiedMs: null,
          text: "line",
          truncated: true,
        }}
      />,
    );
    expect(screen.getByText(/line\s*…/)).toBeInTheDocument();
  });
});
