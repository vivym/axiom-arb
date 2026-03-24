# Live Break-Glass Runbook

Use this when a live incident needs immediate containment: stale venue state, duplicate or stuck orders, relayer problems, broken-leg inventory, or any situation where the engine must stop taking new risk.

## Before You Start

- If the engine has not completed its first successful reconcile, it is still in bootstrap `CancelOnly`.
- Before that first successful reconcile, any repair, hedge, or inventory action requires an explicit break-glass override.
- Do not clear the halt until you can explain every open order, pending relayer transaction, and quarantined inventory bucket.

## 1. Enter `GlobalHalt`

- Force the live process into `GlobalHalt` through the operator path available in the deployment.
- Stop any strategy dispatch, order creation, or automated repair work.
- Record the incident time, operator, and reason for the halt.

## 2. Run `cancel-all`

- Issue a venue-wide cancel-all for the affected account.
- Retry until every live order is either cancelled or positively confirmed absent.
- If the venue responds partially, treat the account as still exposed and stay halted.
- Do not submit any new order while the cancel sweep is in progress.

## 3. Confirm Open Orders Are Absent

- Check the venue open-order view.
- Check local journal and persistence records for remaining live orders.
- Confirm the live order count is zero, or that any remaining rows are terminal and explained.
- If any order is unknown, return to reconciliation and do not resume trading.

## 4. Inspect Relayer and Stuck CTF Work

- List pending relayer transactions and sort them by age.
- Inspect any stuck `split`, `merge`, or `redeem` operation.
- Capture tx IDs, nonces, status, and the last relayer or chain response.
- Keep the engine halted until the relayer queue is either cleared or explicitly quarantined.

## 5. Handle Quarantined Inventory

- Move broken-leg, unmatched, or otherwise suspect inventory into quarantine.
- Separate inventory that is safe to resume from inventory that needs manual review.
- Do not automatically split, merge, or redeem quarantined inventory unless the break-glass override explicitly allows it.
- For each quarantined bucket, decide whether to repair, unwind, hold, or write off.

## 6. Capture Evidence

- Save the last successful and failed reconcile reports.
- Save the last journal sequence number and the replay range needed to reconstruct the incident.
- Export runtime mode, overlay, feed health, heartbeat freshness, pending relayer age, and inventory bucket counts.
- Attach logs that show the transition into `GlobalHalt` and the result of `cancel-all`.

## 7. Conditions For Leaving Halt

- No live open orders remain, or every remaining order is confirmed absent.
- No pending cancel request or relayer transaction can still create exposure.
- Quarantined inventory is either resolved, explicitly accepted, or left isolated with a new risk sign-off.
- Feeds, heartbeat, and reconcile status are healthy again.
- A human operator has reviewed the evidence and approved the restart.

## 8. Resume Path

- Remove the break-glass override only after the conditions above are true.
- Re-enter paper or `Healthy` operation from a clean reconcile.
- Treat the first post-halt session as a new ramp checkpoint, not as a continuation of the previous risk budget.
