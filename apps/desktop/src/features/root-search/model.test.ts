import { describe, expect, it } from "vitest";

import { isPath, kindLabel, splitPath } from "./model";

describe("result model", () => {
  it("labels kinds quietly", () => {
    expect(kindLabel({ kind: "application", extension: null })).toBe("Application");
    expect(kindLabel({ kind: "file", extension: "pdf" })).toBe("PDF");
    expect(kindLabel({ kind: "file", extension: null })).toBe("File");
    expect(kindLabel({ kind: "folder", extension: null })).toBe("Folder");
    expect(kindLabel({ kind: "command", extension: null })).toBe("Command");
  });

  it("keeps the last folder visible when splitting a path", () => {
    expect(splitPath("C:\\Users\\Joao\\Proyectos\\lumen")).toEqual({
      head: "C:\\Users\\Joao\\Proyectos",
      tail: "\\lumen",
    });
    expect(splitPath("/home/joao/docs")).toEqual({ head: "/home/joao", tail: "/docs" });
    // A very short last folder keeps its parent too.
    expect(splitPath("D:\\Proyectos\\lumen\\src")).toEqual({
      head: "D:\\Proyectos",
      tail: "\\lumen\\src",
    });
    expect(splitPath("notes")).toEqual({ head: "", tail: "notes" });
    expect(splitPath("C:\\")).toEqual({ head: "", tail: "C:\\" });
    expect(splitPath("C:\\Users")).toEqual({ head: "C:", tail: "\\Users" });
  });

  it("recognizes paths", () => {
    expect(isPath("C:\\Users")).toBe(true);
    expect(isPath("\\\\server\\share")).toBe(true);
    expect(isPath("/home/joao")).toBe(true);
    expect(isPath("Application")).toBe(false);
  });
});
