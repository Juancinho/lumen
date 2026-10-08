import { act, fireEvent, render, screen } from "@testing-library/react";
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
  resizeOverlay,
  type Appearance,
  type OverlayShown,
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
  resizeOverlay: vi.fn(),
}));

let shownHandler: ((shown: OverlayShown) => void) | undefined;
let appearanceHandler: ((appearance: Appearance) => void) | undefined;
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
  vi.mocked(runAction).mockResolvedValue(undefined);
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

    // A later update re-orders: the selection follows "b".
    results = { rows: [r("x"), r("a"), r("c"), r("b")], status: "done", queryId: 1 };
    rerender(<App />);
    expect(nthOption(3)).toHaveAttribute("aria-selected", "true");

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
