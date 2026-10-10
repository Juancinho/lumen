import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { formatSize } from "./format";
import { PreviewPane } from "./PreviewPane";

describe("PreviewPane", () => {
  it("labels indexed image text and distinct OCR coverage without changing preview keys", () => {
    const close = vi.fn();
    const data = {
      title: "capture.png",
      kind: "image" as const,
      location: null,
      sizeBytes: null,
      modifiedMs: null,
      text: "ERROR 42\ncontraseña",
      truncated: false,
      imageOcr: { state: "indexed" as const, language: "es-ES", reason: null },
    };
    const { rerender } = render(<PreviewPane docked data={data} onClose={close} />);
    expect(screen.getByRole("status")).toHaveTextContent("Image text indexed · es-ES");
    expect(screen.getByLabelText("Text from image")).toHaveTextContent("ERROR 42");
    fireEvent.keyDown(screen.getByRole("complementary"), { key: "Enter", altKey: true });
    expect(close).toHaveBeenCalledOnce();
    for (const [state, label] of [
      ["empty", "no text detected"],
      ["pending", "Waiting"],
      ["off", "off for this image"],
      ["unavailable", "unavailable"],
      ["failed", "will retry"],
      ["skipped", "skipped"],
    ] as const) {
      rerender(
        <PreviewPane
          docked
          data={{ ...data, text: null, imageOcr: { state, language: null, reason: null } }}
        />,
      );
      expect(screen.getByRole("status")).toHaveTextContent(label);
      expect(screen.queryByLabelText("Text from image")).not.toBeInTheDocument();
    }
  });
  it("shows visual coverage and orientation on the same keyboard preview surface", () => {
    const close = vi.fn();
    const data = {
      title: "0001.jpg",
      kind: "image" as const,
      location: null,
      sizeBytes: 1000,
      modifiedMs: null,
      text: null,
      truncated: false,
      image: {
        width: 1080,
        height: 1920,
        orientation: 6,
        format: "JPEG",
        visualState: "pending" as const,
      },
    };
    const { rerender } = render(<PreviewPane docked data={data} onClose={close} />);
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for visual indexing");
    expect(screen.getByText(/1080 × 1920/)).toHaveTextContent("EXIF orientation 6");
    fireEvent.keyDown(screen.getByLabelText("Preview of 0001.jpg"), { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
    rerender(
      <PreviewPane docked data={{ ...data, image: { ...data.image, visualState: "indexed" } }} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Visual meaning indexed");
    rerender(
      <PreviewPane
        docked
        data={{
          ...data,
          image: {
            width: null,
            height: null,
            orientation: null,
            format: null,
            visualState: "skipped",
            reason: "image:unsupported",
          },
        }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("This image format is not supported");
    expect(screen.queryByText(/1080 × 1920/)).not.toBeInTheDocument();
  });
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
