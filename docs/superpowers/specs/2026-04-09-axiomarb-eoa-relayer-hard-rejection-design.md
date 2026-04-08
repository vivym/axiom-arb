# AxiomArb EOA Relayer Hard Rejection Design

Date: 2026-04-09
Status: Proposed

## Context

The repository currently treats Polymarket relayer access as a default part of live-mode operator configuration.

That assumption is incorrect for EOA wallets.

Per the official Polymarket documentation:

- EOA wallets do not have relayer access.
- EOA wallets pay gas directly.
- Builder/relayer credentials belong to non-EOA builder-style flows.

Relevant official references:

- https://docs.polymarket.com/builders/overview
- https://docs.polymarket.com/builders/api-keys
- https://docs.polymarket.com/trading/quickstart

At current `main`, this incorrect assumption appears in several layers:

1. live config validation requires `polymarket.relayer_auth` unconditionally
2. `doctor` always probes relayer reachability in live mode
3. `real_user_shadow_smoke` still depends on a signer shape that bundles relayer auth
4. init/example/runbook flows guide EOA operators into relayer config
5. tests and fixtures normalize `EOA + relayer_api_key` as a default valid shape

This is the wrong system model.

## Goals

- Make `EOA + relayer_auth` an invalid configuration combination.
- Remove relayer as a default requirement for EOA live and smoke flows.
- Keep relayer as a first-class capability for non-EOA builder/proxy/safe-style flows.
- Align config validation, runtime behavior, operator UX, and tests to one consistent rule.

## Non-Goals

- Replacing the relayer transport implementation.
- Removing relayer support entirely.
- Reworking Polymarket websocket behavior.
- Redesigning submit/reconcile ownership outside the relayer/EOA boundary correction.

## Design

### 1. Wallet-kind rule becomes explicit

The repository should treat Polymarket wallet kind as the authority for relayer requiredness.

Rules:

- `signature_type = "eoa"` and `wallet_route = "eoa"`
  - `polymarket.relayer_auth` must not be present
- non-EOA wallet kinds
  - `polymarket.relayer_auth` remains required

This is a hard rejection rule, not a soft skip rule.

The system should not accept a config that expresses "EOA, but also relayer".

### 2. Config validation must enforce the rule early

`config-schema` should stop requiring `polymarket.relayer_auth` for every live config.

Instead:

- require `polymarket.account` for live mode as today
- determine wallet kind from account signature type / wallet route
- if wallet kind is EOA:
  - reject config if `polymarket.relayer_auth` exists
- if wallet kind is non-EOA:
  - continue to require and validate `polymarket.relayer_auth`

This moves the error to the earliest possible layer and prevents later runtime/probe confusion.

### 3. App-side signer/config types must stop bundling relayer by default

`LocalSignerConfig` currently models account/L2 and relayer material as one required shape.

That shape is wrong for EOA flows.

After this change:

- account/L2 credentials must remain available for all live Polymarket flows
- relayer auth must become conditional on wallet kind
- app-side config conversion must not force EOA paths through a relayer-bearing signer object

The repository may implement this either by:

- splitting the current structure into account-side and relayer-side config objects
- or making relayer material explicitly optional with wallet-kind validation guarantees

But the end state must be:

- EOA live/smoke config can construct the required runtime inputs without relayer auth
- non-EOA config still carries relayer auth explicitly

### 4. `doctor` probe sets must be wallet-kind aware

`doctor` currently treats relayer reachability as a universal live-mode requirement.

That is wrong for EOA.

After this change:

- EOA probe set:
  - authenticated REST
  - market websocket
  - user websocket
  - heartbeat
  - database connectivity
  - no relayer probe

- non-EOA probe set:
  - authenticated REST
  - market websocket
  - user websocket
  - heartbeat
  - relayer reachability
  - database connectivity

The change should apply to both:

- connectivity probe execution
- credential-shape checks that currently route through one relayer-bearing signer conversion

`doctor` output for EOA should not imply that relayer is skipped because of a failure or fallback.
It should reflect that relayer is not part of the required probe set for that wallet kind.

### 5. `real_user_shadow_smoke` must no longer inherit relayer requiredness from type shape

The current smoke path still carries relayer assumptions because runtime source construction depends on config types that bundle relayer auth.

That coupling should be removed.

After this change:

- EOA `real_user_shadow_smoke` must remain valid without `polymarket.relayer_auth`
- runtime source bundles for EOA smoke must not require relayer-bearing signer config
- non-EOA live paths may continue to require relayer auth when those paths genuinely use relayer-backed capabilities

This is a semantic correction, not just a doctor-only fix.

### 6. Operator-facing UX must stop teaching the wrong configuration

The operator surface must match the new rule everywhere.

#### Init wizard

The init wizard must no longer ask every live/smoke operator for relayer auth.

Correct flow:

1. collect account wallet kind
2. if EOA:
   - do not ask for relayer auth
   - render no `polymarket.relayer_auth` section
3. if non-EOA:
   - collect relayer auth as today

#### Example config

The default example config must stop presenting `[polymarket.relayer_auth]` as part of the EOA shape.

Recommended presentation:

- EOA example:
  - `[polymarket.account]`
  - no relayer section
- separate commented non-EOA example:
  - `[polymarket.relayer_auth]`

#### Smoke runbook

The real-user shadow smoke runbook must stop instructing EOA operators to fill relayer auth.

It should say explicitly:

- EOA smoke does not require relayer auth
- `doctor` does not require relayer reachability for EOA
- relayer applies only to non-EOA Polymarket flows

### 7. Test and fixture truth must be updated

The repository currently normalizes `EOA + relayer_api_key` across many tests and fixtures.

That test truth must change with the product truth.

Required coverage:

- EOA with relayer auth is rejected
- EOA without relayer auth is valid
- non-EOA without relayer auth is rejected
- EOA doctor runs without relayer probe
- EOA init flow omits relayer auth output
- EOA smoke fixtures no longer include relayer auth

Fixtures and tests should stop using relayer sections as harmless filler in EOA configs.

## File-Level Impact

### Core validation and config modeling

- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/config.rs`

### Runtime and doctor behavior

- `crates/app-live/src/commands/doctor/credentials.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/app-live/src/commands/run.rs`
- `crates/app-live/src/source_tasks.rs`

### Operator-facing UX

- `crates/app-live/src/commands/init/wizard.rs`
- `crates/app-live/src/commands/init/render.rs`
- `config/axiom-arb.example.toml`
- `docs/runbooks/real-user-shadow-smoke.md`
- `README.md` where it currently implies relayer is part of the generic live config shape

### Tests and fixtures

- `crates/app-live/tests/config.rs`
- `crates/app-live/tests/doctor_command.rs`
- `crates/app-live/tests/init_command.rs`
- `crates/app-live/tests/real_user_shadow_smoke.rs`
- `crates/config-schema/tests/validated_views.rs`
- `crates/config-schema/tests/fixtures/*.toml`
- any startup/apply/support fixtures still embedding `EOA + relayer_api_key`

## Risks

### 1. Existing local configs may stop validating

That is expected and correct.

The current accepted EOA+relayer shape is semantically invalid. Failing fast is preferable to silently tolerating it.

### 2. Some runtime code may still assume relayer-shaped signer objects

This is the main implementation risk.

The fix should address the type boundary directly rather than adding more EOA-specific branching on top of a relayer-centric config object.

### 3. Test churn will be wide

Also expected.

This slice changes a repository-wide semantic default, so broad fixture cleanup is part of the work, not incidental noise.

## Recommended Implementation Order

1. Fix validation and app-side config modeling.
2. Fix `doctor` and runtime source construction.
3. Fix init/example/runbook UX.
4. Update fixtures and tests to the new truth.

## Acceptance Criteria

- live EOA configs validate without relayer auth
- live EOA configs fail if relayer auth is present
- non-EOA live configs still require relayer auth
- `doctor` does not perform relayer reachability probes for EOA
- `real_user_shadow_smoke` works with EOA account credentials and no relayer config
- init/example/runbook no longer instruct EOA operators to configure relayer auth
- no test fixture continues to treat `EOA + relayer_api_key` as a valid default configuration
