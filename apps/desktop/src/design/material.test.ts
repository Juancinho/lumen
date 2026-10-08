import { describe, expect, it } from "vitest";

import css from "./material.css?raw";

// WCAG 2.x relative luminance / contrast ratio.
type Rgb = readonly [number, number, number];

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/** `tint` at `alpha` over `backdrop`, composited in sRGB like the browser does. */
function over(tint: Rgb, alpha: number, backdrop: Rgb): Rgb {
  const mix = (t: number, b: number) => alpha * t + (1 - alpha) * b;
  return [mix(tint[0], backdrop[0]), mix(tint[1], backdrop[1]), mix(tint[2], backdrop[2])];
}

function required(value: string | undefined, what: string): string {
  if (value === undefined) throw new Error(`missing ${what}`);
  return value;
}

function hex(value: string): Rgb {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(value);
  if (!m) throw new Error(`not #rrggbb: ${value}`);
  const byte = (i: number) => parseInt(required(m[i], value), 16);
  return [byte(1), byte(2), byte(3)];
}

/** Custom properties of the first `:root { ... }` block at or after `marker`. */
function block(marker: string): Record<string, string> {
  const start = css.indexOf(marker);
  if (start < 0) throw new Error(`no block for ${marker}`);
  const open = css.indexOf(":root {", start);
  const body = css.slice(open + ":root {".length, css.indexOf("}", open));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    vars[required(m[1], "name")] = required(m[2], "value").trim();
  }
  return vars;
}

const light = block(":root {");
const dark = { ...light, ...block("prefers-color-scheme: dark") };
const BLACK: Rgb = [0, 0, 0];
const WHITE: Rgb = [255, 255, 255];

describe.each([
  ["light", light],
  ["dark", dark],
])("material tokens (%s)", (_, vars) => {
  const channels = required(vars["--lumen-surface-rgb"], "surface").split(/\s+/).map(Number);
  const surface: Rgb = [channels[0] ?? NaN, channels[1] ?? NaN, channels[2] ?? NaN];
  const alpha = Number(light["--lumen-tint-alpha"]);
  const primary = hex(required(vars["--lumen-text-primary"], "primary"));
  const secondary = hex(required(vars["--lumen-text-secondary"], "secondary"));
  const worst = (text: Rgb) =>
    Math.min(...[BLACK, WHITE].map((b) => contrast(text, over(surface, alpha, b))));

  it("parses", () => {
    expect(channels).toHaveLength(3);
    expect(surface.every(Number.isFinite)).toBe(true);
    expect(alpha).toBeGreaterThan(0);
    expect(alpha).toBeLessThan(1);
  });

  it("solid surface: primary >= 7:1, secondary >= 4.5:1", () => {
    expect(contrast(primary, surface)).toBeGreaterThanOrEqual(7);
    expect(contrast(secondary, surface)).toBeGreaterThanOrEqual(4.5);
  });

  it("translucent tint over any backdrop: primary >= 7:1, secondary >= 3:1", () => {
    expect(worst(primary)).toBeGreaterThanOrEqual(7);
    expect(worst(secondary)).toBeGreaterThanOrEqual(3);
  });
});

describe("material selectors", () => {
  it("only acrylic and mica make the surface translucent", () => {
    expect(css).toMatch(/:root\[data-material="acrylic"\],\s*:root\[data-material="mica"\]/);
    expect(css).not.toMatch(/data-material="solid"/);
  });
});
