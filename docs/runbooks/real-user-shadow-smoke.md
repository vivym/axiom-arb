# Real-User Shadow Smoke Runbook

Use this only for a manual operator smoke. It is shadow-only for `neg-risk` and does not represent production readiness.

## Preflight

Set the same database that `app-live` and `app-replay` will read. `DATABASE_URL` remains the only deployment env var.

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

Prepare a config file for the smoke, for example by copying `config/axiom-arb.example.toml` and replacing the placeholder signer, relayer auth, and target values. The smoke config must set:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = true
```

Do not use paper mode.

## Run

Start `app-live` with the smoke config:

```bash
cargo run -p app-live -- --config config/axiom-arb.example.toml
```

If you need to capture replay-visible state after the smoke, run:

```bash
cargo run -p app-replay -- --config config/axiom-arb.example.toml --from-seq 0 --limit 1000
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

Fail if any of the following happen:

- `app-live` runs in paper mode
- any `neg-risk` row is recorded as `live`
- shadow artifacts exist without a matching shadow attempt
- the replay summary does not report the smoke rows
