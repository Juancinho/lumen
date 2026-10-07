# PRODUCT.md — product definition

## 1. One sentence

**Lumen is the fastest way to find, understand, remember and act on anything on a Windows PC from one keyboard-first surface.**

## 2. Product thesis

Traditional launchers know names. Search engines know text. File explorers know locations. AI assistants know language but usually do not possess a faithful local model of the computer.

Lumen combines:

- exact/navigation search;
- lexical full-text search;
- multimodal semantic retrieval;
- contextual actions;
- deterministic utilities/commands;
- workflows;
- local temporal/context memory.

The result should feel like invoking a capability, not opening an application.

## 3. Core jobs to be done

### Find

"I know it exists but not where or what it was called."

Return files, pages, symbols, images, media moments, apps, clipboard entries, snippets, commands and workflows.

### Act

"I found it; now let me do the obvious next thing without leaving the keyboard."

Open, reveal, copy, preview, find similar, run, transform, launch workflow, change window state, etc.

### Remember

"I remember what I was doing, not the filename."

Use local activity metadata and semantics to recover recent work, sessions and related items.

### Extend

"My personal workflow has tools Lumen does not ship with."

Eventually expose stable provider/action/workflow capabilities to extensions after the internal model proves itself.

## 4. Root-search principle

The root field is universal. Users should not normally choose a mode first.

Query examples:

```text
spotify
bluetooth settings
2.3 * 17
transformers paper
screenshot docker connection refused
python retry failed http requests
clipboard api key template
dev gestureos
what was I editing Tuesday afternoon
```

Providers compete for relevance. Explicit prefixes/operators remain available for power users but are never required for ordinary use.

## 5. Provider categories

### Search providers

- files/path/name;
- FTS content;
- semantic content;
- applications;
- code symbols;
- PDF pages;
- images/screenshots;
- audio/video moments;
- clipboard history;
- snippets/quicklinks;
- smart collections/workspaces.

### Command providers

- calculator and conversions;
- Windows settings;
- system actions;
- window actions;
- safe shell/process actions;
- workflows.

### Context providers

- recent work;
- workspace context;
- related items;
- Rewind events.

## 6. Universal actions

Every result can expose actions based on its capabilities.

Examples:

| Result | Primary | Secondary examples |
|---|---|---|
| File | Open | Reveal, Copy path, Open with, Pin, Similar |
| PDF page | Open page | Preview, Copy excerpt, Related |
| Code symbol | Open in editor | Copy symbol, Reveal repo, Related code |
| Image | Open | OCR, Copy image, Similar, Related |
| Video moment | Play here | Copy timestamp, Reveal, Similar moment |
| Clipboard entry | Paste/copy | Pin, Delete, Search related |
| App | Launch | Run as admin, Show files |
| Windows setting | Open | — |
| Workflow | Run | Edit, Duplicate |

The action panel is a first-class interaction, not a context-menu afterthought.

## 7. Lumen-specific differentiators

### Semantic Drop

Drop/paste an image, file or text object into the launcher and use it as a query object. "Find things like this" should work without forcing the user to understand embeddings.

### Context Lens

Given one item, show semantically/structurally related local objects: source code, notes, screenshots, PDFs, media, recent activity.

### Semantic Workspaces

Treat a project/topic as an entity rather than a folder. A workspace may aggregate content scattered across folders based on path, usage and semantic relation.

### Rewind

Optional local timeline built primarily from file/app/query/workspace events and timestamps. It is explicitly not required to continuously record the screen. Retention and capture levels are user-controlled.

### Smart Collections

Saved queries that update automatically, e.g. "PDFs on time series modified this month" or "screenshots containing errors".

## 8. Productivity capabilities inspired by mature launchers

- calculator/conversions;
- clipboard history;
- snippets;
- quicklinks;
- search history;
- aliases/keywords;
- workflows;
- app launcher;
- system settings;
- optional window management;
- extension architecture later.

These capabilities must feel native to the root-search model rather than like mini-apps bolted into a dashboard.

## 9. Quality bar

Lumen should be invoked reflexively dozens of times per day.

A feature is weak if it:

- requires navigating settings for ordinary use;
- opens slowly;
- makes the root surface visually busy;
- changes keyboard behavior unpredictably;
- produces jumpy semantic reranking;
- leaks data or surprises the user;
- consumes heavy CPU while idle;
- uses an LLM where a deterministic action is safer/faster.

## 10. Anti-goals

Lumen is not:

- a full file manager replacement;
- an always-open dashboard;
- a chat app with a search box;
- a cloud drive by default;
- a continuous screen recorder by default;
- an Electron app;
- a public plugin platform before the core UX is excellent;
- a generative AI wrapper.

## 11. Success metrics

Product metrics should eventually include:

- hotkey → useful result latency;
- successful query rate without opening Explorer;
- median keystrokes before action;
- repeat daily invocations;
- action-panel usage;
- semantic search success on "remembered meaning" tasks;
- low false-positive rate for command execution;
- indexing completion without foreground disruption;
- retention with core features only/offline.
