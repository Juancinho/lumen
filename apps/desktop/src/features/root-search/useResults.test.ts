import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { onCatalogChanged, onOverlayShown, onResults, search, type ResultsUpdate } from "../../ipc";
import { useResults } from "./useResults";

vi.mock("../../ipc", () => ({
  onResults: vi.fn(),
  onOverlayShown: vi.fn(),
  onCatalogChanged: vi.fn(),
  search: vi.fn(),
}));

let push: ((u: ResultsUpdate) => void) | undefined;
let shown: (() => void) | undefined;
let changed: (() => void) | undefined;

beforeEach(() => {
  push = undefined;
  vi.mocked(onResults).mockImplementation((h) => {
    push = h;
    return Promise.resolve(vi.fn());
  });
  vi.mocked(onOverlayShown).mockImplementation((h) => {
    shown = () => {
      h({ seq: null });
    };
    return Promise.resolve(vi.fn());
  });
  vi.mocked(onCatalogChanged).mockImplementation((h) => {
    changed = h;
    return Promise.resolve(vi.fn());
  });
  vi.mocked(search).mockResolvedValue(true);
});

const row = (id: string) => ({
  id,
  kind: "file" as const,
  title: id,
  detail: null,
  extension: null,
});

async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useResults", () => {
  it("asks only once the listener is ready, with increasing ids", async () => {
    const { result, rerender } = renderHook(({ q }) => useResults(q), {
      initialProps: { q: "" },
    });
    expect(search).not.toHaveBeenCalled();
    expect(result.current.status).toBe("idle");
    await settle();
    expect(search).toHaveBeenLastCalledWith(1, "");
    rerender({ q: "vi" });
    expect(search).toHaveBeenLastCalledWith(2, "vi");
    expect(result.current.status).toBe("searching");
  });

  it("ignores updates for other queries and keeps old rows until new ones arrive", async () => {
    const { result, rerender } = renderHook(({ q }) => useResults(q), {
      initialProps: { q: "v" },
    });
    await settle();
    act(() => {
      push?.({ queryId: 1, done: true, results: [row("item:1")] });
    });
    expect(result.current).toEqual({ rows: [row("item:1")], status: "done" });

    rerender({ q: "vs" }); // id 2
    expect(result.current.rows).toEqual([row("item:1")]);
    act(() => {
      push?.({ queryId: 1, done: true, results: [row("item:9")] }); // stale
    });
    expect(result.current.rows).toEqual([row("item:1")]);
    act(() => {
      push?.({ queryId: 2, done: false, results: [row("item:2")] });
    });
    expect(result.current).toEqual({ rows: [row("item:2")], status: "searching" });
    act(() => {
      push?.({ queryId: 2, done: true, results: [] });
    });
    expect(result.current).toEqual({ rows: [], status: "done" });
  });

  it("re-runs the query when the overlay is shown or the catalog changed", async () => {
    renderHook(() => useResults("code"));
    await settle();
    expect(search).toHaveBeenLastCalledWith(1, "code");
    act(() => {
      shown?.();
    });
    expect(search).toHaveBeenLastCalledWith(2, "code");
    act(() => {
      changed?.();
    });
    expect(search).toHaveBeenLastCalledWith(3, "code");
  });
});
