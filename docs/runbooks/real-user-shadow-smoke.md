# Real-User Shadow Smoke Runbook

Use this only for a manual operator smoke. The smoke path is shadow-only for `neg-risk` and does not represent production readiness or live-submit readiness.

## Preflight

Set the same database that `app-live` and `app-replay` will read. `DATABASE_URL` remains the only deployment env var.

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

Prepare a smoke config at `config/axiom-arb.local.toml` with the preferred high-level flow:

```bash
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
```

For smoke, `bootstrap` reuses the init wizard, keeps startup authority on `[negrisk.target_source].operator_target_revision`, prompts for an explicit adoptable revision when no target anchor exists yet, and then asks whether to stay in `preflight-ready smoke startup` or explicitly enable `shadow-work-ready smoke startup` for the adopted family set. If you prefer the lower-level path, you can still run `app-live init`, `targets candidates`, and `targets adopt` manually, or copy `config/axiom-arb.example.toml` and review the long-lived operator settings. The smoke config must keep rollout lists empty until the operator explicitly confirms the smoke-only rollout posture:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = true
```

Do not use paper mode. Do not hand-author transient auth values or raw `negrisk.targets` members for the normal adopted-target startup path; let the config model and startup flow resolve the adopted target revision. Empty rollout lists are the safe default until an adoptable revision is explicitly chosen, and adopting a revision alone does not create the `neg-risk` shadow-ready rollout state.

If bootstrap stops in `preflight-ready smoke startup`, the config is valid and `doctor` can still pass, but a later run is expected to produce zero `neg-risk` shadow rows. If bootstrap reaches `shadow-work-ready smoke startup`, it has already written the adopted family ids into both rollout lists and `bootstrap --start` may continue into `run`.

Before running the smoke with the lower-level path, inspect the current control-plane state and adopt the startup-scoped target revision you intend to test:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

`status` should now tell you whether the smoke is:

- `target-adoption-required`
- `restart-required`
- `smoke-rollout-required`
- `smoke-config-ready`
- `blocked`

Use `targets status` and `targets show-current` only when you need the lower-level control-plane provenance behind that summary.

## Run

With the preferred high-level flow, stop after `bootstrap` or continue straight into runtime:

```bash
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml --start
```

If you are using the lower-level path, preflight the smoke config, then start `app-live` with the smoke config:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

Interpret `status` before `doctor` like this:

- `smoke-rollout-required`
  - config and adopted target are present, but rollout is still intentionally empty
  - you are still in preflight-only smoke; follow the printed next action before expecting shadow work
- `smoke-config-ready`
  - config, adopted target, and smoke rollout are aligned
  - continue to `doctor`
- `restart-required`
  - the configured revision is not the active revision yet
  - perform a controlled restart before expecting the daemon to use the configured smoke target
- `blocked`
  - fix the reported blocker first, then rerun `status`

`doctor` must pass before `run`. In smoke mode it now acts as the real venue preflight gate: expect sectioned `Config / Credentials / Connectivity / Target Source / Runtime Safety` output, real authenticated REST plus ws plus heartbeat plus relayer probes, and explicit next actions at the end. The smoke guard keeps the `neg-risk` route on the shadow path even though the runtime itself is in `live` mode. `bootstrap --start` reuses that same `doctor -> run` sequence; it does not bypass preflight.

Interpret the ending of the `doctor` report like this:

- `Overall: PASS`
  - the config is ready for `run`
- `Overall: PASS WITH SKIPS`
  - acceptable only when the skipped checks match the current mode or intentionally unresolved rollout state
- `Overall: FAIL`
  - follow the printed next action first, then rerun `doctor`

When `doctor` prints next actions for target setup, the normal follow-up is:

```bash
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

If the wrong target revision was adopted for the smoke, revert it with:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
```

Adopt and rollback only rewrite the configured `operator_target_revision` in the TOML. They do not hot-reload the running daemon.

If you need to capture replay-visible state after the smoke, run:

```bash
cargo run -p app-replay -- --config config/axiom-arb.local.toml --from-seq 0 --limit 1000
```

## SQL Checks

Check that `neg-risk` only produced shadow attempts:

```sql
SELECT execution_mode, route, count(*)
FROM execution_attempts
WHERE route = 'neg-risk'
GROUP BY execution_mode, route
ORDER BY execution_mode;
```

Expected:

- one or more `shadow` rows
- no `live` rows for the smoke run

Check that shadow artifacts exist for those attempts:

```sql
SELECT ea.attempt_id, ea.execution_mode, sa.stream, sa.payload
FROM execution_attempts ea
JOIN shadow_execution_artifacts sa
  ON sa.attempt_id = ea.attempt_id
WHERE ea.route = 'neg-risk'
ORDER BY ea.attempt_id, sa.stream;
```

Expected:

- at least one `neg-risk` shadow attempt
- at least one shadow artifact per attempt
- artifact payloads stay attached to the same attempt id

## Pass / Fail Rubric

Pass if all of the following are true:

- `app-live` starts with the smoke guard enabled and stays on the shadow path for `neg-risk`
- `execution_attempts` contains `neg-risk` rows in `shadow` mode only
- `shadow_execution_artifacts` contains rows for those attempts
- `app-replay` emits the structured `app-replay neg-risk shadow smoke` summary when those rows exist

If rollout readiness is intentionally left empty, zero `neg-risk` shadow rows is an expected outcome and this runbook should be treated as `preflight-ready smoke startup`, not `shadow-work-ready smoke startup`.

Fail if any of the following happen:

- `app-live` runs in paper mode
- any `neg-risk` row is recorded as `live`
- shadow artifacts exist without a matching shadow attempt
- the replay summary does not report the smoke rows
