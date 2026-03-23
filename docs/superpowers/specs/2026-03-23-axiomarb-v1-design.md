# AxiomArb V1 Design

- Date: 2026-03-23
- Status: Draft for user review
- Project: `AxiomArb`
- Venue: `Polymarket`

## 1. Summary

`AxiomArb` is a live-capable arbitrage engine for Polymarket.

The system is designed around four priorities:

1. Correct local state
2. Recoverable execution
3. Replayable history
4. Incremental strategy rollout

This is not a "general multi-venue trading platform" in v1. It is a single-engine, single-venue system with a narrow first live release and a larger second release on the same core architecture.

## 2. Assumptions And Hard Constraints

This design is written under the following assumptions:

- Deployment is compliant and allowed to trade live.
- A live wallet, signing path, API credentials, and funding path are available.
- The system must support real-money trading, not only paper trading.

The following constraints are hard requirements:

- `v1a` should be able to trade live without routine manual inventory operations.
- a documented break-glass manual path must exist for `cancel-all`, inventory quarantine, and relayer recovery.
- `v1a` identifiers, routes, and inventory semantics must be rich enough to support `v1b` family-level neg-risk logic without rewriting the entity model.
- `v1b` must reuse the same engine, state model, risk model, journal, and persistence layer.
- `v1b` must not block `v1a` from going live.

## 3. Release Plan

### 3.1 `v1a`

`v1a` is the first live release.

Scope:

- `non-sports` markets only
- binary markets with a valid YES/NO pair and CTF `split / merge / redeem` support only
- `full-set arbitrage` only
- single account
- single wallet
- single venue: `Polymarket`
- automatic `split / merge / redeem`
- live trading with real execution

Included capabilities:

- market metadata discovery
- `market` and `user` websocket ingestion
- REST reconciliation
- local orderbook and trading state
- full-set opportunity detection
- risk approval and execution planning
- batch two-leg order submission
- automatic `split / merge / redeem`
- event journal
- balances, orders, fills, positions, and PnL persistence
- market-level and global kill switches

Explicit non-goals:

- sports markets
- maker quoting
- liquidity reward optimization
- cross-market logical arbitrage
- multi-account coordination
- multi-venue support
- automatic bridging or treasury orchestration

### 3.2 `v1b`

`v1b` extends the same engine with `full-market negative-risk` support.

Added scope:

- automatic discovery of all tradable standard `neg-risk` market families
- structure validation before activation
- event-family level exposure management
- strategy evaluation across outcome graphs
- `negRisk`-aware execution and recovery

Initial live exclusions:

- augmented `neg-risk` families unless placeholder and `Other` semantics are explicitly modeled
- unnamed placeholder outcomes
- direct trading on `Other`

`v1b` is a new live release on top of the `v1a` production baseline, not a separate system.

## 4. System Boundary

The project goal is not to maximize the number of strategies. The project goal is to produce a Polymarket live engine whose state, risk, and recovery behavior are trustworthy enough for real trading.

The top-level boundary is:

- one production Rust live engine
- one Postgres database
- one append-only event journal
- optional Python research tooling outside the execution path

The research layer may support replay, analysis, and parameter studies, but it must not become a hidden production dependency.

## 5. Architecture

The production architecture is `single process + multiple async tasks`.

### 5.1 Core Runtime Tasks

1. `metadata task`
   - fetch events, markets, conditions, tokens, market routes, market status, tick sizes, fee flags, and `negRisk` metadata
   - refresh metadata on a schedule

2. `market feed task`
   - subscribe to `market` websocket
   - update orderbook, best bid/ask, tick size, and lifecycle information
   - maintain websocket `PING/PONG` liveness for the market channel
   - detect stale or inconsistent feed conditions

3. `user feed task`
   - subscribe to `user` websocket
   - process order updates and trade settlement lifecycle events
   - maintain websocket `PING/PONG` liveness for the user channel

4. `order heartbeat task`
   - maintain the REST order-heartbeat session used to preserve live open orders
   - track the latest `heartbeat_id`
   - on missed or invalid heartbeat, assume venue-side open orders may have been cancelled and trigger immediate reconciliation

5. `state reconciler`
   - periodically reconcile using REST snapshots, approval state, and relayer transaction status
   - recover after reconnects, timeouts, restarts, and `Unknown` states

6. `strategy dispatcher`
   - expose a stable `StateView` to strategies
   - register `full-set` in `v1a`
   - add `neg-risk` in `v1b`

7. `risk engine`
   - evaluate every opportunity before execution
   - approve, reject, downsize, or halt

8. `execution engine`
   - execute business-level execution plans
   - submit, cancel, batch, split, merge, redeem, retry, and recover

9. `journal writer`
   - persist external events and internal decisions as append-only records

10. `scheduler`
   - drive timers, expirations, reconciliation cadence, and periodic maintenance work

### 5.2 State Layers

- `Venue State`: raw venue-facing information from WS and REST
- `Trading State`: orders, balances, positions, reservations, inventory buckets, resolution state, market status
- `Strategy State`: stable read-only state exposed to strategies
- `Recovery State`: bootstrapping, reconciling, degraded, no-new-risk, halted

These layers must stay separate. The system must not treat a derived local assumption as venue truth.

## 6. Runtime Modes

The engine should expose explicit runtime modes:

- `Bootstrapping`
- `Healthy`
- `Reconciling`
- `Degraded`
- `NoNewRisk`
- `GlobalHalt`

Rules:

- new risk may be added only in `Healthy`
- any unresolved order or inventory uncertainty may force `NoNewRisk`
- unrecoverable inconsistency forces `GlobalHalt`

### 6.1 Action Matrix

| Mode | New risk | Cancel | Reduce-only hedge or repair | `split / merge / redeem` | Order heartbeat |
| --- | --- | --- | --- | --- | --- |
| `Bootstrapping` | No | Yes | No, unless explicit operator override is active | No automated inventory actions before first successful reconcile | Yes, once authenticated |
| `Healthy` | Yes | Yes | Yes | Yes | Yes |
| `Reconciling` | No | Yes | Yes | Inventory normalization or unwind only | Yes |
| `Degraded` | No | Yes | Yes, under explicit price guards | Protective or inventory-only actions | Yes |
| `NoNewRisk` | No | Yes | Yes | `merge` and `redeem` allowed; `split` only for approved repair or unwind | Yes unless `cancel-all` policy is selected |
| `GlobalHalt` | No | Yes, including `cancel-all` | No automated repair orders | No automated expansion; wind-down only via explicit break-glass policy | Continue until live orders are cancelled or confirmed absent, then stop |

Before the first fully successful reconciliation, `Bootstrapping` defaults to `CancelOnly`. Any repair, hedge, or inventory action during bootstrap requires explicit break-glass operator override.

### 6.2 Operational Policy Overlays

The engine may apply stricter overlays inside `Reconciling`, `Degraded`, or `NoNewRisk`:

- `ReduceOnly`
- `InventoryOnly`
- `CancelOnly`

These overlays are action filters, not separate lifecycle modes.

### 6.3 Venue And Account Trading Status Mapping

Venue and account restrictions must map deterministically into local runtime behavior.

Minimum mappings:

- venue `trading disabled` -> `GlobalHalt`
- venue `cancel-only` -> `NoNewRisk` plus `CancelOnly`
- address `close-only` -> `NoNewRisk` plus `ReduceOnly`
- geoblocked or banned address -> `GlobalHalt` plus operator alert
- any newly observed venue or account restriction state must be journaled and surfaced to observability immediately

## 7. Domain Model

The minimum domain entities are:

- `Event`
- `EventFamily`
- `Condition`
- `Market`
- `Token`
- `MarketRoute`
- `IdentifierMap`
- `Order`
- `Fill`
- `Balance`
- `ApprovalState`
- `ResolutionState`
- `Position`
- `RelayerTransaction`
- `CTFOperation`
- `Opportunity`
- `JournalEvent`

### 7.1 Identifier Model

The identifier model must be explicit because Polymarket uses different identifiers across APIs and execution paths.

- `Event`: top-level event metadata
- `EventFamily`: a group of related markets used for `neg-risk` discovery and risk aggregation
- `Condition`: the CTF condition and the unit used for user-channel subscriptions
- `Token`: the venue token ID / asset ID / CTF position ID used for market-channel subscriptions and order placement
- `Market`: logical Polymarket market metadata mapped to one condition and one or more tokens
- `MarketRoute`: `standard` or `negRisk`
- `IdentifierMap`: authoritative mapping between `event_id`, `event_family_id`, `market_id`, `condition_id`, token IDs, outcome labels, and route

Rules:

- market data subscriptions are keyed by token ID
- user-channel subscriptions are keyed by condition ID
- replay, reconciliation, and exposure accounting must bridge both through `IdentifierMap`
- `v1a` may operate mostly on one condition with two tokens, but the model must not assume that this is enough for all future routes

### 7.2 Inventory Accounting Buckets

The inventory model must distinguish at least:

- `free`
- `reserved_for_order`
- `matched_unsettled`
- `pending_ctf_in`
- `pending_ctf_out`
- `redeemable`
- `quarantined`

### 7.3 Approval And Allowance State

Approval and allowance are part of live trading state, not one-time setup trivia.

`ApprovalState` must track at least:

- token
- spender
- owner_address
- funder_address
- wallet_route
- signature_type
- allowance
- last_checked_at
- required_min_allowance
- approval_status

The engine must reconcile approval state during bootstrapping and before any execution path that depends on token spending.

### 7.4 Resolution State

Resolution and redemption must depend on authoritative condition-level settlement state, not only market labels.

`ResolutionState` must track at least:

- `condition_id`
- `resolution_status`
- `payout_vector`
- `resolved_at`
- `dispute_state`
- `redeemable_at`

`ResolutionState` is the authoritative source for:

- whether redemption is allowed
- how much value each outcome token redeems for
- when post-resolution inventory can be normalized into realized settlement

### 7.5 Separation Rules

The design must keep the following concepts separate:

- `Order State` vs `Inventory State`
- `Venue Truth` vs `Local Derived State`
- `Execution Intent` vs `Execution Outcome`
- `Detected Opportunity` vs `Approved Opportunity`

These separations are required for correct recovery and postmortem analysis.

## 8. Order, Trade, And Inventory State Machines

### 8.1 Submission State

Submission state tracks the local order-intent lifecycle:

`Draft -> Planned -> RiskApproved -> Signed -> Submitted -> Acked`

Terminal or degraded submission outcomes:

- `Rejected`
- `Unknown`

### 8.2 Venue Order State

Venue order state tracks how the venue classified the order after submission:

- `Live`
- `Matched`
- `Delayed`
- `Unmatched`
- `CancelPending`
- `Cancelled`
- `Expired`
- `Unknown`

### 8.3 Trade Settlement State

Trade settlement state tracks executor and onchain progress independently from order placement:

- `MATCHED`
- `MINED`
- `CONFIRMED`
- `RETRYING`
- `FAILED`
- `UNKNOWN`

`Fill` records and trade-settlement logs must carry settlement state separately from order state.

### 8.4 `Unknown`

`Unknown` is a first-class state.

Typical causes:

- request timeout after submission
- lost user-feed acknowledgement
- restart or reconnect gap
- exchange maintenance window
- cancel result not yet trustworthy

System rule:

- if any materially relevant order enters `Unknown`, the engine must stop adding related new risk and trigger reconciliation

### 8.5 CTF Operation State Machine

`split / merge / redeem` must have their own lifecycle:

- `Requested`
- `Submitted`
- `Confirmed`
- `Failed`
- `Unknown`

The system must not assume inventory changed just because a request was issued.

## 9. Strategy Interface

Strategies must be pure decision components over a stable state view.

They may:

- read `StateView`
- consume market/user/timer events
- emit `Opportunity`

They may not:

- place or cancel orders directly
- mutate balances or positions directly
- bypass the risk engine

## 10. `v1a` Strategy Specification: Full-Set

### 10.1 Inputs

- best bid/ask for YES and NO
- tick size
- fee parameters
- available USDC and shares
- lifecycle state
- current open orders
- runtime mode

### 10.2 Outputs

- `Opportunity::FullSetBuyMerge`
- `Opportunity::FullSetSplitSell`

### 10.3 Precision, Units, And Quantization

The pricing model must preserve venue-native units while normalizing comparisons to USDC-equivalent value.

Rules:

- `USDC.e` uses 6 decimals
- fee calculations round to 4 decimal places, with a minimum charged fee of `0.0001 USDC`
- fee-free markets must be treated as `feeRateBps = 0`
- fee-enabled markets must fetch `feeRateBps` dynamically from the venue or SDK path; the value must never be hardcoded
- buy-side taker fees are collected in shares and must be converted into USDC-equivalent cost for edge calculation
- sell-side taker fees are collected in USDC and must be accounted for directly
- marketable `BUY` orders specify USDC amount, while marketable `SELL` orders specify shares
- `min_order_size` must be fetched and enforced before submission
- `price_quantum` and `size_quantum` must be modeled explicitly
- strategy outputs economic intent; execution converts it into venue-legal `price` and `size`
- all rounding and quantization must complete before submission; the venue must not be used as the first validator
- `sdk_market_order_semantics` and raw signed-order semantics must be documented separately so retry and replay logic are not coupled to SDK-only abstractions

### 10.4 Net Edge

The engine must compute and persist normalized edge in USDC terms.

Buy both sides then merge:

`net_edge_usdc = gross_out_usdc - gross_in_usdc - fee_usdc_equiv - rounding_loss - ctf_cost - latency_buffer - hedge_buffer`

Split then sell both sides:

`net_edge_usdc = gross_in_usdc - gross_out_usdc - fee_usdc_equiv - rounding_loss - ctf_cost - latency_buffer - hedge_buffer`

Rules:

- `gross_in_usdc` and `gross_out_usdc` must be persisted per leg and in normalized total form
- an opportunity may proceed only if `net_edge_usdc > min_edge_threshold_usdc`
- `min_edge_threshold_usdc` must include recovery and execution uncertainty buffer
- all prices must be rounded and validated against tick size rules

### 10.5 `v1a` Strategy Constraints

- `non-sports` only
- only markets with a valid paired YES/NO structure on the same condition
- no alpha prediction
- no cross-market packaging
- no expansion of risk when relevant state is uncertain

## 11. `v1b` Strategy Specification: Full-Market Negative Risk

`v1b` adds a second strategy layer on the same runtime foundation.

### 11.1 Additional Complexity

Compared with full-set, `neg-risk` adds:

- graph-based market discovery
- outcome-family structure validation
- portfolio-level inventory modeling
- more complex partial-fill and broken-leg recovery

### 11.2 `v1b` Requirements

- scan all identifiable `neg-risk` families
- validate family structure before trading
- compute path-aware opportunity value
- track family-level exposure
- expose event-family kill switches

### 11.3 Activation Rule

An event family is not tradable merely because it is labeled `negRisk`.

It must also pass:

- metadata completeness checks
- conversion-path validation
- liquidity and execution sanity checks
- recovery-model support checks
- placeholder and `Other` exclusion rules for initial live scope

## 12. Risk Model

The risk engine is an authority layer, not a passive validator.

It may:

- approve
- reject
- reduce size
- freeze a market
- freeze a strategy
- halt the engine

### 12.1 Required `v1a` Checks

- identifier and market-route consistency
- net edge after fees and slippage
- available inventory and reservation sufficiency
- approval and allowance sufficiency for every spender touched by the execution path
- market status and lifecycle validity
- resolution, payout-vector, and dispute-state validity for `redeem`
- feed freshness
- websocket and order-heartbeat freshness
- unresolved `Unknown` state
- unresolved `CTFOperation`
- unresolved or stale `RelayerTransaction`
- per-market exposure limit
- per-condition exposure limit
- global capital usage limit
- venue health and maintenance conditions, including HTTP `425` restart handling

### 12.2 Risk Principle

When state is uncertain, the engine prioritizes correctness over capture rate.

The default response to uncertainty is to tighten risk, not to continue trading.

## 13. Execution Semantics

Execution is driven by business-level plans, not raw API calls.

### 13.1 Signer, Wallet, And Relayer Model

Execution must explicitly model:

- signer identity
- signature type
- wallet route: `EOA`, `POLY_PROXY`, or `GNOSIS_SAFE`
- funder address
- proxy or safe deployment state
- relayer transaction ID
- relayer nonce
- relayer transaction state
- onchain transaction hash, when available

`split / merge / redeem` are not complete when requested. They are complete only after relayer confirmation or authoritative state reconciliation proves the effect.

### 13.2 Signed-Order Idempotency And Retry Semantics

Retry must distinguish transport-level replay from business-level re-issuance.

Rules:

- on `Submitted but not confirmed`, the engine must query the status of the original signed order before generating a new payload
- `transport retry` resends the same signed payload and preserves `signed_order_hash`, `salt`, `nonce`, and `signature`
- `business retry` generates a new signed payload and therefore requires a new `salt`, `nonce`, and `signature`
- the engine must never treat a new signed payload as equivalent to replaying an existing order
- every business retry must link back to the original via `retry_of_order_id`

### 13.3 Execution-Plan Categories

Required execution-plan categories:

- `FullSetBuyThenMerge`
- `FullSetSplitThenSell`
- `CancelStale`
- `RedeemResolved`

`RedeemResolved` is by `condition_id`, not by arbitrary amount. The redeemable amount is derived from current balances and payout state. It is allowed only when the condition is resolved and not in dispute, challenge, or uncertainty review.

Execution plans are the unit of:

- retry
- idempotency
- recovery
- replay
- postmortem analysis

### 13.4 `v1a` Execution Rules

- two-leg trades should share a `batch_group`
- batch submission is a synchronization aid, not an atomicity guarantee
- thin opportunities should prefer `FOK`
- `FAK` is allowed only where partial-fill handling is explicitly supported
- `maker` logic is excluded from `v1a`
- price legality must be checked before submission
- min-size and quantization legality must be checked before submission

### 13.5 Failure Semantics

Execution failure must distinguish at least:

- not submitted
- submitted but not confirmed
- partially filled
- one leg filled and one leg failed
- trade completed but `split/merge/redeem` failed
- chain or relayer state unknown

### 13.6 Deterministic Recovery Playbooks

The engine must define deterministic actions for the highest-risk failure classes.

- `submitted but not confirmed`
  - mark the order `Unknown`
  - keep heartbeats running
  - reconcile open orders, fills, and remote balances
  - freeze the related market or family to `NoNewRisk` until convergence

- `one leg filled, other leg failed or unconfirmed`
  - freeze the related market or family
  - allow only bounded repair orders under configured worst-case price guards and retry budget
  - if repair fails, move residual inventory to `quarantined` and restrict the system to reduce-only handling for that exposure

- `split submitted, relayer or chain state unknown`
  - query relayer transaction status and recent transactions by owner, proxy, and nonce
  - reconcile balances and inventory buckets
  - keep inventory in `pending_ctf_out` until the effect is proven or rejected
  - block dependent new risk while unresolved

- `merge failed after successful trade legs`
  - do not recognize realized PnL from the intended merge
  - move inventory to `matched_unsettled` or `quarantined`
  - allow only repair or unwind actions until neutralized

- `redeem failed or unresolved`
  - keep inventory in `redeemable`
  - retry via inventory-only worker or manual break-glass procedure

- `batch mixed results`
  - treat each order response independently
  - do not infer symmetry between both legs merely because they shared a batch

### 13.7 Venue Maintenance And Restart Handling

The engine must have explicit behavior for venue restart windows and HTTP `425`.

Rules:

- any order-related HTTP `425` response forces `Reconciling` or `NoNewRisk`
- new order placement pauses immediately
- market and user feeds remain connected when possible
- websocket and order-heartbeat liveness continue unless `cancel-all` policy is selected
- after the venue resumes, the engine performs forced reconciliation of open orders, fills, balances, and relayer transactions
- all orders submitted near the restart window and not fully explained by reconciliation go through uncertainty review

### 13.8 HTTP, Venue Error, And Retry Policy Matrix

HTTP and business-error handling must distinguish retryable transport failures from terminal business rejections.

| Error class | Default action | Retry class | Local mode effect |
| --- | --- | --- | --- |
| network timeout / connection reset | query remote status first, then retry if still uncertain | transport retry with bounded exponential backoff and jitter | may force `Reconciling` |
| HTTP `425` | pause new orders and reconcile after venue recovery | transport retry with exponential backoff | `Reconciling` or `NoNewRisk` |
| HTTP `429` | back off and retry | transport retry with exponential backoff and jitter | stays in current mode unless persistent, then `Degraded` |
| HTTP `500` | back off and retry | transport retry with bounded exponential backoff and jitter | may force `Degraded` or `Reconciling` |
| HTTP `503` trading disabled | do not retry blindly | no automatic retry until venue status changes | `GlobalHalt` |
| HTTP `503` cancel-only | do not place new orders; allow cancels | no automatic new-order retry while active | `NoNewRisk` plus `CancelOnly` |
| duplicated order / signed order already seen | reconcile original signed order first | no blind transport retry; business retry only if the original is definitively absent | may force `Reconciling` |
| min-size / tick-size / malformed payload | reject as terminal business error | no retry | no mode change |
| insufficient balance / allowance | reject and reconcile state | no retry until balance or allowance changes | may force `NoNewRisk` |
| `FOK` not filled | treat as expected terminal execution result | no retry unless strategy emits a new opportunity | no mode change |
| delayed / unmatched accepted responses | treat as live venue outcomes, not transport failures | no automatic transport retry | no mode change |

Rules:

- retry budgets must be explicit per endpoint and operation class
- every retried request must journal its retry class and budget consumption
- transport retry must never silently become business retry
- persistent `429` or `500` conditions must eventually degrade local mode rather than loop indefinitely

## 14. Persistence Model

Postgres is a required system component.

Required tables:

- `events`
- `event_families`
- `conditions`
- `markets`
- `tokens`
- `identifier_map`
- `balances`
- `approval_states`
- `resolution_states`
- `inventory_buckets`
- `orders`
- `fills`
- `positions`
- `relayer_transactions`
- `ctf_operations`
- `opportunities`
- `event_journal`

### 14.1 Persistence Principles

- identifier persistence must bridge token-ID subscriptions, condition-ID subscriptions, and family-level route metadata
- `event_journal` is append-only
- business tables hold current structured views
- recovery must not rely on memory-only state
- idempotent keys are required for orders, fills, and chain-affecting operations

### 14.2 Order, Approval, Inventory, And Relayer Persistence

The `orders` persistence model must include the signed-order fields needed to distinguish transport retry from business retry:

- `signed_order_hash`
- `salt`
- `nonce`
- `signature`
- `retry_of_order_id`

The persisted model must be able to answer:

- what signed payload was actually submitted
- whether a retry reused the same payload or created a new one
- what allowance or approval shortfall blocks execution
- which owner, funder, wallet route, and signature type an approval belongs to
- what payout vector and dispute state govern a resolved condition
- what free inventory exists
- what is reserved by open orders
- what is matched but not yet settled
- what is waiting on relayer or chain effects
- what is redeemable
- what has been quarantined

`ctf_operations` and `relayer_transactions` must be linkable by transaction ID, nonce, signer, and proxy or safe address.

### 14.3 `event_journal`

The journal is mandatory for:

- replay
- debugging
- state reconstruction
- strategy re-evaluation
- postmortem analysis

At minimum each entry must include:

- `journal_seq`
- `stream`
- `source_kind`
- `source_session_id`
- `source_event_id`
- `dedupe_key`
- `causal_parent_id`
- `event_type`
- `event_ts`
- `payload`
- ingestion timestamp

`journal_seq` is the authoritative local replay order for deterministic reconstruction.

## 15. Recovery Model

Startup recovery flow:

1. load identifier maps, signer configuration, approval state, and pending inventory state
2. connect feeds and start websocket liveness
3. start order heartbeat once credentials are available
4. fetch remote open orders, balances, approvals, resolution state, and recent relayer transactions
5. reconcile local and remote state
6. remain `CancelOnly` during bootstrap until the first successful reconcile completes
7. enter `Healthy` only after convergence

If convergence fails, the system must remain in `NoNewRisk` or escalate to `GlobalHalt`.

## 16. Non-Functional Requirements

- correctness over throughput
- replayability
- recoverability
- observability
- idempotency
- auditability
- safe defaults

Minimum observability should include:

- feed health
- websocket reconnect count
- websocket heartbeat freshness
- order-heartbeat freshness
- runtime mode
- reconcile status
- venue-vs-local divergence count
- count of `Unknown` orders
- strategy opportunities
- execution outcomes
- broken-leg inventory count
- relayer transaction pending age
- stuck-state duration per market or family
- balances and exposure
- realized and unrealized PnL

## 17. Launch Gates

### 17.1 `v1a`

The system may go live only after:

- market and user feeds run stably
- websocket `PING/PONG` and REST order-heartbeat behavior are both verified
- event journal is complete enough for replay
- orders, balances, and positions reconcile successfully
- approvals and allowances reconcile successfully for all required spenders
- resolution-state and payout-vector persistence are validated against at least one resolved condition path
- `Unknown` paths force risk tightening
- paper-mode full-set runs have been evaluated on live data
- minimal live `split / merge / redeem` validation succeeds
- signed-order retry logic is validated for transport retry vs business retry
- relayer wallet deployment path and nonce tracking are validated
- broken-leg repair and inventory quarantine flows are exercised
- kill switches are tested
- restart recovery is tested
- break-glass manual procedures are documented and dry-run
- capital ramp policy is defined: begin with a very small market set and notional, then increase only after multiple clean live sessions without unreconciled state

### 17.2 `v1b`

`v1b` may go live only after:

- all tradable `neg-risk` families can be discovered
- structure validation is implemented and enforced
- augmented `neg-risk`, placeholder, and `Other` outcomes remain excluded unless explicitly modeled and validated
- family-level exposure can be reconstructed
- identifier, route, and family mappings can be replayed from persistence
- replay can explain `neg-risk` decisions and failures
- event-family kill switches are tested

## 18. Delivery Sequence

1. `foundation`
   - metadata
   - market feed
   - user feed
   - heartbeat
   - journal

2. `state and recovery`
   - balances
   - open orders
   - positions
   - reconcile
   - `Unknown` handling
   - runtime modes

3. `paper full-set`
   - fee model
   - opportunity detection
   - replay
   - paper execution accounting

4. `live full-set`
   - batch execution
   - `FOK/FAK` policy
   - `split / merge / redeem`
   - live risk gates
   - PnL accounting

5. `neg-risk foundation`
   - graph builder
   - family validator
   - family exposure model

6. `live neg-risk`
   - execution
   - recovery
   - family-level controls

## 19. Open Validation Items

These items are not optional details. They are launch-critical validations:

- verify the exact live signing and credential flow for the chosen wallet setup
- verify the production path for `split / merge / redeem` in the chosen Rust-first architecture
- verify approval and allowance reconciliation across all required spenders
- verify family discovery completeness for `neg-risk` routing
- verify recovery behavior when a two-leg batch produces mixed results
- verify transport retry versus business retry behavior for signed orders
- verify how maintenance, throttling, reconnect gaps, and relayer delays affect order certainty

## 20. Final Decision

The approved direction is:

- product name: `AxiomArb`
- architecture: single-process Rust live engine
- persistence: Postgres plus append-only journal
- first live target: `v1a full-set`
- second live target: `v1b full-market neg-risk`

This design deliberately optimizes for correctness, recovery, and staged live rollout over breadth.
