# 2026-04-09 AxiomArb Main Entrypoint Non-EOA Live Test Fix Design

## Goal

Repair the current `main_entrypoint` baseline break by updating stale runtime tests to match the repository's current live-wallet contract:

- `EOA + non-shadow live` is fail-closed
- non-shadow live runtime tests that expect successful live execution must use a non-EOA config and provide `POLYMARKET_PRIVATE_KEY`

## Why This Fix Exists

`crates/app-live/tests/main_entrypoint.rs` still treats `fixtures/app-live-live.toml` as a successful non-shadow live startup fixture. That fixture is intentionally:

- `signature_type = "eoa"`
- `wallet_route = "eoa"`
- `real_user_shadow_smoke = false`

Recent runtime changes in `crates/app-live/src/commands/run.rs` explicitly reject that path:

- if non-shadow live work is requested
- and wallet kind does not require relayer
- fail with:
  - `EOA non-shadow live runtime is not supported; relayer-backed runtime work requires a non-EOA wallet kind`

This behavior is already locked by `crates/app-live/tests/run_command.rs`, so the baseline break is in the stale `main_entrypoint` tests, not in runtime behavior.

## Desired End State

- Shared fixture `crates/config-schema/tests/fixtures/app-live-live.toml` remains an EOA live fixture for config/schema validation.
- `crates/app-live/tests/main_entrypoint.rs` uses a test-local transformed config for the two runtime-entry tests that expect successful non-shadow live execution or stale-runtime-progress mismatch behavior.
- That transformed config must:
  - switch wallet kind from EOA to non-EOA
  - add a relayer auth block
  - normalize SDK-facing account values the same way other live runtime tests already do
  - replace production Polymarket hosts with a mock/local venue override so the tests do not hit real network endpoints
- The test database seed used by those runtime-entry tests must also normalize route artifacts / rendered live targets to SDK-valid values:
  - hex `condition_id`
  - numeric `token_id`
  - otherwise the live signer/backend path can fail before the intended assertions run
- Those two tests must set `POLYMARKET_PRIVATE_KEY` so they exercise the intended non-shadow live path instead of failing earlier on missing signer material.

## Non-Goals

- Do not loosen the runtime EOA fail-closed gate.
- Do not change the shared config-schema fixture shape.
- Do not broaden into canonical strategy-control rewrite work.

## Implementation Notes

- Reuse the same non-EOA normalization pattern already present in `crates/app-live/tests/apply_command.rs` and `crates/app-live/tests/run_command.rs`.
- Prefer the `apply_command` live-runtime pattern over the simpler `run_command` path:
  - non-EOA config transform
  - SDK-valid target data
  - mock doctor/source overrides
  - fixed test private key
- Keep the change local to `crates/app-live/tests/main_entrypoint.rs` unless a tiny shared helper is clearly justified.
- The repaired tests should continue asserting:
  - successful live startup persists `operator_strategy_revision`
  - stale restored runtime progress still fails with `operator strategy revision anchor mismatch`

## Acceptance Criteria

1. `cargo test -p app-live --test main_entrypoint -- --test-threads=1` passes.
2. The fix does not weaken `EOA non-shadow live` fail-closed behavior.
3. Shared fixture `crates/config-schema/tests/fixtures/app-live-live.toml` remains EOA-shaped.
4. The repaired tests do not depend on live network/authentication side effects; they succeed or fail only on the intended local startup/runtime conditions.
