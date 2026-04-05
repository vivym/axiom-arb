# AxiomArb Polymarket Source Defaults Design

## Goal

Make the Polymarket source config truly optional for normal operator-facing `live` / `real-user shadow smoke` configs so the example config and generated local configs no longer force operators to hand-author default endpoints and intervals.

## Problem

Today the system already has an implicit default Polymarket source shape in the `init` rendering path, but the operator-facing smoke validation path and runtime consumers still treat explicit source config as required truth. That creates three different truths:

1. `init` knows a default source
2. the example config expands the default source explicitly
3. smoke validation and runtime consumers still require an explicit source block to exist

That makes the example larger than necessary and leaks implementation defaults into the operator-facing config surface.

## Scope

This change only covers Polymarket source defaulting for the operator-facing live/smoke path.

It does not change:
- `funder_address` requiredness
- `[negrisk.target_source]`
- `[negrisk.rollout]`
- partial override semantics for source fields
- signer-based legacy live config behavior

## Design

### 1. Default source belongs in config-schema / validated view

The default Polymarket source must live in the config-schema / validated-config layer, not only in `init` rendering and not only in runtime fallback code.

This keeps a single truth for:
- `init`
- `doctor`
- `run`
- `bootstrap`
- `apply`
- `verify`

Callers should observe a fully populated source configuration through the validated view even when the raw config omits both `[polymarket.source]` and `[polymarket.source_overrides]`.

The validated-config contract should make this explicit instead of relying on every caller to invent its own fallback. In practice that means adding or changing a validated accessor so runtime consumers can ask for the effective source config directly, rather than continuing to interpret a raw `Option` as both "missing" and "defaulted".

The existing raw-borrowing source view should remain for raw-presence and introspection use:
- raw presence helpers should continue to answer whether the config explicitly supplied `source` or `source_overrides`
- the new effective/defaulted accessor should answer what runtime should actually use

The operator-facing runtime path should consume the defaulted accessor as its single truth.

### 2. Default field set

The default source supplies the full current operator default set:

- `clob_host`
- `data_api_host`
- `relayer_host`
- `market_ws_url`
- `user_ws_url`
- `heartbeat_interval_seconds`
- `relayer_poll_interval_seconds`
- `metadata_refresh_interval_seconds`

These values match the current hard-coded `init` defaults.

### 3. Effective source and override rules

The rule is intentionally simple:

- If `[polymarket.source_overrides]` is present, use that explicit block.
- Else if `[polymarket.source]` is present, use that explicit block.
- Else use the built-in defaults.
- Partial merge is out of scope for this change.

This preserves the current precedence between `source_overrides` and `source`.

That also means:
- a partially specified `[polymarket.source]` remains invalid
- a partially specified `[polymarket.source_overrides]` remains invalid
- the system will not mix user-specified fields with default-filled remainder values

### 4. Runtime behavior

`app-live` should no longer treat a missing source block as a normal configuration error in the operator-facing `live` / `smoke` path. Instead, it should consume the validated/defaulted source.

This first cut does **not** change the signer-based legacy live path. That path continues to require an explicit source block and remains outside the operator-facing UX surface this design is trying to simplify.

Explicit invalid source values remain fail-closed:
- malformed hosts or websocket URLs
- unsupported schemes
- non-positive intervals

Defaults make omission legal; they do not make bad explicit values legal.

### 5. Init and example changes

Once the validated layer owns the defaults:

- `init` should stop rendering the default `[polymarket.source]` block into newly generated local configs.
- `config/axiom-arb.example.toml` should stop expanding the default source block.
- `init` summary output should stop listing `[polymarket.source]` as a default-written section.
- The example may keep a short comment explaining that operators normally omit source config entirely and should prefer `[polymarket.source_overrides]` only when overriding the default endpoints/intervals or cadence.

This reduces operator-visible config surface while preserving the ability to override endpoints deliberately.

## Compatibility

### Existing explicit configs

Existing configs that already include `[polymarket.source]` must behave exactly as before.

Existing configs that use `[polymarket.source_overrides]` must also behave exactly as before, including the current precedence over `[polymarket.source]`.

### Missing source block

Operator-facing live/smoke configs that omit both `[polymarket.source]` and `[polymarket.source_overrides]` become newly legal and should resolve to the built-in default source.

Signer-based legacy live configs remain unchanged and still require an explicit source block in this first cut.

### Partial source block

A partial source block remains invalid. This change does not introduce field-level merge semantics.

## Error Handling

- Missing `[polymarket.source]` and `[polymarket.source_overrides]` in operator-facing live/smoke configs: valid, defaulted.
- Explicit invalid `[polymarket.source]`: error.
- Explicit invalid `[polymarket.source_overrides]`: error.
- Partial `[polymarket.source]`: error.
- Partial `[polymarket.source_overrides]`: error.

## Testing

At minimum, add coverage for:

1. validation accepts operator-facing live/smoke configs with no `[polymarket.source]` and no `[polymarket.source_overrides]`
2. explicit full `[polymarket.source]` blocks still validate
3. explicit full `[polymarket.source_overrides]` blocks still validate and continue to win over `[polymarket.source]`
4. partial source blocks still fail
5. invalid explicit source values still fail
6. signer-based legacy live configs still require explicit source config
7. `app-live` builds a full `PolymarketSourceConfig` from defaults when the source block is omitted on the operator-facing path
8. raw presence helpers continue to distinguish "explicitly supplied source" from "defaulted effective source"
9. `init` no longer renders the default source block
10. `init` summary no longer claims `[polymarket.source]` was written by default
11. `config/axiom-arb.example.toml` remains a valid high-level skeleton after source removal

## Acceptance Criteria

- `polymarket.source` / `polymarket.source_overrides` are fully optional in normal operator-facing `live` / `smoke` configs
- defaults come from one place: config-schema / validated view
- runtime consumers use a single defaulted validated accessor instead of re-implementing source fallback locally
- explicit source configs remain behaviorally unchanged
- `source_overrides` retains precedence over `source`
- signer-based legacy live configs remain unchanged in this first cut
- partial source blocks are still rejected
- invalid explicit source values still fail-closed
- `init` stops expanding the default source block
- `init` summary stops advertising `[polymarket.source]` as a default-written section
- `config/axiom-arb.example.toml` no longer requires operators to hand-author default endpoints/intervals
