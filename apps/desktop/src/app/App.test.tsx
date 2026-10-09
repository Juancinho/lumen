import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getAppearance,
  hideOverlay,
  listActions,
  runAction,
  onAppearanceChanged,
  onOverlayShown,
  overlayPainted,
  overlayReady,
  previewResult,
  previewPdfPage,
  cancelPdfPreview,
  onOverlayHidden,
  resizeOverlay,
  type Appearance,
  type OverlayShown,
  type Preview,
  type PdfPreview,
} from "../ipc";
import { useResults, type ResultsState } from "../features/root-search/useResults";
import { App } from "./App";

vi.mock("../features/root-search/useResults", () => ({ useResults: vi.fn() }));

vi.mock("../ipc", () => ({
  getAppearance: vi.fn(),
  hideOverlay: vi.fn(),
  listActions: vi.fn(),
  runAction: vi.fn(),
  onAppearanceChanged: vi.fn(),
  onOverlayShown: vi.fn(),
  overlayPainted: vi.fn(),
  overlayReady: vi.fn(),
  previewResult: vi.fn(),
  previewPdfPage: vi.fn(),
  cancelPdfPreview: vi.fn(),
  onOverlayHidden: vi.fn(),
  resizeOverlay: vi.fn(),
}));

let shownHandler: ((shown: OverlayShown) => void) | undefined;
let appearanceHandler: ((appearance: Appearance) => void) | undefined;
let hiddenHandler: (() => void) | undefined;
const unlisten = vi.fn();

function nthOption(index: number): HTMLElement {
  const option = screen.getAllByRole("option")[index];
  if (!option) throw new Error(`no option ${String(index)}`);
  return option;
}

let results: ResultsState = { rows: [], queryId: null, status: "idle" };

beforeEach(() => {
  results = { rows: [], queryId: null, status: "idle" };
  vi.mocked(useResults).mockImplementation(() => results);
  vi.mocked(resizeOverlay).mockImplementation((width, height) =>
    Promise.resolve({ width, height }),
  );
  vi.mocked(previewResult).mockResolvedValue({
    title: "item:1.txt",
    kind: "file",
    location: "C:\\Users\\Joao",
    sizeBytes: 2048,
    modifiedMs: 0,
    text: "hola\nmundo",
    truncated: false,
  });
  vi.mocked(runAction).mockResolvedValue(false);
  vi.mocked(previewPdfPage).mockImplementation((_request, _query, _row, pageNumber) =>
    Promise.resolve({
      pageNumber,
      pageCount: 10,
      width: 742,
      height: 960,
      image: "data:image/png;base64,test",
      unavailable: null,
    }),
  );
  vi.mocked(cancelPdfPreview).mockResolvedValue(undefined);
  hiddenHandler = undefined;
  vi.mocked(onOverlayHidden).mockImplementation((handler) => {
    hiddenHandler = handler;
    return Promise.resolve(unlisten);
  });
  vi.mocked(listActions).mockResolvedValue([
    { id: "lumen.open", title: "Open", group: "primary", shortcut: "Enter" },
    {
      id: "lumen.reveal",
      title: "Reveal in Explorer",
      group: "navigation",
      shortcut: "Ctrl+Enter",
    },
    { id: "lumen.copy-path", title: "Copy path", group: "navigation", shortcut: null },
  ]);
  shownHandler = undefined;
  appearanceHandler = undefined;
  delete document.documentElement.dataset.material;
  delete document.documentElement.dataset.corners;
  vi.mocked(getAppearance).mockResolvedValue({ material: "acrylic", corners: "round" });
  vi.mocked(onAppearanceChanged).mockImplementation((handler) => {
    appearanceHandler = handler;
    return Promise.resolve(unlisten);
  });
  vi.mocked(hideOverlay).mockResolvedValue(undefined);
  vi.mocked(overlayReady).mockResolvedValue(undefined);
  vi.mocked(overlayPainted).mockResolvedValue(undefined);
  vi.mocked(onOverlayShown).mockImplementation((handler) => {
    shownHandler = handler;
    return Promise.resolve(unlisten);
  });
});

describe("App overlay", () => {
  it("renders a labelled search field that owns focus on mount", async () => {
    render(<App />);

    const input = screen.getByRole("combobox", { name: "Search" });
    expect(input).toHaveFocus();
    expect(screen.getByRole("search")).toContainElement(input);
    await act(async () => {
      await Promise.resolve();
    });
    expect(overlayReady).toHaveBeenCalledOnce();
  });

  it("applies the window material before reporting ready", async () => {
    let readyMaterial: string | undefined;
    vi.mocked(overlayReady).mockImplementation(() => {
      readyMaterial = document.documentElement.dataset.material;
      return Promise.resolve();
    });
    render(<App />);
    await act(async () => {
      await Promise.resolve();
    });

    expect(readyMaterial).toBe("acrylic");
    expect(document.documentElement.dataset.corners).toBe("round");

    act(() => {
      appearanceHandler?.({ material: "solid", corners: "round" });
    });
    expect(document.documentElement.dataset.material).toBe("solid");
  });

  it("still reports ready when the appearance query fails", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.mocked(getAppearance).mockRejectedValueOnce(new Error("no shell"));
    render(<App />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(overlayReady).toHaveBeenCalledOnce();
    expect(document.documentElement.dataset.material).toBeUndefined();
  });

  it("Escape hides the overlay", async () => {
    render(<App />);

    await userEvent.keyboard("{Escape}");
    expect(hideOverlay).toHaveBeenCalledOnce();
  });

  it("Escape during IME composition does not hide", () => {
    render(<App />);

    const input = screen.getByRole("combobox");
    fireEvent.keyDown(input, { key: "Escape", isComposing: true });
    fireEvent.keyDown(input, { key: "Escape", keyCode: 229 });
    expect(hideOverlay).not.toHaveBeenCalled();
  });

  it("other keys do not hide", async () => {
    render(<App />);

    await userEvent.keyboard("spotify{Enter}");
    expect(hideOverlay).not.toHaveBeenCalled();
    expect(screen.getByRole("combobox")).toHaveValue("spotify");
  });

  it("re-focuses and selects the previous query when the shell shows the overlay", async () => {
    render(<App />);
    const input = screen.getByRole<HTMLInputElement>("combobox");
    await userEvent.type(input, "notes");
    input.blur();
    await act(async () => {
      await Promise.resolve();
    });

    act(() => {
      shownHandler?.({ seq: null });
    });

    expect(input).toHaveFocus();
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe("notes".length);
  });

  it("stops listening on unmount", async () => {
    const { unmount } = render(<App />);
    await act(async () => {
      await Promise.resolve();
    });

    unmount();
    expect(unlisten).toHaveBeenCalled();
  });

  it("reports the painted frame only when the shell asks for timing", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame"] });
    try {
      render(<App />);
      await act(async () => {
        await Promise.resolve();
      });

      act(() => {
        shownHandler?.({ seq: null });
        vi.advanceTimersToNextFrame();
        vi.advanceTimersToNextFrame();
      });
      expect(overlayPainted).not.toHaveBeenCalled();

      act(() => {
        shownHandler?.({ seq: 4 });
        vi.advanceTimersToNextFrame();
      });
      expect(overlayPainted).not.toHaveBeenCalled(); // not before the frame is painted
      act(() => {
        vi.advanceTimersToNextFrame();
      });
      expect(overlayPainted).toHaveBeenCalledExactlyOnceWith(4);
    } finally {
      vi.useRealTimers();
    }
  });

  it("sizes the window to the content and resets the selection on a new query", async () => {
    const { rerender } = render(<App />);
    expect(resizeOverlay).toHaveBeenLastCalledWith(800, 64);

    results = {
      rows: [
        {
          id: "item:1",
          kind: "application",
          title: "Calculator",
          detail: null,
          extension: null,
          primaryAction: "lumen.open",
        },
        {
          id: "item:2",
          kind: "file",
          title: "calc.xlsx",
          detail: null,
          extension: "xlsx",
          primaryAction: "lumen.open",
        },
      ],
      status: "done",
      queryId: 1,
    };
    rerender(<App />);
    expect(resizeOverlay).toHaveBeenLastCalledWith(800, 64 + 1 + 12 + 2 * 52);

    await userEvent.hover(nthOption(1));
    expect(screen.getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    await userEvent.type(screen.getByRole("combobox"), "c");
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
  });

  it("arrow keys move a selection that stays on its result while results stream", async () => {
    const r = (id: string) => ({
      id,
      kind: "file" as const,
      title: id,
      detail: null,
      extension: null,
      primaryAction: "lumen.open",
    });
    results = { rows: [r("a"), r("b"), r("c")], status: "searching", queryId: 1 };
    const { rerender } = render(<App />);
    const input = screen.getByRole("combobox");
    await userEvent.keyboard("{ArrowDown}");
    expect(nthOption(1)).toHaveAttribute("aria-selected", "true");
    expect(input).toHaveAttribute("aria-activedescendant", nthOption(1).id);

    // A later update re-orders: "b" stays selected and keeps its row (T206); the other
    // rows flow around it.
    results = { rows: [r("x"), r("a"), r("c"), r("b")], status: "done", queryId: 1 };
    rerender(<App />);
    expect(nthOption(1)).toHaveAttribute("aria-selected", "true");
    expect(nthOption(1)).toHaveTextContent("b");
    expect(nthOption(0)).toHaveTextContent("x");

    await userEvent.keyboard("{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}{ArrowUp}");
    expect(nthOption(0)).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{PageDown}");
    expect(nthOption(3)).toHaveAttribute("aria-selected", "true");
    // Enter is claimed (no text typed), the query is untouched.
    await userEvent.keyboard("{Enter}");
    expect(input).toHaveValue("");
  });

  describe("actions", () => {
    const file = (id: string) => ({
      id,
      kind: "file" as const,
      title: `${id}.txt`,
      detail: null,
      extension: "txt",
      primaryAction: "lumen.open",
    });

    beforeEach(() => {
      results = { rows: [file("item:1"), file("item:2")], status: "done", queryId: 7 };
    });

    it("Enter runs the selected result's primary action, Ctrl+Enter reveals", async () => {
      render(<App />);
      await userEvent.keyboard("{ArrowDown}{Enter}");
      expect(runAction).toHaveBeenLastCalledWith(7, "item:2", "lumen.open", "primary");
      await userEvent.keyboard("{Control>}{Enter}{/Control}");
      expect(runAction).toHaveBeenLastCalledWith(7, "item:2", "lumen.reveal", "shortcut");
    });

    it("clicking a row runs it", async () => {
      render(<App />);
      await userEvent.click(nthOption(1));
      expect(runAction).toHaveBeenLastCalledWith(7, "item:2", "lumen.open", "primary");
    });

    it("Ctrl+K opens the Action Panel; arrows + Enter run an action from it", async () => {
      render(<App />);
      await userEvent.keyboard("{Control>}k{/Control}");
      const panel = await screen.findByRole("listbox", { name: "Actions" });
      expect(listActions).toHaveBeenCalledWith(7, "item:1");
      expect(screen.getByRole("region", { name: "Actions for item:1.txt" })).toContainElement(
        panel,
      );
      const input = screen.getByRole("combobox");
      expect(input).toHaveAttribute("aria-controls", panel.id);
      await userEvent.keyboard("{ArrowDown}{ArrowDown}");
      const options = screen.getAllByRole("option", { name: /Copy path/ });
      expect(options[0]).toHaveAttribute("aria-selected", "true");
      await userEvent.keyboard("{Enter}");
      expect(runAction).toHaveBeenLastCalledWith(7, "item:1", "lumen.copy-path", "panel");
      expect(screen.queryByRole("listbox", { name: "Actions" })).not.toBeInTheDocument();
    });

    it("excludes the selected file type by keyboard using only trusted result/action ids", async () => {
      results = {
        rows: [{ ...file("item:1"), title: "app.js", extension: "js" }, file("item:2")],
        status: "done",
        queryId: 7,
      };
      vi.mocked(listActions).mockResolvedValue([
        { id: "lumen.open", title: "Open", group: "primary", shortcut: "Enter" },
        {
          id: "lumen.exclude-file",
          title: "Exclude this file from Lumen",
          group: "advanced",
          shortcut: null,
        },
        {
          id: "lumen.exclude-extension",
          title: "Exclude all .js files",
          group: "advanced",
          shortcut: null,
        },
      ]);
      render(<App />);
      await userEvent.keyboard("{Control>}k{/Control}");
      await screen.findByRole("listbox", { name: "Actions" });
      await userEvent.keyboard("{ArrowDown}{ArrowDown}");
      expect(screen.getByRole("option", { name: "Exclude all .js files" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      await userEvent.keyboard("{Enter}");
      expect(runAction).toHaveBeenLastCalledWith(7, "item:1", "lumen.exclude-extension", "panel");
      expect(screen.queryByRole("listbox", { name: "Actions" })).not.toBeInTheDocument();
    });

    it("Escape closes the panel first, then dismisses", async () => {
      render(<App />);
      await userEvent.keyboard("{Control>}k{/Control}");
      await screen.findByRole("listbox", { name: "Actions" });
      await userEvent.keyboard("{Escape}");
      expect(screen.queryByRole("listbox", { name: "Actions" })).not.toBeInTheDocument();
      expect(hideOverlay).not.toHaveBeenCalled();
      await userEvent.keyboard("{Escape}");
      expect(hideOverlay).toHaveBeenCalledOnce();
    });

    it("a failed action leaves a short notice on the row", async () => {
      vi.spyOn(console, "error").mockImplementation(() => undefined);
      vi.mocked(runAction).mockRejectedValueOnce("result has no local path");
      render(<App />);
      await userEvent.keyboard("{Enter}");
      expect(await screen.findByRole("status")).toHaveTextContent("Couldn't do that");
      await userEvent.keyboard("{ArrowDown}");
      expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });
  });

  describe("quick look", () => {
    const file = (id: string) => ({
      id,
      kind: "file" as const,
      title: `${id}.txt`,
      detail: null,
      extension: "txt",
      primaryAction: "lumen.open",
    });

    beforeEach(() => {
      results = { rows: [file("item:1"), file("item:2")], status: "done", queryId: 7 };
    });

    it("Alt+Enter toggles a docked preview that follows the selection", async () => {
      render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(
        await screen.findByRole("complementary", { name: "Preview of item:1.txt" }),
      ).toHaveClass("preview--docked");
      expect(screen.getByText(/hola/)).toBeInTheDocument();
      expect(screen.getByText(/2\.0 KB/)).toBeInTheDocument();
      expect(resizeOverlay).toHaveBeenLastCalledWith(1200, 64 + 1 + 360);
      expect(previewResult).toHaveBeenLastCalledWith(7, "item:1");

      await userEvent.keyboard("{ArrowDown}");
      expect(previewResult).toHaveBeenLastCalledWith(7, "item:2");

      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    });

    it("Escape closes the preview before dismissing", async () => {
      render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      await screen.findByRole("complementary");
      await userEvent.keyboard("{Escape}");
      expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
      expect(hideOverlay).not.toHaveBeenCalled();
    });

    it("refreshes an open file preview when its code passage arrives, ignoring older answers", async () => {
      const { rerender } = render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(await screen.findByText(/hola/)).toBeInTheDocument();
      const pane = within(screen.getByRole("complementary"));

      let finishEarlier: ((preview: Preview) => void) | undefined;
      vi.mocked(previewResult).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishEarlier = resolve;
          }),
      );
      const code = (snippet: string) => ({
        ...file("item:1"),
        kind: "code" as const,
        snippet,
        code: { symbol: "retry", language: "python", repository: "lumen" },
      });
      results = { rows: [code("earlier passage")], status: "searching", queryId: 7 };
      rerender(<App />);
      expect(previewResult).toHaveBeenCalledTimes(2);
      expect(screen.queryByText(/hola/)).not.toBeInTheDocument();

      const match: Preview = {
        title: "item:1.txt",
        kind: "code",
        location: null,
        sizeBytes: null,
        modifiedMs: null,
        text: "matched retry passage",
        truncated: true,
      };
      vi.mocked(previewResult).mockResolvedValueOnce(match);
      results = { rows: [code(match.text ?? "")], status: "done", queryId: 7 };
      rerender(<App />);
      expect(await pane.findByText(/matched retry passage/)).toBeInTheDocument();
      expect(previewResult).toHaveBeenCalledTimes(3);
      await act(async () => {
        finishEarlier?.({ ...match, text: "earlier passage" });
        await Promise.resolve();
      });
      expect(pane.getByText(/matched retry passage/)).toBeInTheDocument();
      expect(pane.queryByText(/earlier passage/)).not.toBeInTheDocument();
    });

    it("refreshes exact filename previews even when code labels stay the same", async () => {
      const exact = () => ({
        ...file("item:1"),
        snippet: null,
        code: { symbol: "retry", language: "python", repository: "lumen" },
      });
      results = { rows: [exact()], status: "searching", queryId: 7 };
      const { rerender } = render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(await screen.findByText(/hola/)).toBeInTheDocument();
      vi.mocked(previewResult).mockResolvedValueOnce({
        title: "item:1.txt",
        kind: "file",
        location: null,
        sizeBytes: null,
        modifiedMs: null,
        text: "refined code passage",
        truncated: true,
      });
      results = { rows: [exact()], status: "done", queryId: 7 };
      rerender(<App />);
      expect(
        await within(screen.getByRole("complementary")).findByText(/refined code passage/),
      ).toBeInTheDocument();
      expect(previewResult).toHaveBeenCalledTimes(2);
    });

    it("refreshes PDF page context without changing the selected file or query", async () => {
      results = {
        rows: [{ ...file("item:1"), kind: "pdf-page", pdf: { pageNumber: 2 } }],
        status: "searching",
        queryId: 7,
      };
      const { rerender } = render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(await screen.findByText(/hola/)).toBeInTheDocument();
      vi.mocked(previewResult).mockResolvedValueOnce({
        title: "guide.pdf",
        kind: "pdf-page",
        location: null,
        sizeBytes: null,
        modifiedMs: null,
        text: "coastal habitat",
        truncated: true,
        pageNumber: 7,
      });
      results = {
        rows: [{ ...file("item:1"), kind: "pdf-page", pdf: { pageNumber: 7 } }],
        status: "done",
        queryId: 7,
      };
      rerender(<App />);
      expect(
        await within(screen.getByRole("complementary")).findByText(/coastal habitat/),
      ).toBeInTheDocument();
      expect(screen.getByRole("complementary")).toHaveTextContent("Page 7");
      expect(screen.getByRole("option")).toHaveAttribute("aria-selected", "true");
      expect(previewResult).toHaveBeenCalledTimes(2);
    });

    it("navigates rendered PDF pages, returns to the match and cancels on hide", async () => {
      results = {
        rows: [{ ...file("item:1"), title: "guide.pdf", extension: "pdf", pdf: { pageNumber: 3 } }],
        queryId: 7,
        status: "done",
      };
      vi.mocked(previewResult).mockResolvedValue({
        title: "guide.pdf",
        kind: "file",
        location: null,
        sizeBytes: 100,
        modifiedMs: null,
        text: "matched page three",
        truncated: true,
        pageNumber: 3,
      });
      render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      expect(await screen.findByRole("img", { name: "Page 3 of guide.pdf" })).toBeInTheDocument();
      expect(screen.getByRole("combobox", { name: "Search" })).toHaveFocus();
      await userEvent.keyboard("{Alt>}{PageDown}{/Alt}");
      expect(await screen.findByRole("img", { name: "Page 4 of guide.pdf" })).toBeInTheDocument();
      expect(nthOption(0)).toHaveAttribute("aria-selected", "true");
      expect(cancelPdfPreview).toHaveBeenCalled();
      await userEvent.click(screen.getByRole("button", { name: "Return to match" }));
      await screen.findByRole("img", { name: "Page 3 of guide.pdf" });
      const page = screen.getByRole("textbox", { name: "PDF page number" });
      await userEvent.clear(page);
      await userEvent.type(page, "10{Enter}");
      await screen.findByRole("img", { name: "Page 10 of guide.pdf" });
      expect(screen.getByRole("button", { name: "Next PDF page" })).toBeDisabled();
      await userEvent.keyboard("{Control>}l{/Control}");
      expect(screen.getByRole("combobox", { name: "Search" })).toHaveFocus();
      await act(async () => {
        hiddenHandler?.();
        await Promise.resolve();
      });
      expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
      expect(cancelPdfPreview).toHaveBeenCalledTimes(4);
    });

    it("drops a late raster after closing and uses the authorized page-action fallback", async () => {
      results = {
        rows: [{ ...file("item:1"), title: "guide.pdf", extension: "pdf", pdf: { pageNumber: 2 } }],
        queryId: 7,
        status: "done",
      };
      vi.mocked(previewResult).mockResolvedValue({
        title: "guide.pdf",
        kind: "file",
        location: null,
        sizeBytes: 100,
        modifiedMs: null,
        text: "matched page two",
        truncated: true,
        pageNumber: 2,
      });
      let finish: ((pdf: PdfPreview) => void) | undefined;
      vi.mocked(previewPdfPage).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      vi.mocked(listActions).mockResolvedValue([
        {
          id: "lumen.open-pdf-page",
          title: "Open matched PDF page",
          group: "common",
          shortcut: null,
        },
      ]);
      vi.mocked(runAction).mockResolvedValueOnce(true);
      render(<App />);
      await userEvent.keyboard("{Control>}k{/Control}");
      await screen.findByRole("listbox", { name: "Actions" });
      await userEvent.keyboard("{Enter}");
      await screen.findByText("Loading page…");
      await userEvent.keyboard("{Escape}");
      await act(async () => {
        finish?.({
          pageNumber: 2,
          pageCount: 10,
          width: 678,
          height: 960,
          image: "data:image/png;base64,old",
          unavailable: null,
        });
        await Promise.resolve();
      });
      expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
      expect(hideOverlay).not.toHaveBeenCalled();
      expect(runAction).toHaveBeenLastCalledWith(7, "item:1", "lumen.open-pdf-page", "panel");
    });

    it("covers the list when the monitor is too narrow for two panes", async () => {
      vi.mocked(resizeOverlay).mockImplementation((_w, height) =>
        Promise.resolve({ width: 1000, height }),
      );
      render(<App />);
      await userEvent.keyboard("{Alt>}{Enter}{/Alt}");
      const pane = await screen.findByRole("complementary");
      await act(async () => {
        await Promise.resolve();
      });
      expect(pane).toHaveClass("preview--over");
    });
  });
});
