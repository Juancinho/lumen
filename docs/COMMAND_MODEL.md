# COMMAND_MODEL.md — root query, results and actions

## 0. Implementation status

Canonical code (T011): `crates/lumen-core/src/` — `ids.rs`, `result.rs`, `capability.rs`,
`action.rs`, `contract.rs`, `execution.rs`. The code is authoritative where it differs from the
conceptual sketches below. Notable concrete choices:

- `ResultId` is the identity of the **entity**, equal across providers/batches (merge + selection
  stability); convention `<kind>:<stable key>`.
- `ProviderId`/`ActionId` are namespaced names (`lumen.files`, `lumen.reveal`), compile-time
  validated for built-ins; `lumen.` is reserved for built-ins.
- `Score` = provider-normalized `Confidence` ∈ [0,1] + `MatchKind`; raw scores stay inside providers.
- `CapabilitySet` (what a target supports) is distinct from future workflow *permissions* (T501).
- `primary_action` + `secondary_actions`; `validate_result` rejects destructive/privileged primaries,
  unknown/duplicate actions and missing capabilities.
- `ExecutionContext::authorize` turns an id-only `ActionRequest` into an authorized context:
  action must be offered by the result, Enter runs only the primary, destructive/privileged need
  explicit confirmation.
- `Payload` never crosses to the UI (ADR-013). `PreviewRef` is deferred to T105.

## 1. Mental model

There is one root search. Providers propose results; a coordinator normalizes/fuses/ranks them; each result advertises valid actions.

The user does not need to know provider names.

## 2. Canonical result

Conceptual domain type:

```rust
struct ResultItem {
    id: ResultId,
    provider: ProviderId,
    kind: ResultKind,
    title: String,
    subtitle: Option<String>,
    tertiary: Option<String>,
    icon: IconRef,
    preview: Option<PreviewRef>,
    score: ScoreBreakdown,
    capabilities: CapabilitySet,
    action_ids: Vec<ActionId>,
    payload_ref: PayloadRef,
}
```

Do not send arbitrary provider-specific JSON blobs to React as the primary contract. Provider-specific details can use a versioned typed extension field only where needed.

## 3. Provider contract

An internal provider should conceptually expose:

- identity and supported intents;
- fast synchronous/async candidate path;
- cancellation;
- query/context inputs;
- result normalization;
- optional actions;
- optional preview support;
- performance class (`instant`, `fast`, `deferred`).

Not every provider participates on every keystroke. Provider routing must avoid expensive work when query intent makes it irrelevant.

## 4. Progressive result phases

Possible phases:

1. instant exact/app/name results;
2. fast FTS/calculator/settings/snippets;
3. semantic refinement;
4. deferred context/media where explicitly useful.

The UI should not display "phase" concepts unless diagnostics are enabled.

## 5. Primary action

Every result has exactly one primary action chosen to match expected intent:

- file → open;
- app → launch;
- calculator → copy result;
- setting → open setting;
- workflow → run;
- clipboard → paste/copy according to invocation context.

Enter executes primary action. Destructive primary actions are prohibited.

## 6. Action panel

`Ctrl+K` is the canonical action-panel shortcut unless usability testing selects another. Tab may be used for autocomplete/secondary behavior; do not overload it inconsistently.

Action panel must support search/filter of actions when list is long.

Action ordering:

1. primary/default;
2. common safe contextual actions;
3. navigation/copy;
4. advanced actions;
5. destructive actions separated and visually explicit.

## 7. Query syntax

Natural language is default. Explicit operators are stable power-user features.

Reserved initial filters:

- `type:`
- `ext:`
- `in:`
- `before:`
- `after:`

Quoted strings request stronger lexical/exact behavior.

Provider-specific sigils/prefixes should be added only when they create substantial value and never be required for discoverability.

## 8. Ranking

The coordinator ranks across heterogeneous providers using normalized confidence plus strong intent rules. Never compare raw provider scores directly unless calibrated.

Ranking signals may include:

- exactness;
- lexical relevance;
- semantic relevance;
- recency;
- frequency;
- pin/favorite;
- active workspace/project;
- query/provider intent;
- explicit filters.

Learned ranking, if introduced, stays local and must be resettable/explainable at a basic level.

## 9. Safety

Search is cheap and reversible. Actions may not be.

Classify actions:

- `safe_read`
- `safe_reversible`
- `privileged`
- `destructive`
- `external_data`

Privileged/destructive/external-data actions require the permission/confirmation policy in `PRIVACY_SECURITY.md`.
