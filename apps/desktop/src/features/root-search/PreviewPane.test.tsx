import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { formatSize } from "./format";
import { PreviewPane } from "./PreviewPane";

describe("PreviewPane", () => {
  it("keeps indexed text on rendering failure and Escape works from PDF controls", () => {
    const close = vi.fn();
    render(
      <PreviewPane
        docked
        data={{
          title: "guide.pdf",
          kind: "pdf-page",
          location: null,
          sizeBytes: null,
          modifiedMs: null,
          text: "matched text",
          truncated: true,
          pageNumber: 7,
        }}
        pdf={{
          pageNumber: 7,
          pageCount: null,
          width: null,
          height: null,
          image: null,
          unavailable: "Page preview is unavailable",
        }}
        onClose={close}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Page preview is unavailable");
    expect(screen.getByText(/matched text/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next PDF page" })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("button", { name: "Next PDF page" }), {
      key: "Escape",
      isComposing: true,
    });
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("button", { name: "Next PDF page" }), { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
  });
  it("labels the physical PDF page beside its indexed text passage", () => {
    render(
      <PreviewPane
        docked
        data={{
          title: "guide.pdf",
          kind: "pdf-page",
          location: null,
          sizeBytes: 1000,
          modifiedMs: null,
          text: "Protect coastal habitat",
          truncated: true,
          pageNumber: 7,
        }}
      />,
    );
    expect(screen.getByRole("complementary")).toHaveTextContent("Page 7");
    expect(screen.getByText(/Protect coastal habitat/)).toBeInTheDocument();
  });
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
