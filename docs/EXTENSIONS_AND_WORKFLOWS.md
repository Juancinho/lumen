# EXTENSIONS_AND_WORKFLOWS.md

## 1. Philosophy

Lumen needs composability early and a public extension platform late.

Do:

- model providers/actions/workflows cleanly;
- keep capabilities typed;
- keep permission boundaries explicit;
- make built-ins use similar concepts to future extensions where sensible.

Do not:

- freeze a public SDK in M0;
- load arbitrary third-party DLLs in-process;
- build a plugin marketplace before core search/action UX is excellent.

## 2. Workflow model

A workflow is a validated sequence/graph of known actions with parameters.

V1 workflow engine should start sequential:

```text
trigger/query
  ↓
action 1
  ↓
action 2
  ↓
action 3
```

Only add branching/loops when real use cases justify complexity.

Each step defines:

- action ID/version;
- typed inputs;
- parameter bindings;
- required capabilities;
- failure policy;
- output bindings if needed.

## 3. Example workflow

```yaml
name: dev GestureOS
arguments: []
steps:
  - action: editor.open_folder
    path: D:/Projects/GestureOS
  - action: terminal.open
    cwd: D:/Projects/GestureOS
  - action: shell.run_confirmed
    command: uv run app.py
  - action: windows.arrange
    preset: dev-main
```

Exact format is not final. Avoid committing to YAML syntax before task T501.

## 4. Permissions

Capabilities may include:

- filesystem.read
- filesystem.write
- process.launch
- shell.execute
- clipboard.read
- clipboard.write
- network.request
- windows.control
- system.settings

Workflows display required capabilities before first run when nontrivial.

## 5. Future extension host

Candidate isolation approaches to evaluate later:

- subprocess with versioned IPC;
- WASM/WASI for constrained providers;
- signed native extension with strict policy only if necessary.

No decision now. T803 owns the spike.

## 6. Extension quality contract

Future extensions must declare:

- commands/providers/actions;
- permissions;
- network domains if applicable;
- settings schema;
- performance class;
- icon/metadata;
- minimum API version.

Lumen may throttle/disable extensions that exceed latency or resource budgets.
