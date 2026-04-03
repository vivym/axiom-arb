# Verify UX Design

## Problem

The operator startup path now has high-level commands for configuration, readiness, and startup:

- `bootstrap`
- `status`
- `doctor`
- `run`

What is still missing is a single high-level command that answers the operator question that comes **after** `run`:

> Did the latest run produce the result that this mode and config were supposed to produce?

Today that answer still requires manual interpretation of:

- execution attempts
- shadow/live mode distribution
- shadow/live artifacts
- replay-visible output
- control-plane context such as configured-vs-active drift and rollout readiness

This is especially painful for `real-user shadow smoke`, where the operator still falls back to SQL and `app-replay` to decide whether the smoke actually behaved correctly.

## Goals

- Add a high-level `app-live verify` command.
- Cover `paper`, `live`, and `real-user shadow smoke`.
- Keep `verify` strictly local:
  - config
  - durable control-plane/runtime state
  - execution attempts
  - artifacts
  - replay-visible local evidence
- Keep `verify` completely separate from `doctor`:
  - `doctor` remains the venue-facing preflight gate
  - `verify` remains the local run-outcome validator
- Make the default operator experience high-level:
  - no SQL required
  - no journal-seq reasoning required
  - a verdict plus next action is always returned

## Non-Goals

- No venue probes.
- No relayer, websocket, heartbeat, or authenticated REST checks.
- No control-plane mutation.
- No target adoption.
- No rollout mutation.
- No replay pipeline redesign.
- No attempt to certify “production trading success.”

## Top-Level Contract

`verify` is a high-level result command:

```bash
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

The command returns:

1. `Scenario`
2. `Verdict`
3. `Result Evidence`
4. `Control-Plane Context`
5. `Next Actions`

The command never runs venue-facing checks and never mutates runtime or config state.

## Verdict Model

`verify` uses exactly three verdicts:

- `PASS`
- `PASS WITH WARNINGS`
- `FAIL`

### Scenario

The scenario is always reported explicitly:

- `paper`
- `live`
- `real-user shadow smoke`

### Expectations

The first version supports a small fixed set of high-level expectations:

- `auto`
- `paper-no-live`
- `smoke-shadow-only`
- `live-config-consistent`

`auto` derives the expectation from the current config and readiness context.

## Evidence Sources

The first version of `verify` may use only local evidence:

- the operator config file
- durable control-plane state
- runtime progress / configured-vs-active state
- execution attempts
- artifacts
- replay-visible local summaries

`verify` must not contact external systems.

## Default Window Resolution

The architectural target is a durable run-session identity.

That does **not** exist yet as a stable high-level operator contract, so the first version must use fallback window resolution.

### Design Rule

- Long term: `verify` should prefer run-session identity.
- First version: `verify` may infer the latest relevant local evidence window from journal / execution-attempt state.

### Explicit Range Overrides

The first version supports:

- `--from-seq`
- `--to-seq`
- `--attempt-id`
- `--since`

### Critical Boundary

The first version must **not** pretend that an explicitly historical window can be judged against the current config as if the current config were a durable snapshot of that older run.

Therefore:

- the **default latest-run path** may produce config-consistency verdicts
- an **explicit historical range** may produce evidence summaries and side-effect checks
- but it must not produce a strong `live-config-consistent` / `smoke-shadow-only` verdict that depends on the current config unless the verified window is provably tied to the current config anchor

If the implementation cannot prove that tie, it must degrade to:

- evidence summary only
- or `PASS WITH WARNINGS`
- or `FAIL`

It must not silently compare old evidence to current config.

## Result Evidence

The first version of `verify` summarizes:

- execution attempt presence
- execution attempt count
- route distribution
- execution mode distribution
- artifact presence
- artifact/attempt consistency
- replay-visible evidence
- forbidden side effects

### Attempt Evidence

At minimum:

- whether any attempts exist in the resolved window
- how many attempts exist
- whether the attempts are `shadow`, `live`, or mixed
- whether `neg-risk` work appears where expected

### Artifact Evidence

At minimum:

- whether relevant artifacts exist
- whether artifacts align with attempts
- whether shadow/live artifacts match the observed attempt modes

### Forbidden Side Effects

This is a hard requirement in the first version.

At minimum:

- `paper-no-live`
  - must not observe `live` attempts
- `smoke-shadow-only`
  - must not observe `live` attempts
- `live-config-consistent`
  - must not observe obviously contradictory local outcomes relative to the current high-level mode and readiness interpretation

## Replay Evidence

Replay evidence is important, but its weight is mode-specific.

### Smoke

For `real-user shadow smoke`, replay-visible `neg-risk shadow smoke` output is a **strong** evidence source because the current local replay surface already exposes a dedicated shadow-smoke summary.

That makes replay a first-class strengthening signal for smoke verification.

### Live

For `live`, replay output is only a **supplementary** evidence source in the first version.

Current replay surfaces are useful for context and summary, but they are not yet a strong enough mode-specific live-verification contract to act as a decisive verdict source by themselves.

### Paper

For `paper`, replay is also only **supplementary**.

The first version must not pretend that paper has an equivalent replay-backed outcome contract to smoke.

### Rule

Therefore the first version must not describe replay as a uniform “strong evidence” layer across all modes.

Instead:

- smoke: replay is strong evidence
- live: replay is supplementary evidence
- paper: replay is supplementary evidence

## Control-Plane Context

The first version exposes a minimal, fixed context set:

- `mode`
- `target_source`
- `configured_operator_target_revision`
- `active_operator_target_revision`
- `restart_needed`
- `rollout_state`

This context is used to explain verdicts, not to recreate all lower-level provenance views.

If deeper provenance is needed, the operator still falls back to:

- `app-live targets status`
- `app-live targets show-current`

## Auto Expectation Derivation

### Paper

`auto` derives to:

- `paper-no-live`

But paper verification must be scoped conservatively.

The first version of paper verification only promises:

- no forbidden live-side effects
- basic local run evidence when such evidence exists

It does **not** claim a rich paper-specific outcome model parallel to smoke.

### Real-User Shadow Smoke

`auto` derives to:

- `smoke-shadow-only`

But the meaning of “success” must be split carefully:

- a smoke run that produced valid shadow evidence may `PASS`
- a smoke run that executed but intentionally produced no shadow work because rollout was not ready may `PASS WITH WARNINGS`
- a smoke flow that never actually ran at all must **not** be reported as a PASS variant

This is a critical boundary:

- `preflight-ready smoke startup` is a valid startup state
- it is **not** by itself a verified run result

Therefore:

- no run evidence -> not a PASS variant
- real run evidence + no shadow work + rollout not ready -> `PASS WITH WARNINGS`

### Live

`auto` derives to:

- `live-config-consistent`

But the first version of live verification is intentionally conservative:

- it verifies consistency with current config/control-plane expectations
- it does not certify end-to-end live trading success

## Readiness Interaction

`verify` uses `status`-style readiness context, but it does not replace `status`.

### Key Rule

`status` answers:

> What should I do next before or around startup?

`verify` answers:

> Given a completed or attempted run window, did the local result match what should have happened?

This distinction must stay sharp.

## Explicit-Target Legacy Path

High-level UX already treats explicit targets as unsupported in `status`.

The first version of `verify` must keep the same boundary:

- do not re-productize explicit targets in the high-level flow
- if explicit targets are detected, return an unsupported/legacy result
- point operators back to adopted-target startup or lower-level commands

## Mode-Specific Verdict Rules

### Paper

`PASS`:

- local run evidence exists or the basic paper evidence is otherwise sufficient
- no forbidden live-side effects are observed

`PASS WITH WARNINGS`:

- basic paper evidence is incomplete
- but no forbidden live-side effects are observed

`FAIL`:

- live-side evidence appears
- or the local evidence window is not credible enough to support even a weak paper result

### Real-User Shadow Smoke

`PASS`:

- shadow evidence exists
- forbidden live-side effects do not exist
- shadow artifacts align
- replay-visible smoke evidence exists or the remaining evidence is otherwise strong enough

`PASS WITH WARNINGS`:

- a real run happened
- no forbidden live-side effects occurred
- but rollout-not-ready or missing strong replay evidence makes the result incomplete

`FAIL`:

- live-side evidence exists
- artifacts and attempts are inconsistent
- or no credible run evidence exists at all

### Live

`PASS`:

- local result evidence is consistent with current config/control-plane expectations
- no obviously contradictory side effects are observed

`PASS WITH WARNINGS`:

- local result evidence is mostly consistent
- but supporting evidence is incomplete

`FAIL`:

- local result evidence is clearly inconsistent with current mode/readiness expectations
- or evidence is too broken to support a credible consistency verdict

## Next Actions

`Next Actions` are derived from:

- verdict
- scenario
- readiness context

They are not generic fixed templates.

Examples:

- `PASS`
  - no action required
  - continue with normal operations

- `PASS WITH WARNINGS`
  - rerun with rollout enabled
  - run `app-replay` for stronger smoke evidence
  - perform controlled restart, then rerun `verify`

- `FAIL`
  - rerun `status`
  - rerun `doctor`
  - inspect `targets ...`
  - stop and inspect the latest attempts/artifacts before retrying

## Output Style

The first version should be moderately detailed.

It should not be:

- a one-line verdict
- or a long sectioned report as heavy as `doctor`

It should show enough evidence that the operator does not need SQL in the common path.

## Testing Requirements

The first version must test at least:

- default expectation derivation for `paper`, `live`, and `smoke`
- explicit `--expect` profile selection
- explicit range override behavior
- the “historical range cannot be judged against current config” boundary
- smoke `PASS WITH WARNINGS` only when real run evidence exists
- paper verification remaining conservative
- replay weighting by mode
- explicit-target legacy handling

## Acceptance Criteria

- `app-live verify` exists as a new high-level command
- it covers `paper`, `live`, and `real-user shadow smoke`
- it performs no venue probes
- it uses only local evidence
- it supports `auto`, `paper-no-live`, `smoke-shadow-only`, and `live-config-consistent`
- it supports `--from-seq`, `--to-seq`, `--attempt-id`, and `--since`
- it does not over-claim paper verification
- it does not over-claim live verification
- it does not allow preflight-only smoke with zero run evidence to appear as a PASS variant
- it keeps explicit-target legacy flow out of the high-level UX

## Summary

`app-live verify` should become the high-level post-run validation command that follows `bootstrap`, `status`, `doctor`, and `run`. It must stay local-only, mode-aware, and conservative. The first version should prefer the latest relevant run window, allow explicit local evidence ranges, and return a compact but operator-usable result: `Scenario`, `Verdict`, `Result Evidence`, `Control-Plane Context`, and `Next Actions`. The command must distinguish real verified run results from mere startup readiness, must treat paper and live conservatively, and must use replay evidence with mode-specific weight rather than pretending replay means the same thing everywhere. Its purpose is not to certify trading success. Its purpose is to let an operator answer, with one command, whether the latest local run behaved the way the current high-level mode and control-plane state said it should.
