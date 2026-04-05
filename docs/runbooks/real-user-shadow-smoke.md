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

For smoke, `bootstrap` is the Day 0 happy path. It reuses the init wizard, keeps startup authority on `[negrisk.target_source].operator_target_revision`, and when no target anchor or discovery artifacts exist yet it runs `discover` first. From there it either lists adoptable revisions and waits for explicit confirmation, or it truthfully stops at `discovery-ready-not-adoptable` with the recorded reasons. If you prefer the lower-level Day 0 fallback, run `app-live init`, then `app-live discover`, then `targets candidates`, and finally `targets adopt`. The smoke config must keep rollout lists empty until the operator explicitly confirms the smoke-only rollout posture:

```toml
[runtime]
mode = "live"
real_user_shadow_smoke = true
```

Do not use paper mode. Do not hand-author transient auth values or raw `negrisk.targets` members for the normal adopted-target startup path; let the config model and startup flow resolve the adopted target revision. Empty rollout lists are the safe default until an adoptable revision is explicitly chosen, and adopting a revision alone does not create the `neg-risk` shadow-ready rollout state. Discovery persistence and adoption lineage are different lifecycle facts: `discover` writes candidate/adoptable artifacts, while `targets adopt` writes canonical provenance and adoption history for the chosen startup revision.

If bootstrap stops in `preflight-ready smoke startup`, the config is valid and `doctor` can still pass, but a later run is expected to produce zero `neg-risk` shadow rows. If bootstrap reaches `shadow-work-ready smoke startup`, it has already written the adopted family ids into both rollout lists. After that Day 0 setup, keep using `status` and `apply` for Day 1+ smoke progression instead of re-running bootstrap as a generic operator action.

The runtime now records a durable `run_session`, so the operator-facing lifecycle summary is session-aware instead of purely heuristic:

- `status` shows the latest relevant run session for this smoke config.
- `status` also shows a conflicting active run session when the active daemon is stale or points at a different startup anchor.
- `stale` is projected from freshness; it is not stored as a separate durable truth value.
- `verify` defaults to the latest relevant run session and only falls back to evidence-only when a historical window cannot be tied to a single session.

Before running the smoke with the lower-level Day 0 fallback, materialize discovery artifacts first, then inspect the current control-plane state and adopt the startup-scoped target revision you intend to test:

```bash
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

`status` should now tell you whether the smoke is:

- `discovery-required`
- `discovery-ready-not-adoptable`
- `adoptable-ready`
- `restart-required`
- `smoke-rollout-required`
- `smoke-config-ready`
- `blocked`

Use `targets status` and `targets show-current` only when you need the lower-level control-plane provenance behind that summary. For the preferred high-level path, hand the smoke back to `apply`; it can inline the same adopt and rollout work without changing startup authority.

## Run

With the preferred high-level flow, use `bootstrap` for Day 0, then `status` and `apply` for Day 1+ smoke progression:

```bash
cargo run -p app-live -- bootstrap --config config/axiom-arb.local.toml
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
```

`apply` is smoke-only. It reuses `status`, can inline an explicit adopt only when readiness is already `adoptable-ready`, can explicitly enable smoke-only rollout for the adopted families, runs `doctor`, and stops at ready unless you also pass `--start`. It does not run `discover`, and it does not chain `verify`; run that explicitly after the runtime work you want to inspect.

If you are using the lower-level path, preflight the smoke config, then start `app-live` with the smoke config:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

Interpret `status` before `apply` like this:

- `discovery-required`
  - no discovery artifacts exist yet
  - rerun `bootstrap` for the Day 0 happy path, or run `discover` if you want the low-level fallback
- `discovery-ready-not-adoptable`
  - discovery artifacts exist, but nothing is adoptable yet
  - inspect the reasons with `targets candidates`; `apply` does not bypass this state
- `adoptable-ready`
  - discovery has already produced one or more adoptable revisions
  - `apply` can inline the adoptable revision selection for smoke
  - or drop to `targets candidates` / `targets adopt` if you want lower-level control
- `smoke-rollout-required`
  - config and adopted target are present, but rollout is still intentionally empty
  - `apply` can explicitly enable the smoke-only rollout posture for the adopted family set
- `smoke-config-ready`
  - config, adopted target, and smoke rollout are aligned
  - `apply` will run `doctor` and stop at ready, or continue into `run` if you pass `--start`
- `restart-required`
  - the configured revision is not the active revision yet
  - `apply --start` will stop at a manual restart boundary and require explicit confirmation before it enters foreground `run`
- `blocked`
  - fix the reported blocker first, then rerun `status` or `apply`

`doctor` must pass before `run`. In smoke mode it now acts as the real venue preflight gate: expect sectioned `Config / Credentials / Connectivity / Target Source / Runtime Safety` output, real authenticated REST plus ws plus heartbeat plus relayer probes, and explicit next actions at the end. The smoke guard keeps the `neg-risk` route on the shadow path even though the runtime itself is in `live` mode. `apply` reuses that same `doctor -> run` sequence; it does not bypass preflight, and it does not claim any process-management ability beyond the foreground `run` it starts itself.

After `run`, use the high-level verifier as the default happy path:

```bash
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

Interpret the verifier like this:

- `Scenario: real-user shadow smoke`
  - confirms the verifier inferred the smoke path
- `Verdict: PASS`
  - shadow-only run evidence is present
  - no forbidden live side effects were observed
- `Verdict: PASS WITH WARNINGS`
  - a real smoke run happened
  - but rollout-not-ready or missing stronger replay evidence makes the result incomplete
- `Verdict: FAIL`
  - no credible run evidence exists
  - or forbidden live side effects were observed

`verify` is intentionally local-only. It does not do venue probes and it does not replace `doctor`; it validates what the latest local smoke run actually produced. Its `Run Session` line is the concrete lifecycle anchor for the verdict and should appear before `Next Actions`.

Interpret the ending of the `doctor` report like this:

- `Overall: PASS`
  - the config is ready for `run`
- `Overall: PASS WITH SKIPS`
  - acceptable only when the skipped checks match the current mode or intentionally unresolved rollout state
- `Overall: FAIL`
  - follow the printed next action first, then rerun `doctor`

When `doctor` prints next actions for target setup and you stay on the high-level smoke path, the normal Day 1+ follow-up is:

```bash
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
```

If you need the lower-level target workflow instead, use:

```bash
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <ADOPTABLE_REVISION>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

If the wrong target revision was adopted for the smoke, revert it with:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
```

Adopt and rollback only rewrite the configured `operator_target_revision` in the TOML. They do not hot-reload the running daemon.

If you need deeper debugging beyond `verify`, capture replay-visible state after the smoke with:

```bash
cargo run -p app-replay -- --config config/axiom-arb.local.toml --from-seq 0 --limit 1000
```

## Deeper Debugging

Use SQL only when `verify` fails or when you need to inspect the raw evidence behind its verdict.

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

Treat `app-live verify --config config/axiom-arb.local.toml` as the primary verdict:

- `PASS`
  - smoke-only behavior is evidenced locally
  - the report names the latest relevant `Run session`
- `PASS WITH WARNINGS`
  - smoke executed, but rollout/readiness or replay strength remains incomplete
- `FAIL`
  - live side effects appeared, or there is no credible local smoke run evidence

If rollout readiness is intentionally left empty, zero `neg-risk` shadow rows is an expected outcome and this runbook should be treated as `preflight-ready smoke startup`, not `shadow-work-ready smoke startup`. In that case `verify` should not produce `PASS`; it should either warn or fail depending on whether a real run happened.
