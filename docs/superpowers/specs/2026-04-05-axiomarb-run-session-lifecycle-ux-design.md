# AxiomArb Run Session Lifecycle UX Design

- Date: 2026-04-05
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, the operator-facing command surface is substantially stronger than it was earlier in the project:

- `app-live bootstrap` covers Day 0 / first-run setup
- `app-live status` provides high-level readiness
- `app-live apply` provides Day 1+ smoke orchestration
- `app-live doctor` provides venue-facing preflight
- `app-live run` remains the runtime entrypoint
- `app-live verify` provides high-level local outcome checks

The main remaining lifecycle gap is that the system still lacks a durable truth source for:

> Which specific run produced these results, under which startup intent, and what lifecycle state did that run reach?

That gap causes several downstream UX weaknesses:

- `verify` must still degrade for historical windows because it cannot prove those results belong to the same startup intent as the current config
- `status` can only infer recent lifecycle state from runtime progress and local evidence, rather than reading a dedicated run lifecycle record
- future improvements such as stronger latest-run verification, `apply` expansion beyond smoke, and more trustworthy restart guidance remain constrained

The recommended next subproject is:

- introduce a durable `run_session` lifecycle truth source

The recommended architecture is:

- only `app-live run` creates and closes `run_session`
- `run_session` stores explicit lifecycle state plus a de-sensitized startup snapshot
- high-value local result surfaces attach `run_session_id`
- `status` and `verify` default to the latest relevant session for the current config/mode
- historical explicit windows only receive strong config/lifecycle interpretation if they can be uniquely mapped to a session

This is not a process supervisor and not a new startup authority.

It is a durable lifecycle truth source that makes the existing high-level UX more trustworthy.

## 2. Current Repository Reality

At current `HEAD`, the repository already has most of the operator primitives needed for a clean high-level workflow, but lifecycle truth is still fragmented.

### 2.1 `run` is the runtime authority, but not a durable session authority

`app-live run` is the foreground, long-lived daemon entrypoint.

It already owns:

- runtime startup
- active runtime truth
- runtime progress writes

But it does not yet create a durable, operator-visible `run_session` record that can be used later by `status`, `verify`, or future high-level orchestration.

### 2.2 `verify` already exposes the missing lifecycle gap

`verify` now provides a high-level local result contract, but it still has to be conservative for older windows because:

- there is no durable run session identity
- there is no durable startup snapshot owned by a specific run
- historical evidence can therefore be summarized, but not always strongly interpreted against startup intent

### 2.3 `status` already has readiness truth, but not full lifecycle truth

`status` can already explain:

- mode
- target adoption state
- rollout readiness
- configured-vs-active drift
- restart-required state

But it still lacks a dedicated lifecycle anchor for:

- what the latest relevant run actually was
- whether that run is still truly alive or only appears active
- which run produced the results later seen by `verify`

### 2.4 `apply` and future orchestration are blocked by missing lifecycle truth

`apply` now improves Day 1+ smoke UX, but more ambitious orchestration remains blocked without session truth:

- stronger latest-run verification
- safer history-aware verification
- future `apply` expansion beyond smoke
- more trustworthy controlled-restart guidance

The missing piece is therefore not another command.

The missing piece is a durable lifecycle model shared by `run`, `status`, and `verify`.

## 3. Goals

This design should guarantee the following:

- introduce a durable `run_session` record
- make `run` the sole creator and closer of that session
- persist explicit lifecycle state:
  - `starting`
  - `running`
  - `exited`
  - `failed`
  - `stale`
- persist a de-sensitized startup snapshot per session
- attach `run_session_id` to high-value local result surfaces
- let `status` explain the latest relevant session for the current config/mode
- let `verify` use session truth by default for latest-run verification
- let historical explicit windows degrade safely when they cannot be uniquely mapped to a session
- make `run_session_id` operator-visible without making it the primary UX concept

## 4. Non-Goals

This design does not define:

- detached or background process supervision
- a full session event log in the first phase
- `apply --start --verify` chaining in the first phase
- raw config blob persistence
- secret persistence in session snapshots
- full-database backfill of historical rows
- universal `run_session_id` attachment across every table
- new startup authority beyond the existing runtime/config truth
- high-level productization of legacy explicit-target startup

## 5. Architecture Decision

### 5.1 Recommended Approach

Add a dedicated durable `run_session` record as the lifecycle truth source.

Hard rules:

- only `app-live run` creates a `run_session`
- only `run` advances its lifecycle state
- high-level commands such as `bootstrap --start` and `apply --start` only pass `invoked_by` context
- `run_session` records lifecycle truth; it does not replace runtime progress or operator config
- `run_session` must not become a second startup authority
- `run_session` must not store secrets or raw config blobs

### 5.2 Why A Dedicated Session Record Is Better Than Extending Existing Progress

Reusing only runtime progress plus journal/attempt stitching keeps lifecycle semantics too loose:

- progress is optimized for runtime state, not session identity
- journal windows are useful evidence, but weak lifecycle authority
- `verify` would still need large amounts of heuristic matching

A dedicated session record provides:

- one authoritative lifecycle object
- a stable operator-visible handle
- a durable snapshot of startup intent
- a clean join point for high-value result surfaces

### 5.3 Why `run` Must Own Session Creation

If `bootstrap` or `apply` can create sessions independently, lifecycle authority immediately splits.

That would create multiple bad outcomes:

- session existence would no longer mean “a real `run` was attempted”
- orchestration commands could accidentally invent lifecycle truth before runtime had actually started
- `status` and `verify` would have to reconcile multiple competing sources

`run` must therefore remain the sole lifecycle writer.

## 6. Data Model

### 6.1 `run_session`

The first version should introduce a durable `run_session` record with at least these conceptual fields:

- `run_session_id`
- `invoked_by`
- `mode`
- `state`
- `started_at`
- `last_seen_at`
- `ended_at`
- `exit_status`
- `exit_reason`

### 6.2 De-Sensitized Startup Snapshot

Each session should also persist a normalized startup snapshot that is strong enough for later interpretation without storing raw config or secrets:

- `config_path`
- `config_fingerprint`
- `target_source_kind`
- `configured_operator_target_revision`
- `active_operator_target_revision_at_start`
- `rollout_state_at_start`
- `real_user_shadow_smoke`

This snapshot exists to support:

- trustworthy latest-run interpretation
- historical session-aware verification
- operator-readable lifecycle debugging

### 6.3 `invoked_by`

The first version should treat `invoked_by` as contextual metadata only.

Useful values include:

- `run`
- `bootstrap`
- `apply`

It does not create authority.

It only explains how the session was initiated.

### 6.4 Session Identifier

`run_session_id` should be a stable opaque identifier suitable for operator copy/paste and CLI display.

The spec does not require a specific encoding beyond:

- globally unique
- stable across reads
- easy to surface in CLI output

## 7. Lifecycle State Machine

### 7.1 `starting`

`run` should create `run_session(state=starting)` as soon as it has enough context to begin startup and persist the session record.

This means:

- a run attempt really exists
- but runtime startup is not yet considered complete

### 7.2 `running`

The session should move from `starting` to `running` only after startup-critical work has completed.

The exact code boundary should align with runtime reality, but conceptually it must mean:

- config was accepted
- startup target resolution succeeded
- runtime/supervisor main loop was established
- initial runtime truth was durable enough for the session to represent a real running instance

### 7.3 `exited`

If the foreground runtime ends normally, the session should move to `exited` and write:

- `ended_at`
- `exit_status`
- optional `exit_reason`

### 7.4 `failed`

If startup or runtime terminates due to a recognized failure path, the session should move to `failed` and record:

- `ended_at`
- `exit_status`
- `exit_reason`

### 7.5 `stale`

The first version should use heartbeat-style freshness rather than a full supervisor:

- `run` periodically updates `last_seen_at`
- readers treat a stale heartbeat as a stale/abandoned session

The durable row may still physically hold `state=running` until a compensating path tightens it.

What matters for the high-level UX is that:

- `status`
- `verify`

consistently interpret an overdue session as stale rather than pretending it is still healthy.

## 8. Result Linkage Strategy

### 8.1 High-Value Result Surfaces Only

The first version should attach `run_session_id` only to high-value local result surfaces:

- runtime progress / runtime truth surfaces
- neg-risk execution attempts
- the key artifacts or summaries needed by `verify`

### 8.2 Why This Scope Is Correct

This is enough to unlock the UX goals that actually matter now:

- `status` can explain the latest relevant session
- `verify latest run` can stop depending on weak time-window heuristics
- future orchestration can reason about a specific run instance

At the same time, it avoids turning the first version into:

- a full data-model rewrite
- a global session-backfill project
- a mandatory session foreign key on every low-level table

### 8.3 Journal And Other Low-Level Streams

Low-level evidence streams such as journal windows may still exist without direct `run_session_id` linkage in the first version.

They remain useful as supplementary evidence and fallback range inputs.

They just stop being the strongest lifecycle authority.

## 9. Default Read Semantics

### 9.1 Latest Relevant Session

`status` and `verify` should not simply choose “the most recent row”.

They should choose the latest relevant session for the current config/mode.

Relevance should be determined from the session snapshot, especially:

- `mode`
- `config_path`
- `config_fingerprint`
- `configured_operator_target_revision`
- `rollout_state_at_start`

### 9.2 Selection Priority

Within that relevant set, readers should prefer:

1. a currently `running` session
2. the most recent completed `exited` or `failed` session
3. the most recent stale session

This matches operator intent better than a raw “latest row” heuristic.

### 9.3 `status`

`status` should use that latest relevant session to improve answers such as:

- is there a real current running session for this startup intent
- what was the latest relevant run for this config
- is the current lifecycle truth fresh, finished, or stale

### 9.4 `verify`

`verify` should also default to the latest relevant session.

That lets it produce a stronger latest-run verdict without pretending that arbitrary local evidence always belongs to the current config.

## 10. Historical Range Handling

Explicit historical windows remain supported:

- `--attempt-id`
- `--since`
- `--from-seq`
- `--to-seq`

But the first version must be conservative.

### 10.1 Strong Interpretation Rule

If an explicit range can be uniquely resolved to exactly one `run_session`, `verify` may use that session snapshot and lifecycle context for strong interpretation.

### 10.2 Safe Degradation Rule

If an explicit range:

- maps to multiple sessions
- maps to no session
- or cannot be uniquely explained

then `verify` must degrade to evidence-only behavior.

That means:

- summarize what was observed
- still perform forbidden side-effect checks
- do not claim strong config/lifecycle consistency

This preserves the current safety principle:

- when the system cannot prove the lifecycle context, it must not guess.

## 11. Failure Handling

### 11.1 Session Write Failure

If `run` cannot create or update its `run_session`, the run should fail.

Lifecycle truth is too fundamental to silently drop.

### 11.2 Missing Or Partial Snapshot

If readers encounter a session missing key snapshot fields, they should:

- mark interpretation as degraded
- avoid strong lifecycle/config claims
- continue only as far as the evidence still supports

### 11.3 Stale Session Interpretation

If `last_seen_at` exceeds the stale threshold:

- `status` should not present the session as currently healthy
- `verify` should not treat it as an unquestioned current runtime

### 11.4 No Historical Backfill Requirement

The first version does not require complete historical backfill.

Older rows without `run_session_id` linkage remain usable as local evidence, but not as strong lifecycle truth.

That is an intentional and acceptable first-phase limitation.

## 12. Operator Visibility

`run_session_id` should be operator-visible in:

- `run` startup summaries
- `status`
- `verify`

But it should not become the primary UX concept.

The output priority remains:

1. high-level conclusion
2. supporting context
3. stable session handle

This keeps the UX approachable while still giving operators and developers a durable handle for debugging.

## 13. Testing Boundaries

The first implementation should cover:

### 13.1 Lifecycle State Tests

- `starting` creation
- `starting -> running` promotion
- `running -> exited`
- `running/starting -> failed`
- stale interpretation from `last_seen_at`

### 13.2 Snapshot Tests

- normalized startup snapshot is written
- secrets are not persisted
- session ownership remains on `run`

### 13.3 Result-Linkage Tests

- runtime progress can be tied to a session
- neg-risk attempts can be tied to a session
- verify-critical artifacts/summaries can be tied to a session

### 13.4 Reader Tests

- `status` picks the correct latest relevant session
- `verify` picks the correct latest relevant session
- historical ranges degrade safely when session mapping is ambiguous

### 13.5 Failure Tests

- run fails if session creation fails
- stale sessions are interpreted as stale
- missing linkage degrades rather than fabricates strong truth

## 14. Acceptance Criteria

This subproject is complete when:

- a durable `run_session` record exists
- only `run` creates and closes it
- the session stores explicit lifecycle state and a de-sensitized startup snapshot
- high-value local result surfaces attach `run_session_id`
- `status` can explain the latest relevant session for the current config/mode
- `verify` can default to latest relevant session for stronger latest-run interpretation
- explicit historical ranges degrade safely when unique session mapping is unavailable
- `run_session_id` is operator-visible but remains a secondary handle

## 15. Recommended Next Step After This Spec

Once `run_session` exists, the next high-leverage UX improvements become much safer:

- expand `apply` beyond smoke
- strengthen `verify latest run`
- revisit optional high-level chaining such as `apply --start --verify`
- improve controlled-restart guidance with actual lifecycle truth

The durable run-session truth source should therefore be treated as foundational infrastructure for the next UX phase, not as an isolated data-model cleanup.
