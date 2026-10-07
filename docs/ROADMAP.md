# ROADMAP.md

## Product sequencing principle

Build depth before breadth. Lumen should become excellent at one daily loop before adding another.

## M0 — foundation

Goal: prove shell, core boundaries, storage, model and ANN choices.

No productivity feature work.

## M1 — instant launcher

Goal: replace a large fraction of Start/Explorer opening behavior.

Ship-quality loop:

```text
hotkey → type app/file → results → action → dismiss
```

Includes universal result/action contracts but only basic providers/actions.

## M2 — semantic text/code

Goal: make "I remember what it did/said" genuinely useful.

No clipboard/workflow scope until latency/relevance are stable.

## M3 — PDF/image semantic objects

Goal: first major wow moment.

- page-level PDF search
- screenshot OCR + semantic retrieval
- Semantic Drop
- Find Similar
- related-content primitive

## M4 — daily productivity surface

Goal: Alfred/Raycast-class daily usefulness.

Add calculator, settings/system commands, quicklinks, snippets, clipboard, collections and optional window actions.

## M5 — workflows

Goal: users can turn repeated routines into searchable commands.

Keep V1 workflows deterministic and inspectable.

## M6 — memory/context

Goal: Lumen understands "what I was working on" and project context.

- recent-work temporal search
- Semantic Workspaces
- Context Lens
- optional Rewind

## M7 — audio/video

Goal: search media by remembered content and jump to exact moment.

## M8 — public extensibility/hardening

Only now review whether provider/action APIs are stable enough to expose.

## Explicitly deferred

- generic chat-first assistant;
- cloud sync;
- cross-device LAN search;
- extension marketplace;
- continuous screen recording;
- complex visual node workflow editor;
- autonomous destructive agent actions.

These require their own product/architecture decision.
