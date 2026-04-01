# AxiomArb Init Wizard UX Design

- Date: 2026-03-31
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `app-live` already provides:

- a unified TOML config model
- `app-live init`
- `app-live doctor`
- `app-live run`
- startup-scoped target control-plane commands under `app-live targets ...`

That is a meaningful UX improvement over the old env-plus-JSON flow, but `init` is still only a template generator.

It still expects the operator to:

- understand the config model up front
- edit placeholder values by hand
- infer which sections matter for `paper`, `live`, or `real-user shadow smoke`
- manually figure out the next step when no adopted target is available

The next startup-UX subproject should turn `app-live init` into a real operator wizard:

- interactive
- mode-aware
- minimal-question
- safe to rerun
- explicit about the next command the operator should run

This is not a full control-plane automation project.

It is a focused UX project to make first-time config creation and first-time startup significantly easier.

## 2. Current Repository Reality

At current `HEAD`, the repository already has the foundation needed for a wizard:

1. A single TOML business-config source
- `DATABASE_URL` is the only deployment env var
- operator configuration lives in a single TOML file

2. Clear high-level runtime modes
- `paper`
- `live`
- `real-user shadow smoke` through `runtime.mode = "live"` plus `runtime.real_user_shadow_smoke = true`

3. Startup no longer requires low-level transient auth fields
- L2 auth timestamp/signature can be derived at startup
- builder relayer auth can also be derived when the long-lived secret is present

4. Startup target authority already exists
- normal startup now resolves from startup-scoped `operator_target_revision`
- candidate/adoptable artifacts already exist
- adoption is already explicit through `app-live targets adopt`

5. `init` is still too low-level
- it emits placeholders
- it is not interactive
- it does not distinguish “ready to doctor/run” from “you still need to adopt a target”
- it does not behave like an operator-facing wizard

So the core gap is not configuration storage anymore.

The gap is operator guidance and first-run ergonomics.

## 3. Goals

This design should guarantee the following:

- `app-live init` becomes an interactive wizard rather than a static template writer
- the wizard supports `paper`, `live`, and `real-user shadow smoke`
- the wizard asks only for long-lived identity and credential inputs
- the wizard does not ask for transient auth values
- the wizard defaults or reuses low-risk source configuration values
- the wizard can safely reuse or update an existing local config file
- the wizard can detect whether the current config already carries a startup-scoped `operator_target_revision`
- the wizard outputs a valid TOML plus explicit readiness and next-step guidance
- the wizard never invents a target revision or creates a second startup authority

## 4. Non-Goals

This design does not define:

- automatic adoption of the latest candidate or adoptable revision
- hot reload of target revisions into a running daemon
- a separate operator binary
- a full doctor connectivity overhaul
- target ranking or budget-aware selection
- new discovery logic
- a secrets-management system

## 5. Architecture Decision

### 5.1 Recommended Approach

Upgrade `app-live init` into a mode-aware interactive wizard.

The wizard should:

- ask high-level questions first
- automatically fill low-risk defaults
- preserve existing config where safe
- write a valid `.local.toml`
- tell the operator exactly what to run next

This keeps:

- startup config generation
- startup safety expectations
- adopted-target startup semantics

inside the same operator entrypoint that already owns `doctor`, `run`, and `targets`.

### 5.2 Why Not Keep Improving The Static Template

A better template still leaves the operator with the same problems:

- they must know which sections matter for each mode
- they must infer which fields are long-lived vs transient
- they must infer the correct next step after file creation

That does not meaningfully change the first-run experience.

### 5.3 Why Not Fully Auto-Detect Everything

The repository does not yet have enough trustworthy local state to safely auto-build a full startup config without operator confirmation.

High-risk guesswork would be worse UX than a short, clear wizard.

So the first version should favor:

- interactive guidance
- low-risk defaults
- targeted local-state reuse

rather than “magic” reconstruction.

## 6. Public UX Model

### 6.1 Operator Entry

The entry remains:

```bash
cargo run -p app-live -- init --config config/axiom-arb.local.toml
```

But the behavior changes from:

- render a mostly static template

to:

- run a guided startup-config wizard

### 6.2 Wizard Flow

The operator flow becomes:

1. select runtime mode
2. confirm whether to reuse or refresh an existing config
3. answer only the questions required for that mode
4. let the wizard fill defaults and preserve reusable fields
5. receive a clear end-of-flow summary plus next commands

### 6.3 Modes

The wizard must support:

- `paper`
- `live`
- `real-user shadow smoke`

The mode selection must happen before any detailed questions so the wizard can keep the later question set minimal.

### 6.4 Existing Config Reuse

If the target config path already exists, the wizard should not blindly overwrite it.

It should present a safe choice between:

- reusing and updating the existing config
- replacing it with a fresh minimal skeleton

The default should favor preserving valid existing operator inputs when possible.

## 7. Config Generation Rules

### 7.1 General Rules

The generated config must:

- conform to the unified TOML schema
- be structurally consumable by `doctor` and `run`
- avoid transient auth fields
- avoid raw manual startup target payloads for the normal adopted-target path

For `live` and `real-user shadow smoke`, “structurally consumable” means:

- the generated file is schema-valid
- startup can parse it without legacy low-level fields
- any remaining missing startup-scoped target anchor or rollout-readiness inputs are surfaced explicitly as next-step work

It does not mean the wizard must fabricate a fully ready live session.

### 7.2 `paper`

The `paper` branch should generate only the minimal required config shape.

It must not ask for:

- account credentials
- relayer auth
- startup target authority

It should end by telling the operator to run:

- `doctor`
- `run`

### 7.3 `live`

The `live` branch should ask for:

- account address
- optional funder address
- relayer auth type
- long-lived credential fields for the chosen auth type

It should not ask for:

- dynamic timestamps
- signatures
- ws URLs
- raw target members

### 7.4 `real-user shadow smoke`

The smoke branch should be the same as `live`, except it should automatically set:

- `runtime.mode = "live"`
- `runtime.real_user_shadow_smoke = true`

The wizard must clearly explain that:

- this is still a live runtime posture
- `neg-risk` remains shadow-only
- this does not imply real live-submit readiness

### 7.5 Source Configuration

The wizard should directly default the current standard Polymarket source endpoints and cadence values.

These values should not be asked interactively in the first version.

### 7.6 Account And Relayer Auth

The wizard should store only long-lived identity and credential material.

It should not persist:

- derived L2 timestamp/signature
- derived builder timestamp/signature

If the operator chooses:

- `relayer_api_key`, ask only for the relayer API key and owner address
- `builder_api_key`, ask for the builder API key, builder secret, and passphrase

### 7.7 Target Source

The normal startup path should continue to use:

```toml
[negrisk.target_source]
source = "adopted"
```

The wizard must not ask the operator to hand-author:

- `family_id`
- `condition_id`
- `token_id`
- raw `negrisk.targets` payloads

### 7.8 `operator_target_revision`

`init` must not become a second control-plane write path.

Hard rules:

- `init` may preserve an `operator_target_revision` that is already present in the config file being updated
- `init` may inspect current control-plane state for informational summary output
- `init` must not write a new `operator_target_revision` into the config by copying it out of durable state
- any new startup-scoped target anchor must still be written through `targets adopt` or `targets rollback`

If none exists, the wizard must not invent one.

Instead, it should finish by telling the operator to run:

- `targets candidates`
- `targets adopt`

This keeps `operator_target_revision` as the single startup authority while avoiding fake defaults.

### 7.9 Rollout

The wizard must treat `negrisk.rollout` as a safety input, not as something to infer from target artifacts.

Rules:

- if an existing config already has a valid rollout section, preserve it unless the operator explicitly resets the file
- if no rollout section exists, the wizard should write a safe empty skeleton:

```toml
[negrisk.rollout]
approved_families = []
ready_families = []
```

This keeps the generated config schema-valid and safe by default.

An empty rollout section must be treated as:

- valid config output
- not yet ready for `neg-risk` live/shadow work
- requiring explicit follow-up operator action before expecting route activity

## 8. Question Flow

### 8.1 First Question

The first question must always be mode selection:

- `paper`
- `live`
- `real-user shadow smoke`

### 8.2 `paper` Question Set

Minimal:

- confirm config path
- confirm whether to refresh an existing file

No credential or target questions.

### 8.3 `live` Question Set

Minimal long-lived inputs only:

- config-path reuse/update choice if needed
- account address
- optional funder address
- relayer auth type
- credentials for the chosen auth type

The wizard should then attempt low-risk reuse of:

- existing source settings
- existing config-carried adopted target anchor
- existing rollout section

### 8.4 `real-user shadow smoke` Question Set

Same as `live`, plus a clear confirmation that:

- smoke guard will be enabled
- this is intended for shadow-only validation

### 8.5 Existing Config Conflicts

When the config already exists, the wizard must avoid silent overwrites.

It should treat “preserve and patch” as the default path when safe, and only replace the file when explicitly chosen.

## 9. End-Of-Wizard Summary

This is a core part of the UX.

The wizard should finish by printing two sections:

### 9.1 What Was Written

- config path
- selected mode
- account / relayer auth shape
- whether smoke guard is enabled
- whether an `operator_target_revision` is already configured
- whether rollout is already non-empty or still in the safe empty state

### 9.2 What To Run Next

If an operator target revision is already present:

- `doctor`
- `run`

If an operator target revision is not yet present:

- `targets candidates`
- `targets adopt`
- `doctor`
- `run`

If rollout remains empty, the summary must also say that `neg-risk` work will stay inactive until rollout families are explicitly populated.

This should remove guesswork from the first-run path.

## 10. Error Handling

### 10.1 Error Categories

Wizard errors should be operator-facing and categorized:

- `ConfigPathError`
- `InputValidationError`
- `ExistingConfigConflict`
- `CredentialShapeError`
- `TargetBootstrapWarning`

### 10.2 Recoverable Errors

Recoverable input mistakes should stay inside the wizard loop:

- malformed address input
- incomplete builder credential set
- invalid auth-type selection

The wizard should re-prompt rather than exit.

### 10.3 Terminal Errors

The wizard should exit only on conditions such as:

- config path cannot be written
- existing config cannot be safely updated and replacement was not approved
- raw existing config is unreadable and the operator declines replacement

### 10.4 Target-State Handling

Missing startup target state should not be treated as a hard wizard failure.

It should be surfaced as:

- valid config written
- startup target not yet available
- explicit next steps required

## 11. Testing

### 11.1 Init Flow Tests

Verify:

- `paper`, `live`, and `real-user shadow smoke` question flows
- minimal question sets per mode
- existing config reuse/update behavior

### 11.2 Config Output Tests

Verify:

- generated TOML validates through the unified schema
- generated output does not include transient auth fields
- smoke configs correctly enable `real_user_shadow_smoke`
- configs without an existing rollout produce an explicit safe empty rollout section

### 11.3 Target Bootstrap Tests

Verify:

- when an existing startup-scoped target anchor is already present in the config file, the wizard preserves it
- when durable control-plane state exists outside the config file, the wizard may report it but does not write it into the config
- when none is available, the wizard emits clear next-step guidance instead of inventing a revision

### 11.4 Error Handling Tests

Verify:

- recoverable input problems re-prompt
- terminal file/path errors produce actionable exit messages
- existing config conflicts do not silently clobber credentials

## 12. Acceptance Criteria

This design is complete when:

- `app-live init` behaves like an operator-facing interactive wizard
- the wizard supports `paper`, `live`, and `real-user shadow smoke`
- the wizard no longer asks for transient auth values
- the wizard no longer asks for raw startup target payloads on the adopted-target path
- the generated config is schema-valid and can be handed to `doctor` and `run` without legacy low-level fields
- the wizard either preserves a config-carried valid `operator_target_revision` or clearly tells the operator how to adopt one
- the wizard emits explicit readiness guidance when rollout remains empty
- first-time startup UX becomes a predictable `init -> doctor -> run` flow

## 13. Open Questions Deferred

This design deliberately leaves the following for later work:

- full doctor connectivity preflight
- adoption-history browsing and diff UX
- one-command adopt-and-restart workflows
- secret-store integrations
