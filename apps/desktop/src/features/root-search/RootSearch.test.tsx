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
    primaryAction: "lumen.open",
  },
  {
    id: "item:2",
    kind: "file",
    title: "notas.md",
    detail: "C:\\Users\\Joao\\Proyectos\\lumen",
    extension: "md",
    primaryAction: "lumen.open",
  },
  {
    id: "item:3",
    kind: "folder",
    title: "Proyectos",
    detail: "C:\\Users\\Joao",
    extension: null,
    primaryAction: "lumen.open",
  },
];

function nth<T>(items: readonly T[], index: number): T {
  const item = items[index];
  if (item === undefined) throw new Error(`no item ${String(index)}`);
  return item;
}

function renderSearch(
  query: string,
  results: Pick<ResultsState, "rows" | "status">,
  selectedIndex = 0,
) {
  const props = {
    onQueryChange: vi.fn(),
    onSelect: vi.fn(),
    onActivate: vi.fn(),
    onKeyDown: vi.fn(),
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
  it("shows the matched PDF page in the selected accessible file row", () => {
    const pdf: ResultRowModel = {
      ...nth(rows, 1),
      kind: "pdf-page",
      title: "guide.pdf",
      extension: "pdf",
      snippet: "Protect coastal habitat",
      pdf: { pageNumber: 7 },
    };
    renderSearch("ocean conservation ext:pdf", { rows: [pdf], status: "done" });
    const option = screen.getByRole("option");
    expect(option).toHaveAccessibleName(/Page 7 · guide.pdf/);
    expect(option).toHaveTextContent("Protect coastal habitat");
    expect(option).toHaveTextContent("Open");
    expect(screen.getByRole("combobox")).toHaveAttribute("aria-activedescendant", option.id);
  });
  it("shows the code symbol and file in one accessible row with repository context", () => {
    const code: ResultRowModel = {
      ...nth(rows, 1),
      kind: "code",
      title: "client.py",
      extension: "py",
      snippet: "def retry_request(url): return request(url)",
      code: { symbol: "retry_request", language: "python", repository: "lumen" },
    };
    renderSearch("retry python", { rows: [code], status: "done" });
    const option = screen.getByRole("option");
    expect(option).toHaveTextContent("retry_request · client.py");
    expect(option).toHaveTextContent("def retry_request(url)");
    expect(option).toHaveAttribute("title", expect.stringContaining("python · lumen"));
    expect(screen.getByRole("combobox")).toHaveAttribute("aria-activedescendant", option.id);
  });
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

  it("shows the matching passage for content/meaning matches, the location on hover", () => {
    const found: ResultRowModel = {
      id: "item:9",
      kind: "file",
      title: "reunion.md",
      detail: "C:\\Users\\Joao\\Notas",
      snippet: "…enviar el contrato firmado antes del viernes…",
      extension: "md",
      primaryAction: "lumen.open",
    };
    renderSearch("contrato", { rows: [found], status: "done" });
    const option = screen.getByRole("option");
    expect(option).toHaveTextContent("enviar el contrato firmado");
    expect(option).not.toHaveTextContent("Notas");
    expect(option).toHaveAttribute("title", "C:\\Users\\Joao\\Notas");
  });
});
