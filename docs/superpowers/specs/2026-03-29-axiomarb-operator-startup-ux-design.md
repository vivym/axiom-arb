# AxiomArb Operator Startup UX Design

- Date: 2026-03-29
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, the repository now has one shared TOML configuration surface, but operator startup UX is still too low-level:

- `app-live` still expects users to understand signer internals, runtime mode details, and smoke/live caveats
- the example config still exposes dynamic authentication fields such as `timestamp` and `signature`
- Polymarket endpoints and websocket URLs are still copied into local config even when they are almost always defaults
- real-user shadow smoke is safer than before, but it is still an engineering runbook rather than a productized startup path
- `neg-risk` startup targets still feel like low-level runtime payloads rather than an operator-facing intent

This is not the desired long-term operator experience.

The next UX-focused subproject should make startup feel closer to:

- prepare one local config file once
- run a guided `init`
- run a readable `doctor`
- start with `run`
- avoid hand-authoring transient auth values and low-level venue endpoints
- consume an already adopted target revision rather than forcing the operator to type family/member identifiers by hand

The recommended direction is:

- keep `app-live` as the only operator entrypoint
- add `app-live init`, `app-live doctor`, and `app-live run`
- revise the config model so persistent config contains long-lived identity and intent, not transient auth material
- derive dynamic auth values at runtime
- treat adopted target revisions as the default startup target source
- keep `DATABASE_URL` as the only deployment environment variable

This is not a control-plane project and not a target-selection project.

It is a productization project for operator startup.

## 2. Current Repository Reality

At current `HEAD`, startup has improved compared with the old env model, but it still exposes too many internal details to the operator:

1. the main startup contract is still essentially:
- copy `config/axiom-arb.example.toml`
- replace placeholders
- choose `paper` or `live`
- optionally set `real_user_shadow_smoke = true`
- manually run `cargo run -p app-live -- --config ...`

2. the example config still requires explicit low-level fields:
- `polymarket.source.*` host and websocket URLs
- `polymarket.signer.timestamp`
- `polymarket.signer.signature`
- full `negrisk.targets` tables with `family_id`, `condition_id`, and `token_id`

3. the real-user shadow smoke runbook still assumes the operator already knows:
- which values are dynamic versus persistent
- which fields are safe defaults
- what relayer auth mode to choose
- which startup targets are valid

4. the current binary only exposes a generic `--config` entrypoint:
- there is no guided initialization
- there is no first-class preflight doctor flow
- there is no operator-facing startup checklist contract

The repository has already done the hard backend work:

- unified config schema
- live/shadow/runtime authority boundaries
- real-user shadow smoke guard
- adopted target and candidate provenance

What remains poor is the operator experience layered on top of those capabilities.

## 3. Goals

This design should guarantee the following:

- `app-live` becomes the single operator-facing startup entrypoint
- startup is organized around three explicit commands:
  - `init`
  - `doctor`
  - `run`
- persistent config only contains long-lived identity, credentials, and high-level intent
- dynamic auth values such as request timestamps and signatures are not hand-authored by the operator
- default Polymarket endpoints and websocket URLs no longer need to be copied into local config for the common case
- `neg-risk` startup defaults to an already adopted target revision instead of hand-authored family/member payloads
- real-user shadow smoke remains safe and becomes easier to use
- error output becomes operator-readable and action-oriented
- README, runbooks, helper scripts, and config templates converge on the new startup flow

## 4. Non-Goals

This design does not define:

- automatic candidate selection or automatic adoption of the latest candidate revision
- a new control-plane service
- hot reload of running operator config
- a separate `axiomctl` binary
- a secrets-management platform
- automatic live promotion beyond the current rollout and adoption authority model
- a replacement for the existing activation/risk/planner/execution backbone

## 5. Architecture Decision

### 5.1 Recommended Approach

Use `app-live` itself as the productized operator surface.

The binary should expose:

- `app-live init`
- `app-live doctor`
- `app-live run`

The operator should not need a second helper binary to bootstrap or preflight the system.

Hard rule:

- there is one operator entrypoint
- helper scripts may remain for convenience, but they are wrappers over documented `app-live` commands rather than the primary UX

### 5.2 Why Not A Separate Control CLI

Adding a separate `axiomctl` or support CLI would create another place for startup truth to drift:

- different defaults
- different validation
- different docs
- different assumptions about smoke/live safety

This repository already has a natural operator surface:

- `app-live`

The UX should be improved there rather than split.

### 5.3 Why Startup Should Prefer Adopted Targets

The startup path should consume an already adopted target revision because:

- it preserves the current authority boundary between candidate generation and execution
- it removes the need for operators to hand-author `family_id` / `condition_id` / `token_id`
- it keeps startup focused on running the system, not deciding what the system should trade

Hard rule:

- startup may resolve and load an adopted target revision
- startup must not silently promote the latest candidate set to a runnable target set

## 6. Public UX Model

### 6.1 Command Surface

The operator-facing command set should become:

- `app-live init --config <path>`
- `app-live doctor --config <path>`
- `app-live run --config <path>`

`app-live run` becomes the production entrypoint.

`app-live init` and `app-live doctor` are supporting UX commands, not alternative runtimes.

### 6.2 Expected Operator Flow

First-time setup:

1. run `app-live init --config config/axiom-arb.local.toml`
2. fill or confirm prompted values
3. run `app-live doctor --config config/axiom-arb.local.toml`
4. if checks pass, run `app-live run --config config/axiom-arb.local.toml`

Steady-state operation:

1. update config only when identity, credentials, or runtime intent changes
2. optionally rerun `doctor`
3. use `run` as the normal startup path

### 6.3 Runtime Mode Semantics

Top-level runtime mode remains:

- `paper`
- `live`

`Shadow` remains a route/execution result, not a third top-level runtime mode.

For real-user shadow smoke, the operator UX should be:

- `runtime.mode = "live"`
- `runtime.real_user_shadow_smoke = true`

The startup flow must keep reinforcing that this means:

- real upstream user/session data is used
- `neg-risk` execution is forced to `Shadow`
- no live submit side effect is allowed

## 7. Config Model Revision

### 7.1 Persistent Config Should Hold Long-Lived Identity And Intent

The persistent operator config should hold:

- runtime intent
- account identity
- long-lived Polymarket credentials
- relayer auth choice and long-lived relayer credentials
- target source policy
- optional non-default operational overrides

It should not hold fields that are naturally transient per request or per startup.

### 7.2 Fields To Remove From Persistent Config

The following should stop being operator-authored persistent fields:

- `polymarket.signer.timestamp`
- `polymarket.signer.signature`
- explicit default Polymarket hosts and websocket URLs for the common case
- hand-authored `negrisk.targets` family/member/token payloads in the normal startup path

These values should become:

- runtime-derived
- built-in defaults
- or resolved from adopted target state

### 7.3 Recommended High-Level Shape

The config should evolve from the current low-level schema toward a higher-level operator model.

Recommended sections:

- `[runtime]`
- `[polymarket.account]`
- `[polymarket.relayer_auth]`
- `[negrisk.target_source]`
- optional `[polymarket.source_overrides]` only for non-default hosts or cadence changes

Expected long-lived account fields:

- `address`
- optional `funder_address`
- `signature_type`
- `wallet_route`
- `api_key`
- `secret`
- `passphrase`

Expected relayer auth should also move to long-lived credential material rather than transient request values:

- `relayer_api_key` mode:
  - `api_key`
  - `address`
- `builder_api_key` mode:
  - `api_key`
  - `secret`
  - `passphrase`

Per-request relayer signatures or timestamps should be runtime-derived where the auth mode requires them.

Expected target-source fields:

- `source = "adopted"`
- optional `adopted_revision`

Hard rule:

- `source = "adopted"` is the normal operator path
- the operator should not be required to type concrete family/member/token identifiers to start the system

### 7.4 Consumer And Mode Scoped Requiredness

Requiredness must stay scoped to the startup mode rather than collapsing back into "every field is always required."

Recommended matrix:

- `paper`:
  - requires `[runtime]`
  - may omit Polymarket account, relayer auth, and target-source sections entirely
- `live` standard startup:
  - requires `[runtime]`
  - requires `[polymarket.account]`
  - requires `[polymarket.relayer_auth]`
  - requires `[negrisk.target_source]`
  - may omit `[polymarket.source_overrides]` when defaults are acceptable
- `live` with `real_user_shadow_smoke = true`:
  - requires everything from standard live startup
  - requires enough source configuration or defaults to build real upstream connectivity
  - must additionally satisfy smoke safety validation

The UX goal is not to make every config file large.

The UX goal is to make the config file minimal for the chosen startup mode.

### 7.5 Target Source Resolution

When `target_source = "adopted"`:

- `doctor` verifies that an adopted target revision exists and is loadable
- `run` resolves the adopted target revision before runtime starts

If no adopted revision is available:

- `doctor` returns a clear `TargetSourceError`
- `run` fails fast and tells the operator what to fix

Startup must not silently fall back to:

- latest candidate
- empty target set
- hard-coded sample targets

## 8. `app-live init`

### 8.1 Purpose

`init` exists to create or update a local operator config skeleton.

It should ask only high-level, persistent questions such as:

- `paper` or `live`
- whether real-user shadow smoke is desired
- account address
- relayer auth mode
- long-lived Polymarket credential material
- whether startup should load adopted targets

### 8.2 Responsibilities

`init` should:

- generate a local config file if one does not exist
- update an existing local config when run with an explicit update mode
- write safe defaults for endpoint and cadence settings
- avoid asking for transient auth values
- avoid asking for concrete target members in the normal adopted-target flow

### 8.3 Non-Responsibilities

`init` should not:

- contact external services
- validate credentials against real APIs
- verify websocket connectivity
- confirm adopted target availability
- start the runtime

## 9. `app-live doctor`

### 9.1 Purpose

`doctor` exists to run startup preflight checks and present results in operator-readable form.

### 9.2 Check Categories

The doctor flow should report explicit check categories:

- config parsing
- semantic validation
- account credential completeness
- runtime auth material derivation
- REST authentication
- market websocket connectivity
- user websocket authentication/connectivity
- heartbeat contract health
- relayer auth health
- adopted target source availability
- smoke safety enforcement

### 9.3 Output Model

Doctor output should be checklist-oriented, for example:

- `[OK] config parsed`
- `[OK] dynamic auth derivation available`
- `[FAIL] relayer auth rejected`
- `[FAIL] no adopted target revision available`
- `[OK] real-user shadow smoke enforces neg-risk shadow-only`

Each failure must include:

- a stable error category
- a human-readable cause
- a concrete next step

### 9.4 Non-Responsibilities

`doctor` should not:

- mutate live runtime state
- enter a long-running daemon loop
- silently rewrite the config as part of validation

It may optionally emit suggested fixups, but it should not behave like a hidden migration tool.

## 10. `app-live run`

### 10.1 Purpose

`run` is the real startup path.

It should assume:

- a valid config file
- `DATABASE_URL`
- runtime-derived transient auth material
- an adopted target source when in live/adopted target mode

### 10.2 Startup Responsibilities

`run` should:

- load and validate config
- derive dynamic auth values for the current startup
- apply built-in default endpoints unless explicitly overridden
- resolve the adopted target revision
- establish smoke/live overlays from the config
- start the runtime

### 10.3 Failure Experience

If `run` cannot satisfy a prerequisite, it should:

- fail fast
- surface a categorized operator-facing error
- suggest running `app-live doctor --config ...` for detailed diagnostics

`run` should not degrade into:

- implicit interactive prompts
- hidden target selection
- silent fallback to manual test targets

## 11. Error Model

Operator-facing startup errors should be grouped into a stable taxonomy:

- `ConfigError`
- `CredentialError`
- `ConnectivityError`
- `TargetSourceError`
- `RuntimeSafetyError`

Each category should map to:

- a short summary
- the underlying technical cause
- a concrete next step

Examples:

- `CredentialError: missing polymarket.account.secret`
- `Fix: rerun 'app-live init --update' or edit [polymarket.account] in the local config`

- `TargetSourceError: no adopted target revision is available for source=adopted`
- `Fix: adopt a target revision first, or switch the runtime to paper mode`

Hard rule:

- startup UX must not expose raw low-level library errors as the primary operator interface

## 12. Safety And Authority Constraints

This UX project must preserve existing authority boundaries.

### 12.1 Adopted Targets Remain The Default Execution Input

Startup UX may make adopted targets easier to consume.

It must not:

- auto-adopt the latest candidate
- bypass target adoption provenance
- change rollout approval authority

### 12.2 Smoke Safety Must Remain Hard-Guarded

When real-user shadow smoke is enabled:

- `neg-risk` must remain shadow-only
- the UX may make this easier to start
- the UX must not weaken the existing shadow safety contract

### 12.3 Defaults Must Not Create Hidden Behavioral Drift

Built-in defaults are appropriate for:

- Polymarket host URLs
- websocket URLs
- common cadence values
- derivable transient auth material

Built-in defaults are not appropriate for:

- which targets to execute
- which candidate revision to adopt
- rollout approval state

## 13. Migration Strategy

Although the implementation may land in multiple internal steps, operator UX should converge on one public model:

- use `app-live init`
- validate with `app-live doctor`
- start with `app-live run`

Migration work should include:

- revising the config schema toward higher-level account and target-source fields
- updating `config/axiom-arb.example.toml`
- updating local config guidance in README
- replacing the current smoke script assumptions
- updating runbooks to stop instructing operators to author transient auth fields or raw target payloads

The repository should not preserve the current low-level startup UX as a second long-lived documented path.

## 14. Testing Strategy

### 14.1 `init` Tests

Validate that:

- a minimal config skeleton can be generated
- transient auth fields are not required in persistent config
- default endpoints do not need operator authorship
- `paper`, `live`, and real-user shadow smoke choices map to the correct config shape

### 14.2 `doctor` Tests

Validate that:

- failures are categorized correctly
- failures include operator-readable repair guidance
- smoke mode verifies shadow-only enforcement
- missing adopted targets produce `TargetSourceError`
- credential derivation failures surface as `CredentialError`

### 14.3 `run` Tests

Validate that:

- dynamic auth material is derived at startup
- default endpoints are applied when omitted from config
- adopted targets are resolved before runtime start
- missing prerequisites fail fast and recommend `doctor`

### 14.4 Migration Tests

Validate that:

- README, templates, and scripts all point to the new startup flow
- the operator is no longer instructed to fill `timestamp`, `signature`, `family_id`, `condition_id`, or `token_id` for the normal adopted-target startup path

## 15. Acceptance Criteria

This UX project is complete when:

- `app-live` exposes `init`, `doctor`, and `run`
- the normal startup path no longer requires operator-authored transient auth values
- the normal startup path no longer requires operator-authored raw target member payloads
- built-in Polymarket defaults remove the need to copy standard endpoint URLs into local config
- startup defaults to loading an adopted target revision rather than hand-authored target members
- real-user shadow smoke remains safe and becomes easier to bootstrap
- operator-visible errors are categorized, actionable, and no longer dominated by low-level crate wording
- README, templates, runbooks, and helper scripts all reflect the new UX

## 16. Open Follow-On Work

This design intentionally leaves out:

- automatic adoption from the latest candidate revision
- full control-plane UX
- richer target browsing and selection UI
- future live ranking or budgeting UX

Those belong to later phases once startup is no longer the primary UX bottleneck.
