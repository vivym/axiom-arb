# AxiomArb Unified Config Schema Design

- Date: 2026-03-28
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, repository configuration is split across multiple incompatible surfaces:

- `app-live` reads a growing set of `AXIOM_*` environment variables
- `crates/config` still reads a separate `POLY_*` and base-env model
- runbooks and helper scripts now carry large JSON payloads inside env vars
- operator-facing startup inputs for `neg-risk`, smoke, signer, and source wiring are distributed across README snippets, scripts, and local shell state

This is no longer a tolerable early-project shortcut.

The repository should move to a single configuration truth source:

- one operator-facing TOML file
- one public schema owned by a new `config-schema` crate
- one validation path used by every binary
- one remaining deployment-level environment variable: `DATABASE_URL`

The design goal is not merely to make env handling prettier.

The goal is to eliminate configuration drift across binaries, docs, and operator workflows while keeping runtime wiring and business authority boundaries unchanged.

The recommended direction is:

- add a new public `config-schema` crate
- define one typed TOML schema for current, real repository needs
- have the crate handle file loading, parsing, and semantic validation only
- make `app-live`, `app-replay`, and any remaining consumers load the same validated configuration object
- remove the current `AXIOM_*` and `POLY_*` business-config entrypoints rather than adding a long-lived compatibility layer
- preserve `DATABASE_URL` as the only deployment environment variable

In short:

- configuration should stop being a side effect of shell state
- configuration should become a first-class repository contract

## 2. Current Repository Reality

At current `HEAD`, the repository has two overlapping but inconsistent configuration models:

1. `app-live` startup configuration
- `crates/app-live/src/main.rs` reads:
  - `AXIOM_MODE`
  - `AXIOM_NEG_RISK_LIVE_TARGETS`
  - `AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES`
  - `AXIOM_NEG_RISK_LIVE_READY_FAMILIES`
  - `AXIOM_LOCAL_SIGNER_CONFIG`
  - `AXIOM_REAL_USER_SHADOW_SMOKE`
  - `AXIOM_POLYMARKET_SOURCE_CONFIG`
- several of these values are large JSON payloads embedded directly in environment variables

2. legacy/common configuration surface
- `crates/config/src/settings.rs` still reads:
  - `DATABASE_URL`
  - `AXIOM_MODE`
  - `POLY_CLOB_HOST`
  - `POLY_DATA_API_HOST`
  - `POLY_RELAYER_HOST`
  - `POLY_SIGNATURE_TYPE`

This creates multiple forms of drift:

- runtime truth and repository examples disagree
- `.env.example` only partially reflects current `app-live` reality
- smoke helper scripts have to inline large JSON blobs
- binaries do not share one authoritative configuration loader
- library consumers and binaries no longer describe the same system

The project is still early enough that carrying forward both models would be a net negative.

## 3. Goals

This design should guarantee the following:

- the repository has one operator-facing configuration file format
- the file format is TOML, optimized for human editing and review
- all current repository runtime/business configuration comes from one validated schema
- `DATABASE_URL` remains the only deployment-level environment variable
- `app-live` and `app-replay` both load configuration through the same schema path
- shared schema does not imply identical required sections for every binary or runtime mode
- the current `AXIOM_*` and `POLY_*` business-config entrypoints are removed rather than kept as compatibility burden
- large structured payloads stop being represented as JSON-in-env strings
- schema validation is centralized and fail-closed
- the configuration system only models current, real repository needs and does not pre-allocate speculative future sections

## 4. Non-Goals

This design does not define:

- a secrets-management platform
- a multi-file include/import system
- hot reload of running operator configuration
- a compatibility layer that preserves legacy `AXIOM_*` / `POLY_*` startup semantics
- a future ranking, budgeting, or control-plane schema beyond fields currently consumed by the repository
- runtime adapter construction inside the configuration crate

## 5. Architecture Decision

### 5.1 Recommended Approach

Introduce a new public workspace crate:

- `crates/config-schema`

That crate becomes the only owner of:

- TOML schema types
- file loading
- parse errors
- semantic validation
- validated configuration object construction

Binary crates become consumers of validated config views rather than owners of their own parsing logic.

Hard rule:

- one shared schema crate may expose multiple validated views
- each view must still derive from the same TOML document and the same semantic validation ruleset
- sharing schema must not force every binary to require every section

### 5.2 Why TOML

TOML is the right operator-facing format for this repository because it:

- is easier to hand-edit than JSON
- supports comments
- represents nested runtime sections naturally
- works well for lists of targets and grouped credentials
- reduces the cognitive overhead of current JSON-in-env payloads

JSON remains appropriate for generated artifacts, replay rows, and external API payloads.

It should not remain the main human-edited runtime configuration format.

### 5.3 Why Not `clap`-Only

`clap` should be used for a small binary entrypoint contract such as:

- `--config path/to/app-live.toml`

It should not be used to encode the full runtime configuration surface.

Turning signer, source, `neg-risk`, and smoke configuration into dozens of CLI flags would be worse than the current env model.

### 5.4 Why Not `*_FILE` As The Main Design

Supporting `*_FILE` would improve the current env model, but it would not solve the underlying architectural issue:

- business configuration would remain split across many keys
- operator truth would still be fragmented
- binaries would still lack one shared schema

`*_FILE` may be a tactical convenience in some systems.

It is not the long-term architecture for this repository.

## 6. Public Configuration Model

### 6.1 Top-Level Structure

The unified file should be a single TOML document with the following top-level sections:

- `[runtime]`
- `[polymarket.source]`
- `[polymarket.signer]`
- `[polymarket.relayer_auth]`
- `[negrisk.rollout]`
- `[[negrisk.targets]]`
- optional `[candidate]` only if current code truly consumes candidate-generation knobs

There should not be a placeholder section for future features that the current code does not consume.

Presence in the schema does not mean unconditional requiredness.

Requiredness must be evaluated against:

- consumer binary
- runtime mode
- smoke/runtime posture

### 6.2 Runtime Section

The runtime section should express process-level startup semantics, not route-level execution decisions.

Recommended fields:

- `mode = "paper" | "live"`
- `real_user_shadow_smoke = true | false`

Hard rule:

- `Shadow` is not a top-level runtime mode

`Shadow` remains an activation/execution result produced by existing runtime authority, not a third binary mode alongside `paper` and `live`.

### 6.3 Polymarket Source Section

This section should replace the current `AXIOM_POLYMARKET_SOURCE_CONFIG` JSON payload.

It should include the current source fields already modeled in `app-live`:

- `clob_host`
- `data_api_host`
- `relayer_host`
- `market_ws_url`
- `user_ws_url`
- `heartbeat_interval_seconds`
- `relayer_poll_interval_seconds`
- `metadata_refresh_interval_seconds`

### 6.4 Polymarket Signer Section

This section should replace the current `AXIOM_LOCAL_SIGNER_CONFIG` signer and L2 auth payload.

It should include:

- `address`
- `funder_address`
- `signature_type`
- `wallet_route`
- `api_key`
- `passphrase`
- `timestamp`
- `signature`

This section is allowed to contain secret-like values.

The schema should support them explicitly rather than forcing users to inject an opaque JSON string.

### 6.5 Polymarket Relayer Auth Section

This should be a tagged union in TOML representing the currently supported auth modes:

- `builder_api_key`
- `relayer_api_key`

It should replace the current `LocalRelayerAuth` JSON payload model without changing runtime semantics.

### 6.6 Neg-Risk Section

This section should model the current operator-facing `neg-risk` startup inputs:

- approved families
- ready families
- live targets

That means the current env inputs:

- `AXIOM_NEG_RISK_LIVE_TARGETS`
- `AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES`
- `AXIOM_NEG_RISK_LIVE_READY_FAMILIES`

must become proper TOML arrays/tables rather than ad hoc string lists and JSON blobs.

### 6.7 Candidate Section

A `candidate` section should only be introduced if the current codebase already consumes user-supplied candidate-generation knobs.

If current candidate behavior is fixed by code and not operator-configurable, this section should not be added yet.

Hard rule:

- the schema file is not a roadmap document
- only currently consumed fields belong in the schema

## 7. Type Design Principles

The `config-schema` crate should follow these rules:

1. operator-facing semantics first
- field names and structure should reflect what an operator configures
- they should not leak arbitrary internal implementation details

2. strong typing everywhere practical
- nested structs and enums should replace embedded JSON strings
- no TOML field should itself contain a second serialized JSON document

3. raw parse types and validated types should be distinct
- raw parse types represent deserialized TOML
- validated types represent semantically correct config ready for consumption

4. no runtime adapter construction in the config crate
- the crate does not build venue clients, runtime supervisors, or activation state
- it only returns validated configuration data

## 8. Validation And Fail-Closed Behavior

### 8.1 Parse-Level Validation

The `config-schema` crate must reject:

- invalid TOML syntax
- missing required sections or required fields
- invalid URLs
- invalid enum values
- invalid numeric fields such as zero or negative cadence where not allowed

### 8.2 Semantic Validation

The crate must also reject semantically invalid but parseable configs.

Examples:

- `runtime.mode = "paper"` with `real_user_shadow_smoke = true`
- `real_user_shadow_smoke = true` without a valid Polymarket source section
- `real_user_shadow_smoke = true` without signer configuration
- `approved_families` or `ready_families` referencing families absent from `negrisk.targets`
- duplicate target family definitions
- duplicate or malformed target members
- incompatible `signature_type` and `wallet_route`

### 8.3 Consumer-Scoped Requiredness

Requiredness must be consumer-scoped and mode-scoped, not global.

The schema crate should therefore expose validated views such as:

- `ValidatedAppLivePaperConfig`
- `ValidatedAppLiveLiveConfig`
- `ValidatedAppLiveSmokeConfig`
- `ValidatedAppReplayConfig`

The exact type names are illustrative, but the contract is mandatory:

- one TOML file
- one shared schema
- different validated required subsets depending on binary and mode

For example:

- `app-live` in `paper` mode must not require live signer/source/neg-risk rollout sections
- `app-live` in `live` mode requires the live/runtime sections that current code actually consumes
- `app-live` in real-user shadow smoke mode additionally requires smoke-safe Polymarket source and signer inputs
- `app-replay` must not require live signer/source sections simply because they exist in the shared schema

### 8.4 Missing Versus Empty

The unified config should distinguish between:

- a required field being absent
- a field being present but intentionally empty

Hard rule:

- absent required config is an error
- explicitly empty collections are allowed only when they are semantically meaningful

This avoids the current ambiguity where missing env values may silently behave like empty sets.

This rule must be interpreted relative to the validated view:

- a section may be optional for one consumer view and required for another
- once a section is required for a given validated view, missing fields inside that view are errors

### 8.5 `DATABASE_URL`

`DATABASE_URL` remains outside the TOML file, but it still belongs to the startup validation chain.

Binary startup should therefore be:

1. load TOML config
2. parse and validate TOML config
3. require `DATABASE_URL`
4. only then continue to runtime wiring

This preserves a single fail-fast operator experience even though the DB URL stays in env.

## 9. Binary Entry Contract

### 9.1 `app-live`

`app-live` should move to a CLI contract equivalent to:

- `app-live --config path/to/app-live.toml`

It should stop reading the current business env keys entirely.

That means removing startup parsing of:

- `AXIOM_MODE`
- `AXIOM_NEG_RISK_LIVE_TARGETS`
- `AXIOM_NEG_RISK_LIVE_APPROVED_FAMILIES`
- `AXIOM_NEG_RISK_LIVE_READY_FAMILIES`
- `AXIOM_LOCAL_SIGNER_CONFIG`
- `AXIOM_REAL_USER_SHADOW_SMOKE`
- `AXIOM_POLYMARKET_SOURCE_CONFIG`

### 9.2 `app-replay`

`app-replay` should consume the same unified config path.

Even if it only uses a subset of fields today, it should not maintain a separate configuration reality.

However, `app-replay` must not be forced to load a fully populated live-trading configuration just to replay journal state.

Hard rule:

- `app-replay` consumes a replay-scoped validated view of the shared schema
- replay must not require live signer/source/relayer auth sections unless replay code actually consumes them
- a missing live-trading section in replay mode is not by itself a configuration error

Recommended entrypoint:

- `app-replay --config path/to/app-live.toml --from-seq ...`

### 9.3 `crates/config`

The existing `crates/config` env-driven settings model should not survive as a parallel schema.

The recommended end state is:

- remove it entirely, or
- reduce it to a thin facade over the new `config-schema` crate

It must not remain a second authoritative configuration model.

## 10. Migration Strategy

Even though the user-facing contract is a hard switch, implementation should proceed in a controlled order:

1. add `crates/config-schema`
2. encode raw TOML schema and validated config model
3. switch `app-live` to `--config`
4. switch `app-replay` to `--config`
5. remove old `AXIOM_*` and `POLY_*` business-config loading paths
6. collapse or remove old `crates/config` env schema
7. update `.env.example`, README, runbooks, and helper scripts

This is still a one-step architectural cutover from the user perspective:

- after the change lands, the old business config inputs are gone

But it prevents the implementation from becoming a risky all-at-once rewrite inside one patch.

## 11. Testing Strategy

### 11.1 Config-Schema Tests

Add dedicated tests for:

- valid TOML parse
- invalid TOML parse
- missing required sections
- invalid URLs and enums
- duplicate family definitions
- invalid target/member references
- invalid smoke/runtime combinations

### 11.2 Binary Entrypoint Tests

`app-live` and `app-replay` should both test:

- successful startup with `--config`
- fail-fast on missing config path
- fail-fast on invalid config
- fail-fast on missing `DATABASE_URL`
- no fallback to legacy env business config
- per-binary requiredness is enforced correctly
- replay does not require live-only sections

### 11.3 Documentation Drift Tests

The repository should verify that:

- `.env.example` no longer advertises removed business-config env surfaces as primary startup config
- runbooks and helper scripts use the unified TOML path
- smoke helper flows reflect the new configuration contract

### 11.4 End-To-End Wiring Tests

At least one integration test path should verify that a single TOML file can drive:

- `app-live` in paper mode
- `app-live` in live mode
- real-user shadow smoke
- `app-replay` summary startup

Additionally, tests should verify that a minimal replay-scoped config can succeed without unrelated live-only sections.

## 12. Acceptance Criteria

This work is complete when:

- a new `crates/config-schema` crate exists and owns the TOML schema, parsing, and validation
- `app-live` loads its runtime/business configuration from `--config <path>`
- `app-replay` loads its configuration from `--config <path>`
- `DATABASE_URL` is the only remaining deployment-level environment variable
- legacy `AXIOM_*` and `POLY_*` business-config startup paths are removed
- the new TOML schema covers current runtime/source/signer/relayer/neg-risk/smoke needs without forcing every consumer to require every section
- the schema crate does not contain runtime adapter construction logic
- `.env.example`, README, runbooks, and helper scripts all reflect the new configuration model
- no second parallel configuration truth remains in `crates/config`

## 13. Example Shape

Illustrative example only:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = true

[polymarket.source]
clob_host = "https://clob.polymarket.com"
data_api_host = "https://data-api.polymarket.com"
relayer_host = "https://relayer-v2.polymarket.com"
market_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
user_ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
heartbeat_interval_seconds = 15
relayer_poll_interval_seconds = 5
metadata_refresh_interval_seconds = 60

[polymarket.signer]
address = "0x..."
funder_address = "0x..."
signature_type = "eoa"
wallet_route = "eoa"
api_key = "..."
passphrase = "..."
timestamp = "..."
signature = "..."

[polymarket.relayer_auth]
kind = "builder_api_key"
api_key = "..."
timestamp = "..."
passphrase = "..."
signature = "..."

[negrisk.rollout]
approved_families = ["family-a"]
ready_families = ["family-a"]

[[negrisk.targets]]
family_id = "family-a"

[[negrisk.targets.members]]
condition_id = "condition-1"
token_id = "token-1"
price = "0.43"
quantity = "5"
```

This example is intended to clarify shape only.

It is not a claim that every optional section must already exist if current code does not consume it.
