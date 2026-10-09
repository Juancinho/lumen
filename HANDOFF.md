# HANDOFF.md

> Live continuation only. Read AGENTS.md in order; TASKS.md owns status and Next.
> Rewrite each session. Outcomes and historical evidence belong in state/ADRs/worklog.

## Branch

main on github.com/Juancinho/lumen. This session continues 1859f1e (T206) with T209.
Local commit/push state: inspect git status and git log; no push was requested here.

## Active task / exact continuation

None claimed. T209 is REVIEW (codex, ADR-036); implementation, Windows release evidence
and automated validation are complete. Native code-action checks are below.
Next implementation per TASKS.md: T207 incremental watcher. Do not redo T209 or start
query syntax/editor integration/other milestones as prerequisites without evidence.

T207 must build on the existing inventory identity, content pass, persistent queue,
generation validation and provider contracts. Code metadata now also needs invalidation
on moves/content changes: migration 0004 clears repository/language on a path move, and
lumen_content::code refreshes it during the content pass without replacing unchanged
chunks/vectors. Read its domain spec and touched ADRs before claiming T207.

## T209 — native Windows review remaining

Build from the repository root after quitting any running Lumen instance:

    npm --prefix apps/desktop run build
    cargo build --release -p lumen-desktop --features tauri/custom-protocol --locked
    target\release\lumen.exe

A second launch only addresses the resident instance. Use the tray's Quit Lumen first.
Enable content indexing for a repository under an indexed location; wait for the content
pass. No installed embedding model is required for lexical code context.

1. Search a symbol such as get_with_retry, or exponential backoff python against the
   fixture/code repository. Contents arrive on settle. Rows keep the file identity,
   display symbol + filename when extracted, and show the matched passage. Unnamed code
   chunks keep a file-level row; exact filename navigation keeps its filename presentation.
2. Enter opens the file through its registered handler; Ctrl+Enter selects the file in
   Explorer. Precise editor/line launching is deliberately not implemented (ADR-036).
3. Ctrl+K → Copy symbol → paste into a scratch document: exact extracted symbol.
   Ctrl+K → Reveal repository in Explorer selects the nearest Git root (worktree .git
   marker files are supported). Code outside a known repository offers no repository
   action; an unnamed chunk offers no Copy symbol.
4. Alt+Enter shows the indexed matching passage, including a hit beyond the file's first
   16 KB. Open it before contents settle: it must refresh to the matching code passage on
   the same file ID. Escape closes Action Panel, then preview, then overlay. Selection stays at its
   index through a refinement after arrows/PageDown.
5. Restart: context survives. A path move must clear stale repository/language immediately;
   the next catalog/content pass rediscovers the new root. T207 will remove the periodic
   sync delay. A newly created/deleted repository marker alone is not watched yet.

Migration 0004 preserves chunk IDs/text/vector sequences/generations; no re-embedding.
Existing databases backfill metadata on the next background content pass. Initial schema
upgrade runs on a named startup worker, with first show waiting for schema readiness.
100k synthetic chunks/vectors: 1.10 s upgrade; 1.60 s background metadata/FTS backfill.
Results at 162 documents: content code top-1 0.50→1.00 (six queries); q4 fused NDCG 0.964.
Evidence: docs/benchmarks/t209/2026-10-09-joao-pc/ and README. Synthetic UI screenshot
is in target/t209/code-results-dark.png; temporary browser fixture/server were removed.

Implementation pointers: lumen_core::CodeTarget / Payload::Code / ResultKind::Code;
lumen_catalog::code::enrich (both content and semantic lanes), lumen_storage::code_candidates /
set_code_context / chunk_refs, lumen_content::code::refresh; fusion preserves contextual
actions on ties. Shell DTOs expose display labels only; actions use trusted result payloads.

## Older native REVIEW checks (still pending)

- T003: tray → Keyboard shortcut → Ctrl+Space toggles and persists after restart.
  A shortcut in use is marked and choosing it preserves the previous shortcut.
- T004/T103: Acrylic vs Mica default verdict; compare tray → Window material, light/dark,
  100/125/150% DPI, long paths, native corners/shadow, no flash and text over busy backgrounds.
  Transparency effects off/high contrast must use Solid on next show. Existing dark
  measurements are accepted; optional light-mode script:

      powershell -ExecutionPolicy Bypass -File scripts\t004\run-windows-material.ps1

- T107/T104/T108/T109/T105/T110: first catalog appears within seconds; per-keystroke name/app
  results; arrows/PageUp/PageDown/Ctrl+L/IME; Enter launches/opens, Ctrl+Enter reveals, Ctrl+K
  copies a path, repeated choices rise; Alt+Enter metadata/text preview, search bar fixed,
  Escape order. LUMEN_DIAGNOSTICS=1 adds provider/match/confidence and local timing logs.
- T111: tray → Indexed locations → Add D:\Proyectos, results appear without restart;
  Remove clears them. USB unplug keeps results and reports unavailable; replug recovers.
  Toggle node_modules exclusion; build is excluded beside Cargo.toml but otherwise kept.
  Folder → Ctrl+K → Exclude folder, then tray → Include again; restart retains configuration.

      powershell -ExecutionPolicy Bypass -File scripts\t111\run-windows-locations.ps1 -Drive D:\

- T202: tray content progress, remembered Pause, per-location content off, CPU share/idle/
  battery policy. With the T006 cache, quit the resident app before the launch check:

      powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Root D:\Proyectos\lumen
      powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Launch -SkipBench

- T205/T206: installed model + descriptive search yields contents/meaning after settle;
  move selection immediately with ↓ twice: selected row must retain its position, snippet
  replaces location, hover/Quick Look retain context. Diagnostics show lumen.content /
  lumen.semantic. Ordinary queries still work offline before installing a model.
- T210: unset development model/runtime overrides. Tray → Semantic search → Download…
  shows consent (222 MB, huggingface.co/files.pythonhosted.org, Apache-2.0/MIT), progress,
  cancel/resume, atomic installed status and indexing/query availability without restart.
  Installed files are under app data models/ and runtime/; Remove… deletes the model.
  No About/licenses screen, WinHTTP transport or installer packaging is built. For proxy
  checks system curl uses HTTPS_PROXY; do not initiate downloads without the tray consent.

## Other pending Windows evidence (not run by this T209 session)

T014 (plugged in/idle, first download ~3 GB, 20–40 min):

    powershell -ExecutionPolicy Bypass -File scripts\t014\run-windows-throughput.ps1 -Download

Commit counts/timings under docs/benchmarks/t014/<date>-joao-pc/. Choose runtime/thread
cap in a new ADR with the evidence, then amend ADR-015 and apply it to T202. Failed rows
are recorded; do not add LiteRT-LM until a Windows runtime exists.

T204 (T006 cache, ~10 min; accept ADR-030 if with_indexing_preempted -b1 p95 ≤80 ms):

    powershell -ExecutionPolicy Bypass -File scripts\t204\run-windows-query-lane.ps1

T203 (optional large ANN timing, no model):

    powershell -ExecutionPolicy Bypass -File scripts\t203\run-windows-ann-gen.ps1 -Large

T205 (original fixture model evaluation; T209 now has separate Windows hard-set evidence):

    powershell -ExecutionPolicy Bypass -File scripts\t205\run-windows-eval.ps1

Reports belong in each task's docs/benchmarks/<task>/<date>-joao-pc/ folder. Current
T006 cache: .cache/t006/{ort-cpu,ort-dml,embeddinggemma-2-ONNX}. Environment overrides
LUMEN_EMBED_MODEL_DIR / LUMEN_ORT_DYLIB still take precedence over provisioned assets.
After queue slices, ANN files live under app-data vectors/ (first build around 2,000 chunks).

## Validation and reproduction

Run from repository root unless a frontend prefix is shown:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo clippy -p lumen-bench --features directml --all-targets --locked -- -D warnings
    cargo xtask test --locked
    cargo xtask arch
    npm --prefix apps/desktop run check
    npm --prefix apps/desktop run build
    cargo build --release -p lumen-desktop --features tauri/custom-protocol --locked

T209 release evidence, cached model only (no downloads):

    cargo run --release -p lumen-bench --features ort -- eval --fixture fixtures/eval-hard --json target/bench/t209-mock.json
    target\release\lumen-bench.exe eval --backend ort --model-dir .cache/t006/embeddinggemma-2-ONNX --ort-dylib .cache/t006/ort-cpu/onnxruntime.dll --variant q4 --threads 6 --fixture fixtures/eval-hard --json target/bench/t209-q4.json
    cargo run --release -p lumen-storage --example code_upgrade -- 100000 target/bench/t209-upgrade.json

The T209 Windows gate fixes also isolate usage-test temp directories, close a mapped
index before a missing-file test deletes it, and remove two Windows-only unused
qualifications. They are validation prerequisites, not changes to ranking/runtime policy.
Final automated gates passed on Windows, including 71 frontend tests and the release
desktop build; same-ID preview refinements and late replies have a regression test.

## Remaining decisions / environment notes

Default material Acrylic vs Mica is the user's visual verdict; T014 runtime/device
verdict and ADR-030 Windows timing remain open. Precise editor transport, log/data
two_of tuning (ADR-033), q4/fp32 at scale and TX01 remain outside T209.
Linux WebKitGTK minimum height and D-Bus needs are development artifacts.
Old .git/stale-locks/ came from previous sandbox sync; inspect any current lock before
changing it. CI should be pushed/run when authorized; this session does not push.
