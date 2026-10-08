# DESIGN_SYSTEM.md — premium Windows visual and interaction system

## 0. Implementation status (T004/T103)

- Tokens: `apps/desktop/src/design/tokens.css` (spacing, radius, type, geometry, motion,
  interaction colours) and `material.css` (surface/text per window material, ADR-024).
- Root search: search bar (glyph, clear button, combobox semantics), result rows 52 px
  (icon tile, title, middle-truncated location keeping the last folder, quiet kind label,
  "Open ↵" on the selection), no-results message. Window height follows the content
  (`layout.ts`, max 8 visible rows, shell caps at 72 % of the work area; top edge fixed).
- Documented exceptions: search-bar gap 18 px so query text and result titles share one x;
  the window radius is the native Windows 11 radius (ADR-024); the entrance is a 150 ms
  content fade, no scale (the native window and backdrop cannot scale), none under reduced
  motion.
- Not yet: real app/file icons (IconRef::Native), scope chips, status area, sections.

## 1. Design intent

The aesthetic target is “Apple-level care”, not “make Windows look like macOS”. The design should communicate calm, precision, hierarchy, responsiveness and material quality while respecting Windows conventions.

The interface must avoid the common AI-app look: no loud purple gradients, glowing borders everywhere, giant rounded cards, gratuitous particles, or verbose assistant copy.

Use native Windows material concepts intelligently. Microsoft recommends Mica as a performant foundation material and Acrylic for transient/light-dismiss surfaces. Lumen should use those principles rather than stacking multiple expensive blur layers.

## 2. Visual principles

1. **One dominant surface.** The search overlay is a single coherent object, not a dashboard.
2. **Content before chrome.** Search results are the hero.
3. **Depth through material + spacing + shadow, not borders everywhere.**
4. **Motion explains state.** Never animate merely to decorate.
5. **High information density, low visual noise.**
6. **A selected result is obvious without becoming neon.**
7. **Every pixel aligns to a system.** Avoid one-off values.
8. **Light and dark are designed, not inverted.**

## 3. Window geometry

### Compact search state

- preferred width: 760–840 px depending on scale factor;
- initial height: search bar + up to 6–8 result rows;
- maximum height: approximately 65–72% of the active monitor work area;
- horizontal position: centered;
- vertical position: around 18–24% from top of active monitor;
- outer corner radius: 20 px target, adjusted for Windows API limitations and scaling (T004/ADR-024: with a system backdrop the window uses the native Windows 11 radius, 8 px at 100 %);
- border: at most 1 physical pixel of low-contrast separation if needed;
- shadow: broad, soft, low-opacity; stronger in light mode.

The window may expand vertically as results/preview appear, but resizing must be smooth and should not cause content to jump.

### Preview state

When Quick Look opens, prefer horizontal expansion to a two-pane layout on sufficiently wide displays. On small displays, use an overlay preview panel.

Do not open a second floating window for normal preview.

## 4. Material

### Base

- Windows 11: prefer Mica-like base or a native system backdrop where technically stable.
- Search overlay is transient, so a carefully controlled acrylic/translucent treatment may be appropriate if performance and contrast remain excellent.
- Windows 10 / unsupported systems: fall back to an opaque/semi-opaque neutral surface with identical spacing and hierarchy.

### Rules

- never layer multiple full-window blurs;
- keep text and selection contrast WCAG-appropriate;
- avoid transparent text containers over visually busy wallpaper;
- if system material causes legibility issues, increase tint opacity before adding borders.

## 5. Typography

Primary: **Segoe UI Variable** / system UI stack. Do not redistribute proprietary Apple fonts.

Recommended scale:

- Search input: 20–22 px, regular/medium
- Result title: 14–15 px, semibold/medium
- Result subtitle/path: 12–13 px
- Snippet: 12.5–13.5 px with comfortable line height
- Section label: 11–12 px, medium, subtle
- Shortcut hint: 11–12 px, tabular where useful

Typography rules:

- no fake letter spacing on body text;
- paths use middle truncation when possible;
- numeric timecodes use tabular figures;
- maximum snippet: typically 2 lines in compact mode;
- use weight, not saturation, for hierarchy.

## 6. Spacing system

Base unit: 4 px. Most layout spacing uses multiples of 4; major rhythm uses 8.

Suggested tokens:

```text
space-1  = 4
space-2  = 8
space-3  = 12
space-4  = 16
space-5  = 20
space-6  = 24
space-8  = 32
space-10 = 40
```

Avoid random 13/17/19 px layout values unless optical correction is documented.

## 7. Radius system

```text
radius-xs = 6   # tags, tiny controls
radius-sm = 9   # buttons/input internals
radius-md = 12  # result selection, preview cards
radius-lg = 16  # larger panels
radius-xl = 20  # main overlay
```

The window radius is the largest radius. Nested components must not visually compete with it.

## 8. Color system

Use semantic tokens, not hard-coded colors in components.

Minimum token groups:

- `surface/base`
- `surface/elevated`
- `surface/selection`
- `surface/hover`
- `text/primary`
- `text/secondary`
- `text/tertiary`
- `stroke/subtle`
- `accent/default`
- `accent/soft`
- `status/error`, `warning`, `success`

Accent should be used sparingly: selection indicator, focus, indexing status and small emphasis—not every icon.

Allow the system accent color as an option, but ensure the UI still looks coherent with extreme accent choices.

## 9. Search bar

The search field is the visual anchor.

Components:

- subtle search glyph;
- text input with generous left/right padding;
- optional scope chip(s) when active;
- right-side state area for shortcut/help/indexing status only when useful.

Behavior:

- placeholder should teach capability, e.g. `Search files, images, code…`;
- no large “Ask AI” language;
- clear button appears only when there is text;
- IME/composition must work correctly;
- selection/focus ring must be visible but refined.

## 10. Result row

Recommended structure:

```text
[thumbnail/icon] [title                         ] [type/time]
                 [snippet / semantic hit        ]
                 [path / location               ] [shortcut]
```

Rules:

- 52–72 px height depending on result type;
- selected state uses tint + subtle inner highlight, not a thick border;
- thumbnails are consistent in aspect treatment and corner radius;
- icons should not all be colorful;
- show semantic snippets only when they add value;
- do not show raw similarity scores in normal UI.

## 11. Motion system

Target durations:

- hover/selection color: 90–130 ms
- result insertion/reorder: 140–180 ms
- overlay appearance: 150–190 ms
- preview expansion: 180–240 ms
- settings navigation: 180–220 ms

Overlay entrance:

- opacity 0 → 1;
- scale roughly 0.985 → 1;
- tiny vertical translation optional;
- avoid bouncing.

Semantic reranking:

- preserve the selected item position once keyboard navigation starts;
- animate unselected row movement subtly;
- avoid more than one visible reorder burst per query stabilization cycle;
- if new semantic results arrive rapidly, batch them.

Respect Windows reduced-motion settings. Under reduced motion, use opacity/state changes without scale/translation.

## 12. Keyboard model

Default conceptual mapping (actual shortcut is configurable):

- global shortcut → show/focus Lumen
- `Esc` → close preview first, then dismiss overlay
- `↑/↓` → navigate results
- `Enter` → primary open action
- `Ctrl+Enter` → reveal in Explorer
- `Alt+Enter` → Quick Look/details
- `Ctrl+C` with result focused and input not selecting text → copy path (only if discoverable and conflict-safe)
- `Tab` → move into action strip/scopes
- `Shift+Tab` → reverse
- `Ctrl+L` or equivalent → focus/select query if needed

Do not steal common text-editing shortcuts while the input owns them.

## 13. Empty, loading and error states

### Empty query

Show recent/pinned/useful items. Avoid a blank box.

### No results

Offer compact guidance:

- remove filters;
- search a broader phrase;
- check whether the location is indexed.

### Semantic engine unavailable

Lexical search must continue working. Show a quiet non-blocking status; never make the app unusable because the model failed.

### Indexing

Do not show a giant progress modal. A subtle progress affordance can open the Index Control Center.

## 14. Settings design

Settings may be a normal full app window, still using the same visual language.

Sections:

- Search
- Indexing
- Privacy
- Appearance
- Shortcut
- Performance
- About / diagnostics

Avoid more than one level of navigation where possible.

## 15. Visual QA checklist

Every UI PR should check:

- 100%, 125%, 150%, 200% display scaling;
- light/dark;
- high-contrast mode;
- reduced motion;
- 1366×768 and large 4K monitor;
- long filenames/paths;
- empty, 1-result, many-result states;
- keyboard-only operation;
- semantic reranking while selection is active;
- focus restored after closing preview;
- no visible white/black flash when showing overlay.

## 16. Anti-patterns

Do not:

- use glass on every nested card;
- use gradient text for branding;
- add chat bubbles to search results;
- show a spinner for sub-100ms work;
- animate every icon;
- use huge empty padding that reduces result density;
- copy macOS traffic-light controls;
- ship a custom font merely to imitate Apple;
- make search results wait for semantic inference before rendering anything.
# Refinement — command-center interaction design

## Root surface

The default state is one visually calm search surface. Do not show category tabs, dashboard cards or permanent sidebars in the initial overlay.

Provider diversity is communicated subtly through iconography/metadata, not by fragmenting the interface.

## Action Panel

`Ctrl+K` opens contextual actions for the selected result. The panel should feel like a continuation of the same surface, not a modal settings dialog.

Requirements:

- keyboard searchable when actions exceed a small list;
- primary action first;
- icons consistent and low-noise;
- destructive actions visually separated;
- no layout jump of the selected source item;
- closing returns focus exactly where it was.

## Quick Look

Preview expansion should preserve spatial context. Prefer one smooth width/height/layout transition over popping a second unrelated window.

## Semantic Drop

Dragging/pasting a query object should create a clear but restrained state:

```text
[thumbnail/object chip]  Find things like this…
```

The object chip is removable with keyboard and mouse. Do not turn the root field into a chat-composer UI.

## Semantic refinement

When semantic results arrive:

- do not flash/skeleton if lexical results already exist;
- use subtle position transitions only before active keyboard navigation;
- once the user moves selection, preserve the selected item and avoid reordering under it;
- never animate every row independently with exaggerated spring motion.

## Density

Lumen is information-dense but not cramped. Favor compact premium rows over giant cards. A launcher should let users scan 6–10 useful results immediately at normal desktop sizes.

## Apple-level quality interpretation

Target:

- exact spacing rhythm;
- exceptional typography;
- understated materials;
- consistent icon optical weight;
- predictable focus ring;
- 90–240ms purposeful motion;
- zero accidental scrollbars/focus flashes;
- perfect truncation/tooltip behavior;
- DPI correctness;
- dark/light modes designed, not inverted.

Avoid:

- gratuitous gradients;
- huge rounded cards nested in cards;
- blur everywhere;
- macOS traffic-light mimicry;
- "AI" sparkles as a generic semantic-search icon;
- animation on every state change.

