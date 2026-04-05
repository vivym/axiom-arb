# AxiomArb Polymarket Source Defaults Design

## Goal

Make `[polymarket.source]` truly optional for normal `live` / `real-user shadow smoke` operator configs so the example config and generated local configs no longer force operators to hand-author default endpoints and intervals.

## Problem

Today the system already has an implicit default Polymarket source shape in the `init` rendering path, but runtime validation still treats `polymarket.source` as required. That creates three different truths:

1. `init` knows a default source
2. the example config expands the default source explicitly
3. schema/runtime still require the source block to exist

That makes the example larger than necessary and leaks implementation defaults into the operator-facing config surface.

## Scope

This change only covers `polymarket.source`.

It does not change:
- `funder_address` requiredness
- `[negrisk.target_source]`
- `[negrisk.rollout]`
- partial override semantics for source fields

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

Callers should observe a fully populated source configuration through the validated view even when the raw config omits `[polymarket.source]` entirely.

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

### 3. Override rules

The rule is intentionally simple:

- If `[polymarket.source]` is absent, use the built-in defaults.
- If `[polymarket.source]` is present, use the explicit user-supplied block.
- Partial merge is out of scope for this change.

That means a partially specified `[polymarket.source]` remains invalid. The system will not mix user-specified fields with default-filled remainder values.

### 4. Runtime behavior

`app-live` should no longer treat a missing source block as a normal configuration error in the common `live` / `smoke` path. Instead, it should consume the validated/defaulted source.

Explicit invalid source values remain fail-closed:
- malformed hosts or websocket URLs
- unsupported schemes
- non-positive intervals

Defaults make omission legal; they do not make bad explicit values legal.

### 5. Init and example changes

Once the validated layer owns the defaults:

- `init` should stop rendering the default `[polymarket.source]` block into newly generated local configs.
- `config/axiom-arb.example.toml` should stop expanding the default source block.
- The example may keep a short comment explaining that `[polymarket.source]` is only needed when overriding the default endpoints/intervals.

This reduces operator-visible config surface while preserving the ability to override endpoints deliberately.

## Compatibility

### Existing explicit configs

Existing configs that already include `[polymarket.source]` must behave exactly as before.

### Missing source block

Configs that omit `[polymarket.source]` become newly legal and should resolve to the built-in default source.

### Partial source block

A partial source block remains invalid. This change does not introduce field-level merge semantics.

## Error Handling

- Missing `[polymarket.source]`: valid, defaulted.
- Explicit invalid `[polymarket.source]`: error.
- Partial `[polymarket.source]`: error.

## Testing

At minimum, add coverage for:

1. validation accepts live/smoke configs with no `[polymarket.source]`
2. explicit full source blocks still validate
3. partial source blocks still fail
4. invalid explicit source values still fail
5. `app-live` builds a full `PolymarketSourceConfig` from defaults when the source block is omitted
6. `init` no longer renders the default source block
7. `config/axiom-arb.example.toml` remains a valid high-level skeleton after source removal

## Acceptance Criteria

- `polymarket.source` is fully optional in normal `live` / `smoke` configs
- defaults come from one place: config-schema / validated view
- explicit source configs remain behaviorally unchanged
- partial source blocks are still rejected
- invalid explicit source values still fail-closed
- `init` stops expanding the default source block
- `config/axiom-arb.example.toml` no longer requires operators to hand-author default endpoints/intervals
