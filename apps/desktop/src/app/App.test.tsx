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
  type Appearance,
  type OverlayShown,
} from "../ipc";
import { App } from "./App";

vi.mock("../ipc", () => ({
  getAppearance: vi.fn(),
  hideOverlay: vi.fn(),
  onAppearanceChanged: vi.fn(),
  onOverlayShown: vi.fn(),
  overlayPainted: vi.fn(),
  overlayReady: vi.fn(),
}));

let shownHandler: ((shown: OverlayShown) => void) | undefined;
let appearanceHandler: ((appearance: Appearance) => void) | undefined;
const unlisten = vi.fn();

beforeEach(() => {
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

    const input = screen.getByRole("searchbox", { name: "Search" });
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
    expect(screen.getByRole("searchbox")).toHaveValue("spotify");
  });

  it("re-focuses and selects the previous query when the shell shows the overlay", async () => {
    render(<App />);
    const input = screen.getByRole<HTMLInputElement>("searchbox");
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
});
