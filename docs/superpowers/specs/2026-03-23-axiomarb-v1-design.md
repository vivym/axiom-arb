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

- `v1a` must be stable enough to trade live without depending on manual inventory operations.
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

- automatic discovery of all tradable `neg-risk` market families
- structure validation before activation
- event-family level exposure management
- strategy evaluation across outcome graphs
- `negRisk`-aware execution and recovery

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
   - fetch markets, assets, conditions, market status, tick sizes, and `negRisk` metadata
   - refresh metadata on a schedule

2. `market feed task`
   - subscribe to `market` websocket
   - update orderbook, best bid/ask, tick size, and lifecycle information
   - detect stale or inconsistent feed conditions

3. `user feed task`
   - subscribe to `user` websocket
   - process order updates, acknowledgements, and fills
   - maintain heartbeat and connection health

4. `state reconciler`
   - periodically reconcile using REST snapshots
   - recover after reconnects, timeouts, restarts, and `Unknown` states

5. `strategy dispatcher`
   - expose a stable `StateView` to strategies
   - register `full-set` in `v1a`
   - add `neg-risk` in `v1b`

6. `risk engine`
   - evaluate every opportunity before execution
   - approve, reject, downsize, or halt

7. `execution engine`
   - execute business-level execution plans
   - submit, cancel, batch, split, merge, redeem, retry, and recover

8. `journal writer`
   - persist external events and internal decisions as append-only records

9. `scheduler`
   - drive timers, expirations, reconciliation cadence, and periodic maintenance work

### 5.2 State Layers

- `Venue State`: raw venue-facing information from WS and REST
- `Trading State`: orders, balances, positions, reservations, market status
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

## 7. Domain Model

The minimum domain entities are:

- `Market`
- `Asset`
- `Order`
- `Fill`
- `Balance`
- `Position`
- `CTFOperation`
- `Opportunity`
- `JournalEvent`

### 7.1 Separation Rules

The design must keep the following concepts separate:

- `Order State` vs `Inventory State`
- `Venue Truth` vs `Local Derived State`
- `Execution Intent` vs `Execution Outcome`
- `Detected Opportunity` vs `Approved Opportunity`

These separations are required for correct recovery and postmortem analysis.

## 8. Order And Inventory State Machines

### 8.1 Order State Machine

Required order lifecycle:

`Draft -> Planned -> RiskApproved -> Signed -> Submitted -> Acked`

From `Acked`, the order may enter:

- `Live`
- `Rejected`
- `Unknown`

From `Live`, the order may enter:

- `PartiallyFilled`
- `Filled`
- `CancelPending`
- `Cancelled`
- `Expired`

### 8.2 `Unknown`

`Unknown` is a first-class state.

Typical causes:

- request timeout after submission
- lost user-feed acknowledgement
- restart or reconnect gap
- exchange maintenance window
- cancel result not yet trustworthy

System rule:

- if any materially relevant order enters `Unknown`, the engine must stop adding related new risk and trigger reconciliation

### 8.3 CTF Operation State Machine

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

### 10.3 Net Edge

Buy both sides then merge:

`net_edge = 1 - ask_yes - ask_no - fee_yes - fee_no - expected_slippage - ops_buffer`

Split then sell both sides:

`net_edge = bid_yes + bid_no - 1 - fee_yes - fee_no - expected_slippage - ops_buffer`

Rules:

- an opportunity may proceed only if `net_edge > min_edge_threshold`
- `min_edge_threshold` must include recovery and execution uncertainty buffer
- all prices must be rounded and validated against tick size rules

### 10.4 `v1a` Strategy Constraints

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

- net edge after fees and slippage
- available inventory and reservation sufficiency
- market status and lifecycle validity
- feed freshness
- unresolved `Unknown` state
- unresolved `CTFOperation`
- per-market exposure limit
- per-condition exposure limit
- global capital usage limit
- venue health and maintenance conditions

### 12.2 Risk Principle

When state is uncertain, the engine prioritizes correctness over capture rate.

The default response to uncertainty is to tighten risk, not to continue trading.

## 13. Execution Semantics

Execution is driven by business-level plans, not raw API calls.

Required execution-plan categories:

- `FullSetBuyThenMerge`
- `FullSetSplitThenSell`
- `CancelStale`
- `RedeemResolved`

Execution plans are the unit of:

- retry
- idempotency
- recovery
- replay
- postmortem analysis

### 13.1 `v1a` Execution Rules

- two-leg trades should share a `batch_group`
- batch submission is a synchronization aid, not an atomicity guarantee
- thin opportunities should prefer `FOK`
- `FAK` is allowed only where partial-fill handling is explicitly supported
- `maker` logic is excluded from `v1a`
- price legality must be checked before submission

### 13.2 Failure Semantics

Execution failure must distinguish at least:

- not submitted
- submitted but not confirmed
- partially filled
- one leg filled and one leg failed
- trade completed but `split/merge/redeem` failed
- chain or relayer state unknown

## 14. Persistence Model

Postgres is a required system component.

Required tables:

- `markets`
- `assets`
- `balances`
- `orders`
- `fills`
- `positions`
- `ctf_operations`
- `opportunities`
- `event_journal`

### 14.1 Persistence Principles

- `event_journal` is append-only
- business tables hold current structured views
- recovery must not rely on memory-only state
- idempotent keys are required for orders, fills, and chain-affecting operations

### 14.2 `event_journal`

The journal is mandatory for:

- replay
- debugging
- state reconstruction
- strategy re-evaluation
- postmortem analysis

At minimum each entry must include:

- `stream`
- `event_type`
- `event_ts`
- `payload`
- ingestion timestamp

## 15. Recovery Model

Startup recovery flow:

1. load required metadata
2. connect feeds
3. fetch remote open orders and balances
4. reconcile local and remote state
5. enter `Healthy` only after convergence

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
- runtime mode
- reconcile status
- count of `Unknown` orders
- strategy opportunities
- execution outcomes
- balances and exposure
- realized and unrealized PnL

## 17. Launch Gates

### 17.1 `v1a`

The system may go live only after:

- market and user feeds run stably
- user heartbeat behavior is verified
- event journal is complete enough for replay
- orders, balances, and positions reconcile successfully
- `Unknown` paths force risk tightening
- paper-mode full-set runs have been evaluated on live data
- minimal live `split / merge / redeem` validation succeeds
- kill switches are tested
- restart recovery is tested

### 17.2 `v1b`

`v1b` may go live only after:

- all tradable `neg-risk` families can be discovered
- structure validation is implemented and enforced
- family-level exposure can be reconstructed
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
- verify recovery behavior when a two-leg batch produces mixed results
- verify how maintenance, throttling, and reconnect gaps affect order certainty

## 20. Final Decision

The approved direction is:

- product name: `AxiomArb`
- architecture: single-process Rust live engine
- persistence: Postgres plus append-only journal
- first live target: `v1a full-set`
- second live target: `v1b full-market neg-risk`

This design deliberately optimizes for correctness, recovery, and staged live rollout over breadth.
