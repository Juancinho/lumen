import type { Appearance } from "../ipc";

/**
 * Exposes the window surface to CSS as `<html data-material data-corners>`; the material
 * tokens in `src/design/material.css` key off these attributes. Before this runs the
 * defaults are the opaque surface and square corners.
 */
export function applyAppearance(
  appearance: Appearance,
  root: HTMLElement = document.documentElement,
) {
  root.dataset.material = appearance.material;
  root.dataset.corners = appearance.corners;
}
