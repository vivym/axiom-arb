# Bootstrap And Ramp

This runbook covers the bootstrap policy and launch gates for the `v1a` live candidate.

At current `HEAD`, `app-live` is launched from the unified TOML passed with `--config` and uses `DATABASE_URL` for persistence. The preferred Day 0 operator flow is `bootstrap`. When you need lower-level control, the fallback is `init`, `discover`, `targets candidates`, `targets adopt`, `doctor`, then `run`. Day 1+ should start from `status`; for `real-user shadow smoke`, the high-level continuation is `apply`.
The wizard flow no longer assumes fresh databases already have discoverable adoption artifacts. Missing discovery artifacts, non-adoptable discovery output, missing startup anchors, or rollout readiness gaps are surfaced truthfully as next steps instead of being prefilled in the config.

`DATABASE_URL` remains the only deployment env var. Operator business config now lives in a TOML file such as `config/axiom-arb.example.toml`.

## Local Commands

Create or refresh a local operator config, then validate and run it:

```bash
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
cargo run -p app-live -- init --config config/axiom-arb.local.toml
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

Run replay against the same config file:

```bash
cargo run -p app-replay -- --config config/axiom-arb.local.toml --from-seq 0 --limit 1000
```

The sequence below is the operational posture to preserve during launch and ramp. Use it as the runbook for bringing the daemon up safely, validating reconcile truth first, and deciding when a session is clean enough to widen risk. For the adopted-target startup path, the operator should not hand-author raw `negrisk.targets` members; `doctor` and `run` should resolve the adopted target revision from the config model.
When no rollout is present yet, keep the rollout lists empty in the generated config and let the Day 0 flow surface the next step truthfully. `discover` persists advisory and adoptable artifacts only. `targets adopt` writes `operator_target_revision`, canonical adoption provenance, and adoption history; it does not fill `approved_families` or `ready_families`.

Use the target control-plane commands instead of editing `[negrisk.target_source].operator_target_revision` by hand:

```bash
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

If the adopted revision needs to be reverted, use:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
```

or an explicit rollback target:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml --to-operator-target-revision <operator-target-revision>
```

Both `targets adopt` and `targets rollback` rewrite the configured `operator_target_revision` in the TOML and append adoption history, but they remain startup-scoped. They do not hot-reload a running daemon, so a controlled restart may still be required before the new revision becomes active.

## Target Bootstrap Sequence

1. Start in paper mode first.
2. Load config, connect persistence, and start logs and metrics.
3. Connect market and user feeds, then start order heartbeat once credentials are available.
4. Reconcile local state against venue state, approvals, and relayer state.
5. Stay in `Bootstrapping` with `CancelOnly` until the first reconcile succeeds.
6. Promote to `Healthy` only after the reconcile is clean.

Target state: paper mode should exercise the same bootstrap, journal, replay, and reconcile path as live, but without real-money exposure.

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
