# AxiomArb Polymarket Proxy Removal and L2 Probe Design

Date: 2026-04-08
Status: Proposed

## Context

The current `main` branch still carries two behaviors that should not survive the Polymarket Phase B cleanup:

1. `proxy_url` is modeled as an app config field and drives transport selection.
2. `doctor` can still fall back to a legacy CLOB REST path that uses the old repo-owned L2 auth derivation instead of the current Polymarket spec.

This is the wrong long-term shape.

- Proxy behavior should not be encoded in `config/axiom-arb.local.toml`; it should be driven by process environment.
- `doctor` should not require `POLYMARKET_PRIVATE_KEY` just to prove authenticated REST reachability for CLOB endpoints that are fundamentally L2-authenticated.
- The repo should not keep an app-facing legacy CLOB REST path alive just because the official Rust SDK authenticated client is signer-shaped.

At the same time, the repository cannot delete the entire `PolymarketRestClient` today because relayer access still depends on it and there is no replacement relayer SDK path in scope.

## Goals

- Remove `proxy_url` from the app/config schema and runtime wiring.
- Remove app-facing legacy CLOB REST fallback behavior.
- Keep relayer HTTP support intact.
- Keep websocket support intact without introducing a new websocket connector project in this slice.
- Restore a correct `doctor` authenticated REST probe that does not require `POLYMARKET_PRIVATE_KEY`.

## Non-Goals

- Replacing the relayer HTTP implementation.
- Rewriting websocket transport to support explicit env-proxy tunneling at the SDK connector level.
- Reworking live submit ownership or order-signing boundaries.
- Reintroducing a wide custom REST client as the mainline Polymarket abstraction.

## Design

### 1. Remove `proxy_url` as a first-class config concept

Delete the Polymarket HTTP proxy field from config parsing and validated runtime config.

After this change:

- `PolymarketSourceConfig` no longer contains `outbound_proxy_url`.
- `config/axiom-arb.local.toml` no longer documents or accepts `[polymarket.http].proxy_url`.
- Process-level proxy behavior comes only from environment variables such as `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY`.

This is an explicit simplification. It accepts that the operator must configure process environment correctly.

### 2. Delete app-facing legacy CLOB REST fallback

Remove the remaining app-facing branches that pick a legacy CLOB REST backend because a proxy was configured or because a signer-shaped SDK client is inconvenient.

This includes:

- `app-live` probe fallback for authenticated CLOB REST.
- metadata fallback from SDK to legacy REST.
- submit-path fallback from SDK-backed runtime behavior to legacy CLOB REST.

After this change:

- authenticated CLOB runtime paths no longer branch on proxy config.
- metadata no longer branches on proxy config.
- the old repo-owned L2 auth derivation is no longer part of any app-facing CLOB path.

### 3. Add a narrow `venue-polymarket` current-spec L2 probe client

Add a new narrow probe module in `venue-polymarket`, for example:

- `crates/venue-polymarket/src/l2_probe.rs`

This module is intentionally small and probe-specific. It is not a resurrection of the old `rest.rs`.

Its responsibilities are limited to:

- `GET /data/orders`
- `POST /v1/heartbeats`

Its inputs are limited to:

- CLOB host
- API key
- secret
- passphrase

Its implementation should:

- use the current Polymarket L2 HMAC rules
- use `reqwest`
- rely on environment proxy behavior only
- map failures into the existing connectivity/protocol categories expected by `doctor`

The L2 HMAC implementation must not be left implicit.

It should match the current Polymarket SDK/documentation behavior:

- signature payload is constructed from `timestamp + method + requestPath + body`
- `method` is the uppercase HTTP method
- `requestPath` is the path plus query string exactly as sent on the wire
- `body` is the exact request body bytes rendered as a UTF-8 string, or the empty string for requests without a body
- the stored `secret` is decoded according to the current Polymarket credential format before HMAC calculation
- the resulting HMAC-SHA256 digest is encoded in the same format expected by current Polymarket L2 headers

The implementation should be verified against the official current-spec reference behavior, not against the repository's previous `derive_l2_auth_material` output.

It should not:

- create or derive API credentials
- sign order payloads
- expose a wide request-builder surface
- absorb relayer behavior
- depend on `POLYMARKET_PRIVATE_KEY`

`doctor` should stop routing its authenticated CLOB probe through `LocalSignerConfig`.

Instead, `doctor` should split its probe inputs into:

- a narrow L2 credential DTO for authenticated CLOB probe calls
- existing relayer auth / signer-derived material for relayer reachability

This keeps the CLOB probe aligned with the protocol while allowing relayer checks to continue using the separate relayer auth path that already exists in the repository.

### 4. Keep the protocol split explicit

After this slice, Polymarket integration should be split as follows:

- SDK-backed gateway:
  - metadata
  - runtime submit/sign path
  - SDK-backed websocket path where already used
- Narrow probe client:
  - authenticated CLOB reachability for `doctor`
- Legacy HTTP shell:
  - relayer only

This is a deliberate architecture boundary. The probe client exists because the official Rust SDK authenticated client currently assumes a signer-shaped workflow, while `doctor` needs an L2-only connectivity check.

### 5. `POLYMARKET_PRIVATE_KEY` behavior after this change

After this slice:

- `doctor` authenticated REST probe must not require `POLYMARKET_PRIVATE_KEY`
- real live submit paths may still require `POLYMARKET_PRIVATE_KEY`
- real-user shadow smoke should remain able to validate connectivity without forcing an order-signing key into the environment

This matches the Polymarket protocol model:

- L1 private key for key derivation and order signing
- L2 credentials for authenticated CLOB request authorization

### 6. Websocket backend selection after `proxy_url` removal

Removing `proxy_url` does not mean websocket selection becomes unconditional SDK usage.

After this slice:

- websocket backend selection must no longer depend on config-provided proxy state
- websocket backend selection may still choose the environment-aware websocket shell when:
  - market and user websocket base endpoints differ
  - or process proxy environment is present and the SDK websocket path would not preserve equivalent behavior

This is necessary because the existing websocket shell already honors environment-derived HTTP proxy settings, while the current SDK websocket path does not provide equivalent env-proxy behavior.

The intent of this slice is:

- remove config-driven proxy branching
- not regress websocket behavior for operators who rely on environment proxy settings

### 7. Runtime provider boundary tightening

This slice must not stop at `app-live` callsites.

`venue-polymarket` runtime providers that currently still accept `PolymarketRestClient` and `L2AuthHeaders` as part of their mainline CLOB constructor shape must be tightened as part of this work.

In particular:

- gateway-backed submit/reconcile constructors should no longer preserve a mainline legacy CLOB execution dependency
- any remaining `PolymarketRestClient` usage in runtime submit/reconcile must be justified as relayer-only support, not as a fallback CLOB transport

The repository must not land in a half-migrated state where:

- `app-live` no longer directly chooses legacy CLOB REST
- but `venue-polymarket` providers still quietly keep a legacy CLOB fallback alive behind gateway-backed constructors

## File-Level Impact

### Remove or reshape

- `crates/config-schema/src/raw.rs`
- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/config.rs`
- `crates/app-live/src/polymarket_probe.rs`
- `crates/app-live/src/polymarket_runtime_adapter.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- `crates/venue-polymarket/src/negrisk_live.rs`

### Add

- `crates/venue-polymarket/src/l2_probe.rs`

### Retain, but with narrower responsibility

- `crates/venue-polymarket/src/rest.rs`
  - relayer/test support only
- `crates/venue-polymarket/src/relayer.rs`
- `crates/venue-polymarket/src/sdk_backend/relayer.rs`
- `crates/venue-polymarket/src/ws_client.rs`

## Risks

### 1. Environment-only proxy behavior is less explicit

True. This slice deliberately accepts that tradeoff in order to remove the config-driven legacy branch and simplify the runtime model.

### 2. Websocket proxy behavior remains imperfect

Also true. This slice does not solve SDK websocket env-proxy parity. It only ensures that:

- websocket behavior no longer depends on `proxy_url`
- websocket transport continues to use the existing environment-aware shell where applicable

### 3. A new narrow probe client adds another venue integration seam

Yes, but this is acceptable because it is intentionally narrow and bounded. It is not a second general-purpose Polymarket client.

## Verification

This slice is complete when all of the following are true:

1. `proxy_url` is no longer accepted in config parsing or runtime config.
2. `doctor` authenticated REST probe succeeds or fails through the new current-spec L2 probe path, not through legacy L2 auth derivation.
3. `doctor` no longer requires `POLYMARKET_PRIVATE_KEY` for authenticated REST probe success.
4. metadata and submit mainline paths no longer branch on proxy config.
5. websocket probe behavior remains intact under environment-proxy setups without relying on config-driven proxy branching.
6. relayer reachability still works.
7. real-user shadow smoke remains able to reach `doctor` and startup without reviving the legacy CLOB REST path.

## Operator Notes

After this change, operators who need outbound HTTP proxying should set environment variables such as:

```bash
export HTTP_PROXY=http://127.0.0.1:7897
export HTTPS_PROXY=http://127.0.0.1:7897
```

This spec does not guarantee support for `socks5://...` proxy URLs.
