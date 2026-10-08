import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ResultRowModel } from "./model";
import { listState, rootSearchHeight } from "./layout";
import { RootSearch } from "./RootSearch";
import type { ResultsState } from "./useResults";

const rows: ResultRowModel[] = [
  {
    id: "item:1",
    kind: "application",
    title: "Visual Studio Code",
    detail: "Application",
    extension: null,
  },
  {
    id: "item:2",
    kind: "file",
    title: "notas.md",
    detail: "C:\\Users\\Joao\\Proyectos\\lumen",
    extension: "md",
  },
  { id: "item:3", kind: "folder", title: "Proyectos", detail: "C:\\Users\\Joao", extension: null },
];

function nth<T>(items: readonly T[], index: number): T {
  const item = items[index];
  if (item === undefined) throw new Error(`no item ${String(index)}`);
  return item;
}

function renderSearch(query: string, results: ResultsState, selectedIndex = 0) {
  const props = {
    onQueryChange: vi.fn(),
    onSelect: vi.fn(),
    onActivate: vi.fn(),
  };
  render(
    <RootSearch
      query={query}
      inputRef={null}
      results={results}
      selectedIndex={selectedIndex}
      {...props}
    />,
  );
  return props;
}

describe("RootSearch", () => {
  it("shows only the search bar while idle", () => {
    renderSearch("", { rows: [], status: "idle" });
    const input = screen.getByRole("combobox", { name: "Search" });
    expect(input).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear search" })).not.toBeInTheDocument();
  });

  it("lists results and points the combobox at the selected row", () => {
    renderSearch("no", { rows, status: "done" }, 1);
    const input = screen.getByRole("combobox");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(3);
    expect(input).toHaveAttribute("aria-expanded", "true");
    expect(input).toHaveAttribute("aria-controls", screen.getByRole("listbox").id);
    expect(input).toHaveAttribute("aria-activedescendant", options[1]?.id);
    expect(options[1]).toHaveAttribute("aria-selected", "true");
    expect(options[0]).toHaveAttribute("aria-selected", "false");
  });

  it("renders kind labels, the open hint on the selection and middle-truncatable paths", () => {
    renderSearch("no", { rows, status: "done" }, 0);
    const [app, file, folder] = screen.getAllByRole("option");
    expect(app).toHaveTextContent("Open");
    expect(file).toHaveTextContent("MD");
    expect(folder).toHaveTextContent("Folder");
    expect(file?.querySelector(".result-row__detail-head")).toHaveTextContent(
      "C:\\Users\\Joao\\Proyectos",
    );
    expect(file?.querySelector(".result-row__detail-tail")).toHaveTextContent("\\lumen");
    expect(app?.querySelector(".result-row__detail-head")).toBeNull();
  });

  it("hover selects, click activates, clear empties the query", async () => {
    const props = renderSearch("no", { rows, status: "done" }, 0);
    const options = screen.getAllByRole("option");
    await userEvent.hover(nth(options, 2));
    expect(props.onSelect).toHaveBeenCalledWith(2);
    await userEvent.click(nth(options, 1));
    expect(props.onActivate).toHaveBeenCalledWith(1);
    await userEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(props.onQueryChange).toHaveBeenCalledWith("");
  });

  it("says so quietly when a finished query has no results", () => {
    renderSearch("zzz ", { rows: [], status: "done" });
    expect(screen.getByRole("status")).toHaveTextContent("No matches for “zzz”");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("does not flash the no-results message while still searching", () => {
    expect(listState("zzz", { rows: [], status: "searching" })).toEqual({ kind: "none" });
    expect(listState("  ", { rows: [], status: "done" })).toEqual({ kind: "none" });
    expect(rootSearchHeight("x", { rows, status: "searching" })).toBe(64 + 1 + 12 + 3 * 52);
  });
});
