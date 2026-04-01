# Real-User Shadow Smoke Runbook

Use this only for a manual operator smoke. The smoke path is shadow-only for `neg-risk` and does not represent production readiness or live-submit readiness.

## Preflight

Set the same database that `app-live` and `app-replay` will read. `DATABASE_URL` remains the only deployment env var.

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

Prepare a smoke config at `config/axiom-arb.local.toml` by running the wizard flow with `app-live init --config config/axiom-arb.local.toml` and then stepping through `targets candidates` and `targets adopt --adoptable-revision <ADOPTABLE_REVISION>` before `doctor` and `run`, or by copying `config/axiom-arb.example.toml` and then reviewing the long-lived operator settings. The smoke config must keep rollout lists empty until the wizard surfaces a real adoptable revision:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = true
```

Do not use paper mode. Do not hand-author transient auth values or raw `negrisk.targets` members for the normal adopted-target startup path; let the config model and startup flow resolve the adopted target revision. Empty rollout lists are the safe default until an adoptable revision is explicitly chosen, and adopting a revision alone does not create the `neg-risk` shadow-ready rollout state.

If you are only checking connectivity and smoke preflight, stop after `doctor` and `run` and expect zero `neg-risk` shadow rows. If you want the later SQL checks to expect shadow attempts, populate `approved_families` and `ready_families` first, or otherwise establish rollout readiness before starting the run.

Before running the smoke, inspect the current control-plane state and adopt the startup-scoped target revision you intend to test:

```bash
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
```

`targets status` and `targets show-current` should tell you whether the configured revision is already active or whether the smoke run still needs a controlled restart to pick up the new startup-scoped target revision.

## Run

Preflight the smoke config, then start `app-live` with the smoke config:

```bash
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

`doctor` must pass before `run`. In smoke mode it now acts as the real venue preflight gate: expect sectioned `Config / Credentials / Connectivity / Target Source / Runtime Safety` output, real authenticated REST plus ws plus heartbeat plus relayer probes, and explicit next actions at the end. The smoke guard keeps the `neg-risk` route on the shadow path even though the runtime itself is in `live` mode.

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

If rollout readiness is intentionally left empty, zero `neg-risk` shadow rows is an expected outcome and this runbook should be treated as a preflight-only smoke.

Fail if any of the following happen:

- `app-live` runs in paper mode
- any `neg-risk` row is recorded as `live`
- shadow artifacts exist without a matching shadow attempt
- the replay summary does not report the smoke rows
