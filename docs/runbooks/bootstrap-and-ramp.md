# Bootstrap And Ramp

This runbook covers the startup sequence, why the engine stays `CancelOnly` until the first successful reconcile, and how to ramp live size after paper validation.

## Bootstrap Sequence

1. Start in paper mode.
2. Load config, connect to Postgres, and start logs, metrics, feeds, and order heartbeat.
3. Reconcile local state against venue state.
4. Stay in `Bootstrapping` with `CancelOnly` until the first reconcile succeeds.
5. Promote to `Healthy` only after the reconcile is clean.

Paper mode should exercise the same bootstrap, journal, replay, and reconcile path as live, but without real-money exposure.

If the first reconcile fails or any materially relevant order becomes `Unknown`, keep the engine in `NoNewRisk` or `GlobalHalt` and resolve the divergence before resuming.

## Why `CancelOnly` Until First Reconcile

- At startup, the local store cannot yet prove that open orders, approvals, inventory, and relayer state match venue truth.
- `CancelOnly` prevents new exposure before the engine has a trustworthy baseline.
- Any repair before the first successful reconcile must be an explicit break-glass action, because it can change inventory or exposure while the baseline is still uncertain.

## Launch Gates

Do not progress beyond paper or a minimal live pilot until all of the following have been exercised:

- clean bootstrap and reconcile after restart
- journal replay over the same session without divergence
- websocket `PING/PONG` and REST order-heartbeat behavior
- order, balance, and position reconciliation
- approval and allowance reconciliation for required spenders
- at least one resolved-condition path for `split / merge / redeem`
- broken-leg repair and inventory quarantine flows
- documented manual procedures, including break-glass, reviewed and dry-run
- kill switches tested
- restart recovery tested
- relayer pending transaction and nonce handling verified
- paper-mode full-set runs evaluated on live data

## Suggested Ramp Ladder

Use a very small initial notional and expand only after multiple clean sessions with no unreconciled state.

- Stage 0: paper only, one market, full-set signals only, no live orders.
- Stage 1: one market, minimum venue-accepted notional, one live session, with the first order cycle fully explainable end to end.
- Stage 2: the same one-market set, still at minimum-sized risk, repeated across multiple clean live sessions.
- Stage 3: the same market set at about `2x` the initial notional, only after replay, quarantine, and relayer checks stay clean.
- Stage 4: narrow live set of `2-3` markets, still small, and only after the earlier stages show stable bootstrap and recovery.

## What Counts As A Clean Session

- bootstrap reaches `Healthy` after a successful reconcile
- no unresolved reconcile attention remains
- no `Unknown` live order state remains
- no relayer transaction is stuck without a documented owner
- no quarantined inventory remains unexplained
- replay of the journal matches the live outcome
