# AxiomArb Doctor Preflight UX Design

- Date: 2026-04-01
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `app-live` now has a productized operator startup path:

- `app-live init`
- `app-live doctor`
- `app-live run`

But `doctor` is still only a light startup checker. It currently proves:

- config parsing and semantic validation
- mode-scoped config requiredness
- database connectivity
- startup target resolution
- configured-vs-active target state reporting

It does **not** yet prove the most important external readiness contracts for `live` and `real-user shadow smoke`:

- long-lived credential derivation
- authenticated REST reachability
- market websocket connectivity
- user websocket auth/subscribe connectivity
- heartbeat contract compatibility
- relayer reachability

This leaves the startup flow with an obvious weak point:

- `init` now guides configuration
- `run` now starts the runtime
- but `doctor` still cannot tell the operator whether the real venue-facing path is actually usable

The next UX-focused subproject should upgrade `app-live doctor` into a real operator preflight.

The recommended direction is:

- keep `app-live doctor` as the single preflight entrypoint
- preserve mode-scoped semantics for `paper`, `live`, and `real-user shadow smoke`
- add real external probes for `live` and `smoke`
- allow only protocol-required writes such as heartbeat
- keep `doctor` read-only with respect to repo-owned runtime/control-plane state
- present results as structured sections with clear next actions

This is not a smoke-runner project, not a control-plane mutation project, and not a live-submit validation project.

It is a startup preflight project.

## 2. Current Repository Reality

At current `HEAD`, `doctor` lives in [`crates/app-live/src/commands/doctor.rs`](/Users/viv/projs/axiom-arb/crates/app-live/src/commands/doctor.rs).

Its current behavior is intentionally lightweight:

1. it loads and validates the operator TOML
2. in `paper` mode it reports live checks as `SKIP`
3. in `live` mode it connects to the database and resolves startup targets
4. it reports configured-vs-active target state and `restart needed` semantics

This is useful, but still incomplete for real operator readiness.

The repository now already has the backend primitives needed for a stronger `doctor`:

- long-lived `polymarket.account` credentials with runtime auth derivation
- `relayer_api_key` and `builder_api_key` auth shapes
- real market websocket subscribe wiring
- real user websocket auth/subscribe wiring
- real `postHeartbeat(previous_heartbeat_id)` request wiring
- `real_user_shadow_smoke` safety guard

The gap is no longer the venue client capabilities.

The gap is that `doctor` does not yet use them to prove actual readiness.

## 3. Goals

This design should guarantee the following:

- `app-live doctor` becomes the single operator-facing startup preflight command
- `doctor` reports results in stable sections rather than one flat checklist
- `doctor` keeps covering `paper`, `live`, and `real-user shadow smoke`
- `paper` mode remains lightweight and explicitly reports non-applicable checks as `SKIP`
- `live` and `smoke` run real external preflight probes
- the preflight verifies authenticated REST, market ws, user ws, heartbeat, and relayer reachability
- the preflight continues to report startup target resolution and configured-vs-active target state
- failures become operator-readable and action-oriented
- successful output clearly states whether the next action is `run`, `targets adopt`, or credential repair

## 4. Non-Goals

This design does not define:

- order submission checks
- live venue-side write checks beyond protocol-required actions
- automatic adoption or rollback
- config rewriting from `doctor`
- a shadow smoke runner
- hot reloading runtime state
- deeper control-plane history auditing beyond existing target resolution and configured-vs-active semantics

## 5. Architecture Decision

### 5.1 Recommended Approach

Upgrade the existing `doctor` command in place.

Hard rule:

- there remains one operator preflight entrypoint
- the command remains `app-live doctor --config <path>`
- new probes are integrated into `doctor`, not split into a parallel connectivity-only CLI

### 5.2 Why Not A Separate Connectivity Command

A separate preflight binary or subcommand would immediately create two startup-check stories:

- one for config and target readiness
- one for venue connectivity

That would weaken the `init -> doctor -> run` operator path that was just established.

The repository should converge on one answer to:

- “am I ready to run?”

That answer should remain:

- run `app-live doctor`

### 5.3 Allowed Side Effects

`doctor` may perform protocol-required write-like actions when those actions are necessary to prove connectivity and contract compatibility.

Allowed examples:

- websocket auth/subscribe handshakes
- heartbeat requests

Disallowed examples:

- order submission
- adoption or rollback mutations
- config rewrites
- runtime state mutation
- long-running daemon loops

Hard rule:

- `doctor` may prove live connectivity
- `doctor` must not trigger order/live submit behavior

## 6. Public UX Model

### 6.1 Command Surface

The command remains:

- `app-live doctor --config <path>`

No new public subcommand is required for the first phase of this improvement.

### 6.2 Expected Operator Flow

Normal operator startup should become:

1. run `app-live init --config ...`
2. if needed, run `app-live targets candidates` and `app-live targets adopt ...`
3. run `app-live doctor --config ...`
4. if `doctor` passes, run `app-live run --config ...`

For steady-state restart:

1. ensure configured target state is correct
2. run `doctor`
3. if `doctor` passes, run `run`

### 6.3 Output Structure

`doctor` should emit a stable sectioned report with these top-level groups:

- `Config`
- `Credentials`
- `Connectivity`
- `Target Source`
- `Runtime Safety`

Each section should contain individual checks.

Each check must report one of:

- `OK`
- `FAIL`
- `SKIP`

At the end, `doctor` must also emit an overall result:

- `PASS`
- `FAIL`
- `PASS WITH SKIPS`

And a `Next` action section.

## 7. Mode-Scoped Check Matrix

### 7.1 `paper`

`paper` mode should run:

- `Config`
- `Runtime Safety`
- optional target-source config sanity checks when cheap and local

It must explicitly `SKIP`:

- credential derivation
- authenticated REST
- market ws
- user ws
- heartbeat
- relayer reachability

### 7.2 `live`

`live` mode should run:

- `Config`
- `Credentials`
- `Connectivity`
- `Target Source`
- `Runtime Safety`

This is the full preflight path.

### 7.3 `real-user shadow smoke`

`real-user shadow smoke` should also run the full preflight matrix:

- `Config`
- `Credentials`
- `Connectivity`
- `Target Source`
- `Runtime Safety`

But `Runtime Safety` must additionally prove:

- `runtime.mode = "live"`
- `real_user_shadow_smoke = true`
- the neg-risk shadow-only guard is in effect

Hard rule:

- smoke should prove real upstream readiness
- smoke should not claim live-submit readiness

## 8. Probe Contracts

### 8.1 Config Section

This section should prove:

- config file can be loaded
- semantic validation succeeds
- mode-scoped requiredness is satisfied

This section remains fail-fast.

### 8.2 Credentials Section

This section should prove:

- long-lived L2 credentials are sufficient for runtime auth derivation
- relayer auth shape is internally consistent
- required long-lived fields exist for the selected mode

This section must not:

- rewrite config
- persist derived auth material
- silently auto-repair malformed credentials

### 8.3 Connectivity Section

This section should prove real external readiness.

Required probes for `live` and `smoke`:

1. authenticated REST probe
- one minimal authenticated read-only request
- enough to prove the server accepts the credentials and the response shape is parseable

2. market ws probe
- connect market ws
- send minimal subscribe
- receive an interpretable ack, pong, or data message

3. user ws probe
- connect user ws
- perform auth/subscribe
- receive an interpretable ack, pong, or data message

4. heartbeat probe
- issue one `postHeartbeat(previous_heartbeat_id)` request or equivalent current contract path
- prove response compatibility

5. relayer probe
- prove reachability and auth acceptance for the configured relayer auth mode

Hard rules:

- these probes may use protocol-required writes
- these probes must not enter the order submit path
- these probes must not create runtime follow-up work

### 8.4 Target Source Section

This section should continue the current contract:

- resolve startup targets
- report configured operator target revision
- report active operator target revision when available
- report `restart needed` semantics

This section must not expand into a deeper control-plane audit.

It should stay focused on startup readiness.

### 8.5 Runtime Safety Section

This section should prove:

- mode flags are internally consistent
- smoke guard is active when required
- the preflight is not accidentally being interpreted as a live-submit readiness claim

For `paper`, this section primarily proves non-live expectations.

For `smoke`, this section should explicitly state the shadow-only safety posture.

## 9. Error Model

The operator-visible error categories should be:

- `ConfigError`
- `CredentialError`
- `ConnectivityError`
- `TargetSourceError`
- `RuntimeSafetyError`

Each failure should also include an action-oriented next step.

Examples:

- `CredentialError: missing polymarket.account.secret`
- `Fix: rerun init or update [polymarket.account] in config/axiom-arb.local.toml`

- `TargetSourceError: adopted source configured but operator_target_revision missing`
- `Fix: run 'app-live targets candidates' then 'app-live targets adopt ...'`

Hard rule:

- do not dump only low-level transport or parser errors without operator context

## 10. Reporting Model

### 10.1 Section Summaries

Every section should emit a section-level result such as:

- `Config: PASS`
- `Connectivity: FAIL`
- `Credentials: PASS`

This lets the operator scan the report quickly.

### 10.2 Overall Result

The overall result should be exactly one of:

- `PASS`
- `FAIL`
- `PASS WITH SKIPS`

`restart needed` is not a failure state.

`runtime state unavailable` is not automatically a failure state.

### 10.3 Next Actions

The report must always end with explicit next actions, such as:

- `Next: run app-live -- run --config ...`
- `Next: run app-live -- targets candidates`
- `Next: run app-live -- targets adopt ...`
- `Next: fix credentials and rerun doctor`

## 11. Implementation Notes

The implementation should favor reusable probe helpers over inlining raw connectivity logic inside `doctor.rs`.

A reasonable internal split is:

- section model / result rendering
- mode matrix / orchestration
- credential probes
- connectivity probes
- existing target-source and runtime-safety checks

But this does not need to surface as additional public commands.

## 12. Tests

### 12.1 Mode Matrix Tests

Validate:

- `paper` runs only its applicable checks and marks others `SKIP`
- `live` runs the full matrix
- `smoke` runs the full matrix plus smoke safety checks

### 12.2 Probe Contract Tests

Validate:

- credentials probes do not mutate config
- REST probe is authenticated and read-only
- market/user ws probes correctly classify success and protocol mismatch
- heartbeat probe remains inside allowed protocol-required side effects
- relayer probe does not enter order submit

### 12.3 Output Tests

Validate:

- output renders in stable sections
- section summaries match the item-level checks
- overall result is one of `PASS / FAIL / PASS WITH SKIPS`
- failure output contains next actions

### 12.4 Target Source Regression Tests

Validate:

- startup target resolution remains unchanged
- configured-vs-active / restart-needed semantics remain unchanged
- `doctor` still does not adopt, roll back, or rewrite config

## 13. Acceptance Criteria

This work is complete when:

- `app-live doctor` is the single operator-facing startup preflight entrypoint
- `doctor` renders `Config`, `Credentials`, `Connectivity`, `Target Source`, and `Runtime Safety` sections
- `paper`, `live`, and `real-user shadow smoke` all have stable mode-scoped check matrices
- `live` and `smoke` perform real external probes for REST, ws, heartbeat, and relayer reachability
- those probes do not trigger order submit or repo-owned runtime/control-plane state mutation
- failures are operator-readable and action-oriented
- the startup path `init -> doctor -> run` becomes meaningfully trustworthy for real operator use

## 14. Summary

The next UX-focused subproject should upgrade `app-live doctor` into a real startup preflight. The command stays in place and remains the single operator-facing preflight entrypoint, but its output becomes sectioned and mode-aware, and its `live` / `real-user shadow smoke` behavior expands to run real external venue-facing probes. Those probes may use protocol-required actions such as websocket auth/subscribe and heartbeat, but they must stop well short of order submission or runtime mutation. Target-source and configured-vs-active reporting remain in scope, but deeper adoption/control-plane auditing stays out of scope for this phase. The goal is not to create a new smoke runner. The goal is to make `doctor` trustworthy enough that `init -> doctor -> run` is a real operator startup path instead of a mostly local checklist.
