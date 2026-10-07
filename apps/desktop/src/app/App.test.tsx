import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { hideOverlay, onOverlayShown, overlayReady } from "../ipc";
import { App } from "./App";

vi.mock("../ipc", () => ({
  hideOverlay: vi.fn(),
  onOverlayShown: vi.fn(),
  overlayReady: vi.fn(),
}));

let shownHandler: (() => void) | undefined;
const unlisten = vi.fn();

beforeEach(() => {
  shownHandler = undefined;
  vi.mocked(hideOverlay).mockResolvedValue(undefined);
  vi.mocked(overlayReady).mockResolvedValue(undefined);
  vi.mocked(onOverlayShown).mockImplementation((handler) => {
    shownHandler = handler;
    return Promise.resolve(unlisten);
  });
});

describe("App overlay", () => {
  it("renders a labelled search field that owns focus on mount", () => {
    render(<App />);

    const input = screen.getByRole("searchbox", { name: "Search" });
    expect(input).toHaveFocus();
    expect(screen.getByRole("search")).toContainElement(input);
    expect(overlayReady).toHaveBeenCalledOnce();
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
      shownHandler?.();
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
});
