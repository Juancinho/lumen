# Lumen — semantic command center for Windows

> Working codename. Lumen is not merely a file-search application. It is a keyboard-first, local-first command center for Windows: the speed of Everything, the invocation model of Spotlight, the action/workflow philosophy of Alfred/Raycast, and a semantic layer powered by EmbeddingGemma 2.

## North star

Press one global shortcut and reach **anything you know, anything you were doing, and anything you can do on the computer** without navigating apps, folders or menus.

The root surface must stay deceptively simple:

```text
┌──────────────────────────────────────────────────────────────┐
│  Search anything…                                            │
├──────────────────────────────────────────────────────────────┤
│  best results from files, apps, commands, snippets, etc.    │
└──────────────────────────────────────────────────────────────┘
```

Complexity belongs underneath the surface, not in front of the user.

Examples:

- `spotify`
- `bluetooth settings`
- `2.3 * 17`
- `the screenshot where VS Code had a Docker error`
- `pdf where I explained gradient descent convergence`
- `python function that retries failed HTTP requests`
- `video where I raise my hand`
- `what I was working on Tuesday afternoon`
- `clipboard docker compose`
- `type:pdf transformers attention`
- drag an image into Lumen → find things like this
- select any result → `Ctrl+K` → contextual actions

## Product identity

Lumen has four pillars:

1. **Find** — exact, lexical, semantic and multimodal retrieval.
2. **Act** — contextual actions on every result, system commands and workflows.
3. **Remember** — local temporal context, recent work and optional Rewind.
4. **Extend** — internal providers first, public extension/workflow SDK later.

The differentiator is not "AI in a launcher". It is a **local semantic model of the user's computer** that other capabilities can build on.

## Non-negotiables

1. Local-first and private by default.
2. Perceived latency is a product feature.
3. Keyboard-first, mouse-excellent, accessibility preserved.
4. Premium visual quality: restraint, hierarchy, excellent motion and typography.
5. No UI blocking on filesystem, extraction, inference or indexing.
6. Incremental, resumable, prioritized indexing.
7. Bounded RAM/CPU/disk with explicit performance profiles.
8. Tauri/React is the current shell, **not the architecture**; core remains shell-agnostic.
9. Providers/actions/workflows are first-class domain concepts, but no premature public plugin framework.
10. Project state lives in the repo so Codex, Claude and other agents can continue without chat context.

## Reference product patterns

We borrow patterns, not visual copies:

- **Everything:** immediate exact filename/path retrieval.
- **Spotlight:** effortless global invocation and progressive disclosure.
- **Alfred:** universal actions, workflows, clipboard/snippets/quicklinks.
- **Raycast:** command palette, action panel, extensions, productivity surface.
- **PowerToys Run:** practical Windows-native commands and system integration.

Lumen's own territory:

- semantic file/code/document/image/audio/video search;
- cross-modal search and Semantic Drop;
- Context Lens and related-content graph;
- Semantic Workspaces;
- optional Rewind built from local metadata/events rather than mandatory continuous screen recording;
- local semantic collections and "find similar" across modalities.

## Recommended production stack

- **Desktop shell:** Tauri 2
- **UI:** React + TypeScript + Vite
- **Core/search/indexing:** Rust
- **Metadata + lexical:** SQLite WAL + FTS5
- **Vector ANN:** USearch/HNSW, memory-mapped where practical
- **Model:** EmbeddingGemma 2 behind `EmbeddingBackend`
- **Default embedding:** 256d, normalized; compact scalar chosen by benchmark
- **Windows integration:** Win32/Windows crate + Tauri global shortcut
- **Code parsing:** Tree-sitter where useful
- **PDF:** PDFium-style extraction/rendering after packaging/licensing validation
- **Media:** Windows Media Foundation in production where practical

The inference runtime must remain replaceable. Do not require Python in production.

## Repository map

```text
/
├─ AGENTS.md
├─ CLAUDE.md
├─ README.md
├─ PROJECT_STATE.md
├─ TASKS.md
├─ HANDOFF.md
├─ WORKLOG.md
├─ docs/
│  ├─ PRODUCT.md
│  ├─ FEATURE_CATALOG.md
│  ├─ ARCHITECTURE.md
│  ├─ COMMAND_MODEL.md
│  ├─ EXTENSIONS_AND_WORKFLOWS.md
│  ├─ DESIGN_SYSTEM.md
│  ├─ SEARCH_AND_INDEXING.md
│  ├─ PERFORMANCE.md
│  ├─ PRIVACY_SECURITY.md
│  ├─ TESTING.md
│  ├─ ROADMAP.md
│  ├─ AGENT_PROTOCOL.md
│  ├─ DECISIONS.md
│  ├─ IMPLEMENTATION_NOTES.md
│  ├─ RELEASE_AND_LICENSING.md
│  └─ AGENT_PROMPTS.md
└─ src / apps / crates ...
```

## Build order

Lumen is built as vertical slices:

1. instant launcher + files/apps;
2. universal result/action model;
3. text/code semantic search;
4. PDFs/images + Semantic Drop / Find Similar;
5. productivity providers (calculator, snippets, clipboard, quicklinks, Windows commands);
6. workflows;
7. temporal context / Semantic Workspaces;
8. audio/video;
9. Rewind / Context Lens maturation;
10. public extension SDK only after the internal provider API is stable.

A feature is not complete if it works but feels slow, visually rough, unstable or resource-hungry.
