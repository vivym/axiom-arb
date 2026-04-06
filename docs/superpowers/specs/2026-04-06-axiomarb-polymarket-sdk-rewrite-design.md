# AxiomArb Polymarket SDK Rewrite Design

- Date: 2026-04-06
- Status: Draft for user review
- Project: `AxiomArb`

## 1. Summary

At current `HEAD`, `AxiomArb` still owns too much Polymarket protocol logic inside `venue-polymarket`:

- authenticated CLOB REST paths are hand-maintained
- L2 auth material is derived locally and treated as static config
- websocket connectivity is repo-owned transport code
- Gamma/Data metadata fetching is repo-owned transport code
- `doctor` and runtime providers depend on those repo-owned protocol details

That ownership is already producing real protocol drift:

- stale REST paths caused `405 Method Not Allowed`
- stale auth semantics caused `401 Unauthorized`
- request construction and response handling are coupled to repo-specific assumptions instead of the official client contract

The recommended fix is a staged internal rewrite of `venue-polymarket` around the official Rust SDK `polymarket-client-sdk`, while keeping `app-live` and the control plane repo-owned.

The key strategy is:

- keep `venue-polymarket` as the only Polymarket integration crate
- rewrite its internals around official SDK-backed capabilities
- build the new protocol core now, but defer main-path cutover until the strategy-neutral control-plane work lands
- delete repo-owned transport/auth main paths only in the final cutover phase
- preserve `app-live` as the control-plane owner
- preserve relayer support inside the same crate, but as a repo-owned backend behind the same gateway surface
- explicitly design the new surface so the `strategy-neutral-control-plane` work can plug into it without another protocol-layer rewrite

In short:

- external boundary stays stable at the crate level
- internal protocol implementation is replaced wholesale, but in two execution phases
- control-plane semantics are not changed in this project

## 2. Current Repository Reality

At current `HEAD`, `venue-polymarket` exports low-level transport concepts directly:

- `L2AuthHeaders`
- derived auth material helpers
- `PolymarketRestClient`
- `PolymarketWsClient`
- request-builder style functions

`app-live` consumes those transport-facing concepts in places that should not need protocol knowledge:

- `doctor` connectivity checks
- runtime submit/reconcile providers
- heartbeat polling
- metadata discovery and refresh

The current config path also bakes protocol details into application state:

- `LocalSignerConfig` stores static `timestamp` and `signature`
- config parsing derives L2 auth material once
- runtime then reuses that precomputed auth instead of signing per request

That model is incompatible with official client semantics and creates the wrong ownership boundary:

- `app-live` ends up indirectly coupled to Polymarket auth details
- `venue-polymarket` leaks transport concepts instead of exposing venue capabilities
- control-plane work and protocol work are harder to separate cleanly

At the same time, another active project is already reshaping the control plane toward route neutrality:

- neutral strategy lineage
- neutral runtime anchors
- route-neutral `status` / `doctor` / `apply` / `verify`

That work touches:

- `config-schema`
- `persistence`
- `app-live` runtime and command surfaces

It does not currently rewrite `venue-polymarket`.

This means the protocol rewrite should be treated as a venue-integration rewrite with strict boundaries, not as an excuse to also redesign the control plane.

## 3. Goals

This design should guarantee the following:

- `venue-polymarket` no longer owns a custom primary implementation of Polymarket authenticated CLOB REST
- `venue-polymarket` no longer owns a custom primary implementation of Polymarket websocket session transport
- `venue-polymarket` no longer owns a custom primary implementation of Polymarket Gamma/Data transport
- `app-live` no longer depends on static L2 auth headers or request-builder APIs
- Polymarket credentials are treated as long-lived credential material, not as pre-signed request artifacts
- one crate, `venue-polymarket`, remains the single venue integration boundary for the repository
- relayer support remains available through that same crate, even if its backend remains repo-owned
- the new `venue-polymarket` public surface is route-agnostic and compatible with future strategy-neutral route adapters
- the rewrite can replace the current smoke blocker path end-to-end:
  - `doctor`
  - metadata discovery
  - runtime submit/reconcile
  - heartbeat
  - websocket feeds
- the work can be split so that low-risk protocol-core work starts immediately, while high-conflict cutover work waits for the strategy-neutral control-plane merge

## 4. Non-Goals

This design does not define:

- a rewrite of the strategy-neutral control plane
- a rewrite of candidate/adoptable/startup lineage
- new readiness semantics for `status`, `apply`, `doctor`, or `verify`
- relayer protocol replacement with an official SDK
- new market selection logic
- new planner or risk logic
- long-term support for legacy static signer semantics
- an immediate pre-merge cutover of all `app-live` call sites to the new gateway

## 5. Approaches Considered

### 5.1 Option A: Patch The Existing Custom Client

Keep the current crate shape and continue repairing:

- endpoints
- auth semantics
- websocket protocol behavior
- metadata fetching

Pros:

- smallest immediate diff
- easiest short-term unblock

Cons:

- continues protocol-drift ownership in-repo
- repeats work already owned by the official SDK
- leaves the wrong abstractions in place
- likely to regress again as upstream evolves

### 5.2 Option B: Replace The Protocol Layer Inside `venue-polymarket`

Keep `venue-polymarket` as the integration crate, but rebuild its internals around the official Rust SDK and a capability-oriented public API.

Pros:

- one-time cleanup of the protocol boundary
- minimal control-plane blast radius
- no dual-stack migration debt
- compatible with the future strategy-neutral control plane

Cons:

- substantial internal rewrite
- requires public API breakage inside the crate
- requires broad test rewiring

### 5.3 Option C: Introduce A New Polymarket Integration Crate

Create a new crate and migrate callers to it, leaving the old crate behind temporarily.

Pros:

- cleanest conceptual separation
- easiest to compare old and new behavior

Cons:

- leaves temporary duplicate integration layers
- creates migration glue and delayed cleanup
- conflicts with the goal of not carrying historical baggage

### 5.4 Recommendation

Choose Option B, but execute it in two phases.

This gives the repository one Polymarket boundary, one protocol implementation path, and no long-lived migration shell, while avoiding unnecessary rework against the in-flight strategy-neutral branch.

## 6. Architecture Decision

### 6.1 Top-Level Decision

Keep `venue-polymarket` as the public integration crate, but redefine it as a capability-oriented gateway over:

- official SDK-backed CLOB REST
- official SDK-backed websocket access
- official SDK-backed Gamma/Data access
- repo-owned relayer access

The crate should stop presenting transport primitives as its public identity.

### 6.2 New Internal Layers

The recommended internal structure is:

- `gateway`
  - top-level assembly and dependency ownership
- `sdk_backend`
  - wraps `polymarket-client-sdk`
  - owns authenticated CLOB REST, websocket sessions, Gamma/Data access, and SDK-specific auth/session setup
- `relayer_backend`
  - repo-owned relayer transport and mapping
- `providers`
  - repo-owned higher-level adapters used by `app-live`
  - order execution
  - metadata refresh
  - market/user stream projection
  - heartbeat projection
- `mapping`
  - translation from SDK-native responses and events into repo-owned stable domain types
- `errors`
  - venue-level error model that hides SDK-specific details from upper layers while preserving detail

### 6.3 What Gets Deleted

The new design should remove the old primary abstractions, not preserve them:

- static L2 auth header types as a public concept
- repo-owned request-builder APIs
- repo-owned generic REST client API as the main abstraction
- repo-owned websocket client API as the main abstraction
- repo-owned per-request auth derivation helpers as an app-facing concept

## 7. Public Surface

The new public API should be organized around venue capabilities, not transports.

Recommended public capability surface:

- `PolymarketGateway`
  - top-level construction and shared dependency owner
- `PolymarketExecutionSource`
  - submit, cancel, open orders, allowance/balance, heartbeat
- `PolymarketMetadataSource`
  - discovery and refresh of Gamma/Data metadata needed by repo workflows
- `PolymarketMarketStreamSource`
  - market feed subscription and projected repo-owned market events
- `PolymarketUserStreamSource`
  - user feed subscription and projected repo-owned user/order/trade events
- `PolymarketRelayerSource`
  - relayer reachability and transaction access

Hard rules:

- upper layers must not construct SDK clients directly
- upper layers must not construct transport headers directly
- upper layers must not know request paths, auth formulas, or websocket auth payload details
- capability APIs must not encode `neg-risk` as the only live route

The route-agnostic requirement applies to the venue gateway boundary, not to every route-owned payload adapter above it.

That means:

- auth, transport, connectivity, market data, and stream APIs must not encode `neg-risk`
- route adapters may still translate route-owned execution payloads into venue-facing calls above the gateway

## 8. Auth And Config Model

### 8.1 Replace Static Signer Semantics

The current static-signer model should be removed from the mainline path.

Config should represent long-lived credential material only:

- address
- funder address
- signature type
- wallet route
- API key
- secret
- passphrase

Runtime/session objects should derive authenticated SDK behavior dynamically per request or session.

### 8.2 New Configuration Ownership

`config-schema` and `app-live` should distinguish:

- credential material
- venue connection settings
- runtime cadence/proxy settings

They should not persist:

- pre-signed timestamp values
- precomputed request signatures

### 8.3 Legacy Static Signer Path

Because the project goal is to avoid historical baggage, the old static signer path should not survive as a long-term compatibility mode.

Recommended handling:

- Phase A:
  - do not extend it
  - do not route any new protocol-core code through it
  - keep parse-time support only where needed to avoid creating an unnecessary merge hotspot before strategy-neutral lands
- Phase B:
  - remove it from normal runtime construction
  - reject it explicitly during config loading or validation if still present
  - update docs and examples so `[polymarket.account]` is the only supported mainline authenticated path

## 9. Contract With Strategy-Neutral Control Plane

This rewrite must treat the strategy-neutral control-plane work as an architectural constraint.

### 9.1 What This Rewrite May Change

- `crates/venue-polymarket/*`
- Polymarket-specific config parsing and validation fields related to credentials, source hosts, websocket URLs, and proxy settings
- app wiring where `app-live` constructs Polymarket venue providers
- Polymarket-specific docs and runbooks

### 9.2 What This Rewrite Must Not Redesign

- neutral lineage and persistence
- `operator_strategy_revision`
- route-neutral readiness semantics
- route-neutral `status`, `apply`, `doctor`, and `verify` product behavior
- neutral run-session anchor semantics
- route bundling and control-plane adoption logic

### 9.3 Compatibility Requirement

The new venue gateway must be route-agnostic so that future route adapters can consume it without another protocol rewrite.

That means:

- no `neg-risk`-only naming in capability boundaries
- no family-shaped assumptions in the top-level public API
- no control-plane-specific revision language inside venue integration types

### 9.4 Known Overlap Zones

Even with the intended boundary, later merge work is still expected in a few places:

- `crates/config-schema/src/validate.rs`
- `crates/app-live/src/config.rs`
- `crates/app-live/src/commands/init/render.rs`
- `crates/app-live/src/source_tasks.rs`
- `crates/app-live/src/commands/doctor/connectivity.rs`
- shared config and command test fixtures

These are not reasons to stop the rewrite, but they are explicit merge hotspots and should be treated as deferred cutover work rather than Phase A targets.

## 10. Runtime And Command Integration

The rewrite should change `app-live` only where it currently depends on protocol details.

### 10.0 Order Signing Ownership

This rewrite must preserve the current repository ownership split unless and until a separate design changes it.

Current ownership is:

- repo-owned execution code produces signed order payload material
- venue code translates that signed payload into Polymarket submission format and transmits it

This project should keep that split.

That means:

- the SDK becomes the transport/auth/session owner
- the repository remains the owner of execution-plan signing and signed-order intent construction
- moving order construction or signing authority into the SDK is out of scope for this rewrite

### 10.0.1 Venue-Facing Signed Payload Boundary

The gateway must not consume `SignedFamilySubmission` directly.

Instead, the repository should introduce stable venue-facing Polymarket DTOs above the gateway, for example:

- `PolymarketSignedOrder`
- `PolymarketCancelOrder`
- `PolymarketOrderQuery`

Required rules:

- route-specific payloads such as `SignedFamilySubmission` stay above the gateway
- route adapters or repo-owned providers translate route-owned payloads into these venue-facing DTOs
- the gateway consumes only Polymarket-facing signed order primitives, never `neg-risk` family semantics directly

This preserves the current repo-owned signing boundary while keeping the gateway route-agnostic.

### 10.1 `doctor`

`doctor` should consume `PolymarketExecutionSource` and `PolymarketRelayerSource` capability probes.

It should not know:

- auth headers
- endpoint paths
- raw websocket handshake details

### 10.2 Runtime Submit/Reconcile

Runtime providers should consume `PolymarketExecutionSource`.

They may remain repo-owned provider types, but they should no longer store transport-facing auth header payloads.

For reconciliation truth:

- order-reference reconciliation may continue to use authenticated order visibility
- transaction-reference reconciliation may continue to use relayer-backed visibility
- any future attempt to unify those truths should be a separate design decision, not an implicit side effect of the SDK rewrite

### 10.3 Async Boundary

This rewrite should not expand into a repository-wide async trait conversion.

Recommended rule:

- Phase A may introduce async-native gateway internals
- Phase A should keep the current sync-facing execution/provider traits intact
- Phase B should replace the current ad hoc Tokio bridging with a deliberate shared adapter, but only after the strategy-neutral merge reduces surrounding churn

Hard rules:

- constructing a fresh Tokio runtime per provider call is not an acceptable end state
- the preferred Phase B shape is a shared async adapter behind the existing sync-facing seams
- widening `execution` or `app-live` traits to async should happen only if the shared-adapter approach proves insufficient and only under a separate explicit design decision

### 10.4 Metadata Discovery

Discovery should consume `PolymarketMetadataSource`.

The repository may keep its own policy on:

- malformed row handling
- pagination summaries
- caching
- refresh cadence

But the transport and session semantics should come from the SDK backend.

### 10.5 Market And User Streams

Runtime stream tasks should consume `PolymarketMarketStreamSource` and `PolymarketUserStreamSource`.

The repository should continue to expose repo-owned stable parsed event types upward, but those events should now be mapped from SDK-driven sessions rather than repo-owned raw websocket transport.

## 11. Error Model

The rewrite should consolidate transport-facing errors into a venue-level model.

Recommended categories:

- `Auth`
  - invalid or missing credentials
- `Connectivity`
  - transport failure, timeout, DNS, TLS, proxy, websocket connect failures
- `UpstreamResponse`
  - non-success upstream response with preserved status/body context
- `Protocol`
  - malformed or incompatible SDK/upstream payloads
- `Policy`
  - repo-owned safety checks such as malformed metadata row handling
- `Relayer`
  - relayer-specific failures

Upper layers should receive:

- stable categories
- actionable messages
- preserved low-level context where needed

They should not need to pattern-match SDK internals directly.

## 12. Implementation Shape

This is a one-time rewrite, but it should be executed in two phases.

### 12.1 Phase A: Build The Protocol Core Now

Phase A is allowed to start before the strategy-neutral control-plane branch merges.

The goal is to build the new protocol core with minimal merge risk.

Phase A should stay concentrated in `crates/venue-polymarket/*` and related tests.

#### Slice A1: Define The New Public Surface

- create the new capability-oriented types in `venue-polymarket`
- keep old public exports available during Phase A, but mark them as legacy cutover shell only
- define the stable repo-owned output models and error categories

This slice sets the target architecture before wiring SDK details.

Hard rule:

- Phase A must not break existing `app-live` call sites merely by removing old exports
- true public-export removal belongs to Phase B cutover

#### Slice A2: Replace Authenticated CLOB REST Internals

Integrate official SDK-backed support for:

- status and connectivity probes where applicable
- open orders
- submit and cancel
- balance and allowance
- heartbeat

This slice addresses the current live blocker first.

#### Slice A3: Replace Websocket Internals

Integrate official SDK-backed market and user stream sessions, then map them into the repo-owned event types already consumed by runtime tasks.

#### Slice A4: Replace Gamma/Data Metadata Internals

Integrate official SDK-backed metadata fetches and retain repo-owned policy above that transport layer:

- caching
- malformed row treatment
- cadence
- operator-visible diagnostics

#### Slice A5: Preserve A Controlled Compatibility Shell

Until Phase B, the repository may temporarily keep old wiring alive at the `app-live` boundary.

Hard rules:

- no new features should be added to the old transport path
- old wiring exists only as a short-lived cutover shell
- tests for the new gateway must not depend on the old auth/header/request-builder APIs

### 12.2 Phase B: Cut Over After Strategy-Neutral Merge

Phase B should begin only after the strategy-neutral control-plane branch is merged or otherwise stabilized enough that the overlapping files are no longer moving underneath the rewrite.

#### Slice B1: Rewire `app-live`

Move all Polymarket call sites in `app-live` to capability-oriented construction:

- doctor probes
- runtime provider construction
- discovery wiring
- websocket task construction

#### Slice B2: Remove Old Auth And Transport Paths

- delete the old transport path
- remove static signer mainline support
- remove stale public exports and request-builder APIs
- collapse app-side wiring to the new gateway only

After this slice, the old transport path should be deleted.

## 13. Testing Strategy

The rewrite should be verified at three layers.

### 13.1 Unit Tests

- config-to-credential mapping
- error categorization
- SDK response/event mapping
- malformed metadata policy handling
- relayer gateway mapping

### 13.2 Integration Tests

- `doctor` authenticated connectivity
- discovery metadata refresh
- submit/reconcile behavior
- heartbeat polling
- market/user stream message projection

### 13.3 End-To-End Operator Flows

At minimum:

- `bootstrap` on smoke config reaches discovery and authenticated probes through the new gateway
- `doctor` surfaces auth vs connectivity failures correctly
- `apply --start` can pass through submit/reconcile/stream setup without using repo-owned protocol auth

## 14. Merge Risk Management

Because the strategy-neutral control-plane project is already active in another worktree, this rewrite should optimize for semantic non-overlap.

Recommended rules:

- keep most code churn inside `crates/venue-polymarket/*`
- keep Phase A `app-live` edits near zero
- keep Phase B `app-live` edits focused on provider/gateway wiring only
- avoid rewriting `status`, `apply`, `doctor`, or `verify` product semantics beyond the Polymarket transport boundary
- avoid introducing new control-plane concepts in config and persistence

If these rules are followed, later merge conflicts should collapse mostly to:

- constructor wiring
- config field reshaping
- test fixture updates
- removal of the temporary compatibility shell

rather than architecture-level rework.

## 15. Success Criteria

This project is complete when:

- `venue-polymarket` no longer exposes stale custom protocol primitives as its main public API
- authenticated CLOB REST, websocket access, and Gamma/Data metadata access all run through the official Rust SDK backend
- relayer remains available behind the same venue gateway surface
- `app-live` no longer relies on static L2 auth material or direct transport request construction
- the current real-user smoke path can authenticate and progress using the rewritten gateway
- the resulting venue boundary is compatible with the strategy-neutral control-plane project without another venue rewrite
- Phase A delivers reusable SDK-backed protocol core without painting the repository into a pre-merge cutover corner
- Phase B completes the actual mainline switch and old-path deletion after the control-plane branch lands
