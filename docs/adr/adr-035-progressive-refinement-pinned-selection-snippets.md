# ADR-035 — Progressive refinement: one settled update, the selected row stays put, passages for content/meaning matches

**Status:** Accepted (T206). Code: `lumen_search::SETTLED_BATCH` (coordinator), UI
`selection.ts::stabilize`, `App.tsx`, `ResultRow.tsx`, wire field `ResultDto.snippet`.

**Context.** Since ADR-032 a query is answered twice: names while typing, then names +
contents + meaning once settled. DESIGN_SYSTEM §11/§17 ask for no reorder under the
user's selection, at most one visible reorder burst per stabilisation, and snippets only
where they add value.

**Decision**

- **One refinement burst.** A settled run first recomputes the rows the typing run already
  showed; its intermediate lists are held back for `SETTLED_BATCH` = 150 ms, so with a warm
  model (meaning ≈ 50 ms) contents and meaning arrive in one update. A lane slower than the
  window (cold model load) no longer holds back the others: updates resume after 150 ms.
- **The selected row stays put.** Until the user moves the selection the refined order is
  shown as is (top row selected). Once they have moved it, a refinement keeps the selected
  result at its on-screen index and lets the other rows flow around it (`stabilize`,
  pure, applied in `App` before every use of the rows: keys, actions, preview, layout). A
  result that disappears falls back to the same index (T104).
- **Passages for content/meaning matches.** When the row shown for an entity came from the
  content or meaning lane (`MatchKind::FullText | Semantic` with a passage), the second
  line shows that passage (one line, ellipsized) instead of the location; the location
  moves to the row's tooltip and stays in Quick Look. Name matches keep the location line.
  Row height stays 52 px, so window sizing (layout.ts) is unchanged.

**Consequences**

- Keyboard users never see the row they are on jump away; mouse hover selection counts as
  a move, so the row under the pointer also stays.
- The passage is the chunk start (contents: the FTS snippet around the match; meaning: the
  first 160 characters), which for code is often a doc comment. Choosing the best sentence
  of the chunk is a later refinement.
- No animation yet for unselected rows (DESIGN_SYSTEM allows subtle movement); the list is
  re-rendered in place.
