# Operator Target Adoption Runbook

This runbook covers the operator-facing control-plane workflow for startup-scoped `neg-risk` target revisions.

Use it when you need to:

- inspect advisory candidates and adoptable revisions
- confirm which `operator_target_revision` is currently configured or active
- adopt a new startup-scoped target revision
- roll back to the previous adopted revision or an explicit older revision
- determine whether a controlled restart is required

This workflow stays under `app-live targets ...`. It does not hot-reload a running daemon.

## Preconditions

1. Start local Postgres and set `DATABASE_URL`.
2. Prepare a local config file such as `config/axiom-arb.local.toml`.
3. Keep `[negrisk.target_source].source = "adopted"` in that TOML.
4. Do not hand-edit `[negrisk.target_source].operator_target_revision` during normal operations. Use the control-plane commands below instead.

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

## 1. Inspect Candidate And Adoptable State

List advisory candidates, adoptable revisions, and the currently adopted revision:

```bash
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
```

Expected output shape:

- `advisory candidate_revision = ... snapshot_id = ...`
- `adoptable adoptable_revision = ... candidate_revision = ... operator_target_revision = ...`
- `adopted operator_target_revision = ... adoptable_revision = ... candidate_revision = ...`

Use this command to choose the revision you want to adopt. `advisory` rows are not yet adopted. `adoptable` rows are eligible inputs to `targets adopt`.

## 2. Check High-Level Readiness First

Start with the high-level readiness view:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

`status` is the operator homepage for config and control-plane readiness. It tells you whether the next step is:

- `targets adopt`
- `apply` for conservative Day 1+ progression
- controlled restart
- `doctor`
- `run`

Once adoption is complete and rollout posture exists, Day 1+ live progression returns to `apply`.

Then drop into the lower-level control-plane views when you need exact provenance.

## 3. Check Current Configured Vs Active State

Get a compact status summary:

```bash
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
```

This prints:

- `configured_operator_target_revision`
- `active_operator_target_revision`
- `restart_needed`
- `provenance`
- `latest_action`

For a more detailed explainability view, use:

```bash
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

That view also shows:

- `adoptable_revision`
- `candidate_revision`
- `latest_action_kind`
- `latest_action_operator_target_revision`

Interpretation:

- if `configured_operator_target_revision = active_operator_target_revision`, the running daemon is already using the configured target revision
- if they differ, the new target is configured but not yet active
- `restart_needed = true` means a controlled restart is required before the daemon will use the configured revision

## 4. Adopt A New Target Revision

The normal path is to adopt from an `adoptable_revision`:

```bash
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <adoptable-revision>
```

This rewrites `[negrisk.target_source].operator_target_revision` in the TOML and records adoption history.

The command prints:

- `operator_target_revision`
- `previous_operator_target_revision`
- `adoptable_revision`
- `candidate_revision`
- `restart_required`

Only use direct `operator_target_revision` adoption when you already know the durable lineage is present:

```bash
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --operator-target-revision <operator-target-revision>
```

Fail-closed cases:

- providing both selector flags
- providing neither selector flag
- selecting an adoptable revision that does not resolve to exactly one rendered operator target revision
- selecting a revision whose durable provenance is missing or malformed

## 5. Restart To Activate The Newly Adopted Revision

Adoption is startup-scoped. It does not change the target revision inside a running daemon.

After a successful adopt:

1. re-run `status`
2. re-run `targets status` if you need the lower-level control-plane details
3. once adoption and rollout posture exist, return to `apply` for the conservative Day 1+ flow
4. let `apply` run `doctor` after readiness is in place
5. if `restart_needed = true`, perform a controlled restart before `run`
6. start `run`
7. verify the local result

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

For `real-user shadow smoke`, the preferred Day 1+ follow-up is:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

`apply` stays conservative here: it follows readiness, does not inline target adoption or rollout mutation, and keeps `verify` as a separate post-run check.

`doctor` is the preflight gate after adoption. It now reports sectioned `Config / Credentials / Connectivity / Target Source / Runtime Safety` output, checks venue-facing live or smoke readiness when those probes apply, and ends with explicit next actions. If `doctor` reports target-source failure, go back to `targets candidates` / `targets adopt` instead of trying to hand-edit the TOML.

`verify` is the post-run local result check. It does not perform venue probes; instead it answers whether the latest local run evidence is consistent with the current mode and control-plane posture. Use it after `run` to confirm the daemon produced the kind of result you expected before moving on.

After the daemon comes back up, confirm the active revision caught up:

```bash
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

## 6. Roll Back

Roll back to the previous adopted revision:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
```

Or roll back to an explicit older `operator_target_revision`:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml --to-operator-target-revision <operator-target-revision>
```

Rollback behavior:

- rewrites the configured `operator_target_revision` in the TOML
- appends adoption history
- preserves startup-scoped semantics
- does not hot-reload the running daemon

After rollback, use the same verification flow as adopt:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

If `restart_needed = true`, perform a controlled restart before expecting the daemon to use the rolled-back revision.

## 7. Recommended Operator Flow

For day-to-day target control-plane operations, use this sequence:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <adoptable-revision>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

For `real-user shadow smoke`, replace the `doctor` / `run` pair with:

```bash
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
```

Interpret `status` before `doctor` like this:

- `target-adoption-required`
  - run `targets candidates`, then `targets adopt`
  - once adoption and rollout posture exist, return to `apply` for Day 1+ progression
- `restart-required`
  - perform a controlled restart before expecting the running daemon to pick up the configured revision
- `live-rollout-required` or `smoke-rollout-required`
  - once rollout readiness is in place, return to `apply`
  - otherwise fix rollout readiness first
- `live-config-ready` or `smoke-config-ready`
  - continue to `apply`, which will run `doctor` after readiness is in place
- `blocked`
  - follow the printed next action and rerun `status`

Interpret the `doctor` result before `run`:

- `PASS`
  - safe to continue to `run`
- `PASS WITH SKIPS`
  - verify the skipped checks match the current mode
- `FAIL`
  - follow the printed next action, then rerun `doctor`

Interpret `verify` after `run`:

- `PASS`
  - the latest local result is consistent with the current mode and control-plane posture
- `PASS WITH WARNINGS`
  - the result is usable but incomplete; follow the printed next action
- `FAIL`
  - local evidence conflicts with the expected posture or is not credible enough; stop and inspect before rerunning

For rollback:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

## 8. Troubleshooting

If `status` shows:

- `target-adoption-required`
  - no startup-scoped adopted target revision is currently configured
  - inspect `targets candidates`, then adopt one
  - after adoption and rollout posture exist, continue with `apply`

- `restart-required`
  - the configured revision differs from the daemon's active revision
  - adoption or rollback succeeded, but a controlled restart is still required

- `blocked`
  - follow the printed next action first
  - if you need lineage detail, fall back to `targets status` / `targets show-current`

If `targets status` shows:

- `configured_operator_target_revision = unavailable`
  - no startup-scoped target revision is currently configured
  - inspect `targets candidates`, then adopt one

- `provenance = unavailable`
  - the configured revision exists but cannot be resolved back through durable provenance
  - do not continue by hand-editing the TOML; adopt a valid revision again

- `restart_needed = true`
  - the configured revision differs from the daemon's active revision
  - adoption succeeded, but a controlled restart is still required

If `targets adopt` or `targets rollback` fails:

- run `targets candidates` to verify the available revision ids
- run `targets show-current` to inspect the current provenance chain
- verify the config path is the same file you intend `doctor` and `run` to use

## 9. Safety Rules

- `operator_target_revision` is the only startup and restore authority
- `candidate_revision` and `adoptable_revision` are selection and provenance context only
- do not treat `targets adopt` as a live reload
- do not hand-edit raw target payloads for the normal adopted-target startup path
- always check `configured` vs `active` before assuming a new revision is in effect
