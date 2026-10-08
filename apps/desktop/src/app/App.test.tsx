import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getAppearance,
  hideOverlay,
  onAppearanceChanged,
  onOverlayShown,
  overlayPainted,
  overlayReady,
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
  onAppearanceChanged: vi.fn(),
  onOverlayShown: vi.fn(),
  overlayPainted: vi.fn(),
  overlayReady: vi.fn(),
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

let results: ResultsState = { rows: [], status: "idle" };

beforeEach(() => {
  results = { rows: [], status: "idle" };
  vi.mocked(useResults).mockImplementation(() => results);
  vi.mocked(resizeOverlay).mockImplementation((h) => Promise.resolve(h));
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

    fireEvent.keyDown(window, { key: "Escape", isComposing: true });
    fireEvent.keyDown(window, { key: "Escape", keyCode: 229 });
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
    expect(resizeOverlay).toHaveBeenLastCalledWith(64);

    results = {
      rows: [
        { id: "item:1", kind: "application", title: "Calculator", detail: null, extension: null },
        { id: "item:2", kind: "file", title: "calc.xlsx", detail: null, extension: "xlsx" },
      ],
      status: "done",
    };
    rerender(<App />);
    expect(resizeOverlay).toHaveBeenLastCalledWith(64 + 1 + 12 + 2 * 52);

    await userEvent.hover(nthOption(1));
    expect(screen.getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    await userEvent.type(screen.getByRole("combobox"), "c");
    expect(screen.getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
  });
});
