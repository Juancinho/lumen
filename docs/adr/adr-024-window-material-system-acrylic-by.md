# ADR-024 — Window material: system Acrylic by default, Solid fallback, native corners


**Status:** Accepted on measurements (T004); the default (`auto` = Acrylic vs Mica) stays open
to the user's visual review. Evidence: `docs/benchmarks/t004/2026-10-08-joao-pc/
material-dark.json` (Windows 11 build 26300, dark, GTX 1650 + Radeon iGPU):

| material | show → painted p50/p95 | DWM 3D GPU visible / hidden | on-screen contrast primary / secondary |
|---|---:|---:|---:|
| auto → acrylic | 21.3 / 49.3 ms | 3.8 % / 0.0 % | 12.8 / 5.9 |
| acrylic | 22.8 / 39.5 ms | 3.0 % / 1.6 % | 11.6 / 5.3 |
| mica | 22.1 / 37.3 ms | 0.8 % / 0.3 % | (screenshot missed) |
| solid | 23.4 / 36.1 ms | 0.2 % / 0.0 % | 16.3 / 7.5 |

No latency cost; Acrylic costs a few % of DWM GPU only while visible; real composited secondary
text ≥ 5.3:1 (the tint-only floor is 3.4:1). Per-show re-check 0.06 ms p50. Code: `lumen_windows::material` (pure plan + DWM/WinRT probes),
`apps/desktop/src-tauri/src/material.rs`, `apps/desktop/src/design/material.css`.

**Decision**

- **Materials:** `acrylic` = DWM `DWMSBT_TRANSIENTWINDOW` (Microsoft's material for
  transient, light-dismiss surfaces), `mica` = `DWMSBT_MAINWINDOW` (wallpaper-only), `solid` =
  no backdrop. Applied through Tauri window effects on a transparent window (one system blur;
  the UI never adds CSS blur). `auto` (default) = Acrylic.
- **Only documented APIs:** backdrops from Windows 11 22H2 (build 22621). Older builds get
  `solid`, never the undocumented `SetWindowCompositionAttribute` (Windows 10 acrylic) or
  `DWMWA_MICA_EFFECT` (21H2) paths. Windows 10 = solid, square corners.
- **Accessibility wins:** high contrast or "Transparency effects" off → `solid` whatever was
  chosen (CSS `forced-colors` then uses system colours). Re-read before every show (two WinRT
  property reads); re-applied only when the plan changes. The tray says when a choice fell
  back and why.
- **Corners and shadow are native:** `DWMWCP_ROUND` (8 px at 100 %, DPI-scaled) with the DWM
  shadow and 1 px system border. The 20 px target in DESIGN_SYSTEM §3 is not reachable with a
  system backdrop (DWM clips the backdrop to its own radius; a CSS-rounded surface inside a
  transparent window would show square blurred corners and needs a click-through margin for a
  CSS shadow). `radius-xl` therefore follows the system radius; nested radii stay below it.
- **Legibility floor in tokens, not borders:** the UI paints the surface colour at
  `--lumen-tint-alpha` 0.76 over the backdrop. With the tint alone (ignoring the system
  material's luminosity layer) primary text stays ≥ 7:1 and secondary ≥ 3:1 over a pure
  black or white backdrop; on solid, secondary ≥ 4.5:1. `material.test.ts` enforces this.
- **Choice:** tray → Window material (Automatic / Acrylic / Mica / Solid), saved as
  `appearance.material`; `LUMEN_MATERIAL` overrides it for one run (benchmarks).

**Consequences**

- T103 builds on `--lumen-background`, `--lumen-text-primary/secondary` and the corner
  attribute (`<html data-material data-corners>`); it must not introduce a second blur layer.
- The Windows run decides: show→paint cost per material (must stay within the 35 ms p50
  budget), DWM GPU load while visible, real composited contrast over bright/dark windows, and
  whether `auto` should be Mica instead (calmer, no busy-window bleed). Then Accepted.
- Battery saver and inactive windows are handled by Windows (the backdrop turns solid).
