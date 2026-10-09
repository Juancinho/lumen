# T213 — visible indexing and measured hybrid image acceleration

## 0. Implementation status

Implemented 2026-10-10 after the user's live zero-image report. ADR-042 records the
scheduling/progress and measured GPU decision. Native real-library verification and
final gates are recorded in the benchmark README and HANDOFF.

## Behavior

- Root search retains ordinary results, selection, actions and keyboard focus. Its footer
  shows the active reading/embedding/paused/waiting phase and CPU/GPU/hybrid device.
- Separate files-read and vectors-ready counts/percentages avoid confusing file discovery
  with semantic coverage. Images show ready/total and queued; skips/errors remain visible.
- Content preparation and vector slices alternate; photos are prepared before each text
  round and get a queue turn within eight text batches when vision is available.
- GPU uses the existing tray opt-in. Image acceleration is admitted only by the isolated
  compatibility/speed probe; CPU vision plus shared GPU backbone preserves the text space.
- All content and progress stay local. IPC contains counts and bounded phase labels,
  no source paths or current filenames. No hidden UI timers/polling.

## Validation

Bounded extraction resumes within a page and preserves cancelled work. Fairness tests
seed image IDs before a large text backlog and prove that every vector is committed.
Coverage tests separate consent scopes, read/skipped/failed and image files. Decoder/UI
tests reject invalid counts, strip extra source fields and keep unknown coverage honest.
GPU image acceptance rejects insufficient speed, low fidelity and invalid timings.
Public-photo release comparisons cover CPU, prior mixed-device and shared-GPU-backbone
cycles and post-image text fidelity. Native UI checks retain root keyboard behavior.
