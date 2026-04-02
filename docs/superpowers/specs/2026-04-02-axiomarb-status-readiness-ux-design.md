# AxiomArb Status And Readiness UX Design

- Date: 2026-04-02
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `AxiomArb` now has a much stronger operator UX than it had earlier in the project:

- `app-live init` is an interactive config wizard
- `app-live doctor` is a real venue-facing preflight
- `app-live run` is the runtime entrypoint
- `app-live bootstrap` is the preferred high-level startup path for `paper` and `real-user shadow smoke`
- `app-live targets ...` provides first-class target adoption and rollback controls

That is a meaningful improvement, but the operator still has to infer too much system state from multiple surfaces:

- `bootstrap`
- `doctor`
- `targets status`
- `targets show-current`
- rollout configuration
- durable runtime progress

The next UX-focused subproject should add a single high-level readiness surface:

- `app-live status`

This command should not be another preflight.

It should be a high-level state aggregator that answers:

- what mode am I in
- what startup authority is configured
- whether the configured target is already active
- whether rollout is ready
- whether a restart is required
- what the next operator action should be

This status surface should become the config-and-control-plane readiness truth source that later `verify` and `bootstrap v2` can both reuse.

## 2. Current Repository Reality

At current `HEAD`, repository state is already spread across several useful but separate surfaces:

1. Config truth
- operator intent lives in a single TOML file
- `DATABASE_URL` remains the only deployment env var

2. Control-plane truth
- startup authority is `[negrisk.target_source].operator_target_revision`
- `targets adopt` and `targets rollback` rewrite that value and record adoption history

3. Runtime truth
- `targets status` and `targets show-current` already distinguish:
  - configured operator target revision
  - active operator target revision
  - restart needed

4. Rollout truth
- actual `neg-risk` live or shadow work still depends on rollout readiness
- adopted target presence alone does not mean the daemon will request work

5. Preflight truth
- `doctor` already performs:
  - config validation
  - credential checks
  - venue probes
  - startup target resolution
  - configured-vs-active checks
  - mode-scoped runtime safety checks

The remaining UX gap is not missing backend capability.

It is that the operator still has to mentally merge multiple lower-level views into one answer to:

- am I ready
- if not, what is missing
- what do I do next

## 3. Goals

This design should guarantee the following:

- add `app-live status` as a high-level readiness entrypoint
- cover:
  - `paper`
  - `live`
  - `real-user shadow smoke`
- keep `status` strictly read-only
- derive status only from:
  - config
  - durable control-plane state
  - runtime progress
- do not perform any venue probes
- present a high-level config-and-control-plane readiness summary plus key details
- answer “what should I do next” before showing lower-level details
- treat `operator_target_revision` as the only startup authority
- clearly distinguish:
  - target adoption missing
  - rollout not ready
  - configured vs active drift
  - restart required
  - config-ready for the next workflow step
  - blocked
- provide a stable readiness model that later `verify` and `bootstrap v2` can reuse

## 4. Non-Goals

This design does not define:

- a replacement for `doctor`
- any venue connectivity or credential probe
- automatic target adoption
- automatic rollout enablement
- runtime mutation or hot reload
- a deeper adoption history audit
- new control-plane persistence
- support for productizing legacy explicit-target startup in the high-level flow

## 5. Architecture Decision

### 5.1 Recommended Approach

Add a new high-level command:

- `app-live status`

Hard rules:

- `status` is a state and config-readiness aggregator, not a preflight
- `status` must not call venue probes
- `status` must not mutate config or durable state
- `status` must not introduce a second startup authority
- `status` must not guess when durable state is inconsistent

### 5.2 Why Not Keep Expanding `doctor`

`doctor` answers a different question:

- can the system safely attempt startup right now

`status` should answer:

- what state am I currently in
- what is missing
- what is the next operator action

If `doctor` absorbs high-level readiness, it will blur preflight with state aggregation and become harder to trust.

### 5.3 Why Not Keep Expanding `targets status`

`targets status` is control-plane specific.

It is useful for:

- configured vs active
- target lineage
- restart-needed semantics

But it is not the correct home for:

- mode-level readiness
- rollout readiness
- paper-mode operator state
- high-level next actions across the whole startup flow

### 5.4 Why High-Level Status Should Not Productize Explicit Targets

The repository still supports explicit-target startup at lower levels today, but that path is now legacy from a UX perspective.

Hard rule:

- `app-live status` should not present explicit-target startup as a first-class high-level readiness path
- if explicit targets are detected, `status` should report an unsupported high-level flow and direct the operator toward adopted-target migration or lower-level commands

This keeps the high-level model clean while the project is still pre-launch.

## 6. Public UX Model

### 6.1 Command Surface

The public entry becomes:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

This command is intended to become the operator homepage.

It should be the first command used when an operator wants to know:

- whether the system is ready
- whether a target still needs adoption
- whether rollout is ready
- whether a restart is required
- whether the next step is `doctor`, `run`, or a control-plane action

### 6.2 Output Structure

Default output should always use three layers:

1. Summary
- mode
- config readiness

2. Key Details
- configured operator target revision
- active operator target revision
- target source
- rollout readiness
- restart needed

3. Next Actions
- concrete operator actions such as:
  - run `targets adopt`
  - run `doctor`
  - perform controlled restart
  - run `run`

The command should optimize for fast operator comprehension, not exhaustive detail dumps.

### 6.3 Relationship To Existing Commands

- `status`
  - high-level readiness summary
  - no venue probes
  - no deep lineage dump

- `doctor`
  - venue-facing preflight
  - credentials, connectivity, runtime safety

- `targets status` and `targets show-current`
  - detailed control-plane explainability
  - lineage and revision-specific debugging

The commands are complementary, not redundant.

## 7. Readiness Model

### 7.1 Inputs

`status` may derive readiness only from:

- validated config
- startup target source state
- durable adoption/control-plane state
- runtime progress / active revision state

It must not:

- probe Polymarket
- probe relayer
- call heartbeat
- perform ws auth or subscription

### 7.2 Mode Layer

The first layer is the configured operator mode:

- `paper`
- `live`
- `real-user shadow smoke`

This determines which readiness states are meaningful.

### 7.3 Readiness Summary States

The high-level output should use a small stable set of config-and-control-plane states:

- `paper-ready`
- `target-adoption-required`
- `restart-required`
- `smoke-rollout-required`
- `smoke-config-ready`
- `live-rollout-required`
- `live-config-ready`
- `blocked`

These are operator states, not implementation states.

Hard rule:

- these states must not imply that venue preflight has already passed
- `doctor` remains the only source of truth for venue-facing readiness

### 7.4 Derivation Rules

#### Paper

`paper-ready` when:

- config is valid
- no high-level blocking state is present

`blocked` when:

- config is invalid
- durable state needed for current config cannot be resolved

#### Live / Smoke Target Anchor

If adopted-target startup is configured but no `operator_target_revision` is present:

- `target-adoption-required`

If durable startup authority exists, continue to rollout and active-state checks.

#### Configured Vs Active

If configured revision and active revision differ:

- `restart-required`

If configured revision exists but active revision is unavailable:

- that is not automatically blocked
- it should normally be interpreted as “this config is not yet active in a running daemon”
- the final state should continue into rollout and mode-specific readiness derivation

#### Rollout Readiness

For `real-user shadow smoke`:

- adopted target present + rollout not enabled:
  - `smoke-rollout-required`
- adopted target present + rollout enabled for the adopted family set:
  - `smoke-config-ready`

For `live`:

- adopted target present but rollout not ready:
  - `live-rollout-required`
- adopted target present + rollout ready:
  - `live-config-ready`

#### Blocked

Any inconsistent or non-explainable state should fail closed to:

- `blocked`

Examples:

- configured revision exists but provenance is unreadable
- durable state cannot be resolved consistently
- startup source is unsupported for the high-level flow

## 8. Legacy Explicit-Target Policy

If `status` detects a legacy explicit-target startup path:

- it must not render that path as normal high-level readiness
- it must report an unsupported high-level flow
- it must give migration-oriented next actions

The message should clearly distinguish:

- this path still exists at lower levels
- this path is not supported by the new high-level readiness UX

This intentionally avoids baking historical complexity into the new operator homepage.

## 9. Error Handling

Internally, failures may still classify as:

- `ConfigError`
- `StateError`
- `UnsupportedHighLevelFlow`
- `BlockedReadiness`

But operator-facing output should default to:

- a config-readiness summary
- a plain-language reason
- explicit next actions

Do not surface raw category names unless they materially help diagnosis.

Hard rule:

- if durable state is contradictory or incomplete, `status` must not guess
- it should report `blocked` with a clear reason and next action

## 10. Testing

This design requires at least:

1. Mode matrix tests
- `paper`
- `live`
- `real-user shadow smoke`

2. Readiness derivation tests
- `paper-ready`
- `target-adoption-required`
- `restart-required`
- `smoke-rollout-required`
- `smoke-config-ready`
- `live-rollout-required`
- `live-config-ready`
- `blocked`

3. Control-plane linkage tests
- configured vs active derivation
- missing adopted target handling
- rollout readiness impact
- fail-closed handling for broken durable state

4. Legacy path tests
- explicit-target config must report unsupported high-level flow
- it must not be presented as normal ready state

5. Output tests
- summary first
- key details second
- next actions third
- next actions should be concrete and mode-appropriate

## 11. Acceptance Criteria

This subproject is complete when:

- `app-live status` exists
- it acts as a high-level readiness entrypoint
- it performs no venue probes
- it derives readiness only from config plus durable/runtime state
- it covers `paper`, `live`, and `real-user shadow smoke`
- it can clearly distinguish:
  - target adoption missing
  - rollout not ready
  - restart required
  - config-ready for the next workflow step
  - blocked
- it does not productize explicit-target startup in the high-level UX
- its output gives clear next actions
- it provides a stable readiness model that later `verify` and `bootstrap v2` can consume

## 12. Architectural Clarification

The optimal architecture is to keep `status` and `doctor` on separate axes:

1. `status`
- state aggregation
- config and control-plane readiness
- next workflow step

2. `doctor`
- venue-facing preflight
- credentials and connectivity
- startup-time runtime safety checks

`status` should not try to emulate a partial preflight verdict.

That means the high-level states above intentionally stop at “config-ready” and “rollout-ready” semantics, not “fully preflighted” semantics.

When `status` says:

- `smoke-config-ready`
- `live-config-ready`

the intended operator interpretation is:

- config and durable state look ready for the next step
- now run `doctor`

not:

- venue checks have already passed

## 13. Recommended Next Step

After this status/readiness surface is implemented, the next UX improvements should build on it in this order:

1. high-level `verify`
2. `bootstrap v2`

That ordering keeps readiness semantics centralized instead of letting multiple high-level flows invent their own status vocabulary.
