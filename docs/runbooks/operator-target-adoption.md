# Operator Strategy Adoption Runbook

This runbook covers the lower-level control-plane workflow for startup-scoped strategy adoption.

For a fresh database, prefer `app-live bootstrap`. Use this runbook when you intentionally want direct control over:

- `discover`
- `targets candidates`
- `targets adopt`
- `targets rollback`
- lower-level readiness and provenance inspection

The steady-state control-plane shape is canonical:

- `[strategy_control]`
- `source = "adopted"`
- `operator_strategy_revision = "..."`

Legacy explicit target input is not a normal operating mode. Read-only commands report migration-required guidance, and `targets adopt --config ...` is the explicit rewrite path into canonical `[strategy_control]`.

## Preconditions

1. Start local Postgres and set `DATABASE_URL`.
2. Prepare a local config such as `config/axiom-arb.local.toml`.
3. Keep startup authority on `[strategy_control]`.
4. Do not hand-edit `operator_strategy_revision` during normal operations; use the control-plane commands below.

```bash
export DATABASE_URL=postgres://axiom:axiom@localhost:5432/axiom_arb
```

## 1. Inspect Candidate And Adoptable State

Materialize or refresh discovery artifacts, then inspect strategy candidates and adoptable revisions:

```bash
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
```

Expected output shape:

- `advisory strategy_candidate_revision = ...`
- `adoptable adoptable_revision = ... strategy_candidate_revision = ... operator_strategy_revision = ...`
- `adopted operator_strategy_revision = ... adoptable_revision = ... strategy_candidate_revision = ...`

Use this output to decide which revision you want to adopt. Advisory rows are discovery artifacts only; adoptable rows are valid inputs to `targets adopt`.

## 2. Check High-Level Readiness First

Start with the high-level readiness view:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
```

`status` is the operator homepage for config and control-plane readiness. It tells you whether the next action is:

- `discover`
- `targets candidates`
- `targets adopt`
- `apply`
- a controlled restart
- `doctor`
- `run`

For `real-user shadow smoke`, `apply` can still inline smoke-only strategy adoption and rollout enablement. For live configs, once adoption is complete and rollout posture exists, Day 1+ progression returns to conservative `apply`.

## 3. Check Configured Vs Active Strategy State

Get a compact summary:

```bash
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
```

This prints:

- `configured_operator_strategy_revision`
- `active_operator_strategy_revision`
- `restart_needed`
- `provenance`
- `latest_action`

For a more detailed explainability view:

```bash
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

That view also shows:

- `adoptable_revision`
- `strategy_candidate_revision`
- `latest_action_kind`
- `latest_action_operator_strategy_revision`

Interpretation:

- if `configured_operator_strategy_revision = active_operator_strategy_revision`, the running daemon is already using the configured revision
- if they differ, the new strategy revision is configured but not yet active
- `restart_needed = true` means a controlled restart is required before the daemon will use the configured revision

## 4. Adopt A New Strategy Revision

The normal path is to adopt from an `adoptable_revision`:

```bash
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <adoptable-revision>
```

This rewrites `[strategy_control].operator_strategy_revision` in the TOML, writes or reuses canonical strategy adoption provenance for that startup revision, and appends adoption history.

The command prints:

- `operator_strategy_revision`
- `previous_operator_strategy_revision`
- `adoptable_revision`
- `strategy_candidate_revision`
- `restart_required`

Only use direct strategy-revision adoption when you already know the durable lineage is present:

```bash
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --operator-strategy-revision <operator-strategy-revision>
```

Fail-closed cases include:

- providing both selector flags
- providing neither selector flag
- selecting an adoptable revision that does not resolve to exactly one rendered operator strategy revision
- selecting a revision whose durable provenance is missing or malformed

Discovery and adoption are intentionally different lifecycle steps:

- `discover` persists advisory candidate rows and adoptable revision rows
- `targets adopt` writes canonical provenance that links the chosen `operator_strategy_revision` back to its adoptable and strategy-candidate revisions
- repeated adoption of the same `operator_strategy_revision` preserves that canonical provenance and still appends a new adoption-history row

If read-only commands report migration-required legacy input, `targets adopt --config ...` is also the explicit config rewrite step into canonical `[strategy_control]`.

## 5. Restart To Activate The Newly Adopted Revision

Adoption is startup-scoped. It does not hot-reload a running daemon.

After a successful adopt:

1. re-run `status`
2. re-run `targets status` if you need the lower-level control-plane details
3. once rollout posture exists, return to the high-level Day 1+ flow
4. keep the controlled restart boundary explicit before `run`
5. verify the local result

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

For `real-user shadow smoke`, `apply` can still inline smoke-only strategy adoption and rollout enablement here. For live configs, return to `apply` only after adoption is complete and rollout posture exists; it stays conservative, does not inline live rollout mutation, and keeps `verify` as a separate post-run check. If `status` still reports `live-rollout-required` after adoption, update route-owned rollout scopes before returning to `apply`.

`doctor` is the preflight gate after adoption. It reports sectioned `Config / Credentials / Target Source / Runtime Safety / Connectivity` output, checks venue-facing readiness when those probes apply, and ends with explicit next actions. If `doctor` reports migration-required legacy input, run `targets adopt --config ...` before trying to continue. If it reports canonical rebuild or manual TOML repair, follow that remediation rather than hand-editing live state blindly.

After the daemon comes back up, confirm the active revision caught up:

```bash
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

## 6. Roll Back

Roll back to the previous adopted revision:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
```

Or roll back to an explicit older strategy revision:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml --to-operator-strategy-revision <operator-strategy-revision>
```

Rollback behavior:

- rewrites the configured `operator_strategy_revision` in the TOML
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

For day-to-day strategy control-plane operations, use this sequence:

```bash
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- discover --config config/axiom-arb.local.toml
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <adoptable-revision>
cargo run -p app-live -- status --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml
cargo run -p app-live -- apply --config config/axiom-arb.local.toml --start
cargo run -p app-live -- verify --config config/axiom-arb.local.toml
```

Interpret `status` before `apply` like this:

- `discovery-required`
  - run `bootstrap` for the preferred Day 0 flow, or `discover` if you are intentionally using the low-level fallback
- `discovery-ready-not-adoptable`
  - discovery artifacts exist, but adoption is still blocked by the recorded reasons
  - inspect `targets candidates` before trying anything else
- `adoptable-ready`
  - for `real-user shadow smoke`, prefer `apply`; it can inline the adopt step now that adoptable artifacts already exist
  - otherwise run `targets adopt`
- `restart-required`
  - perform a controlled restart before expecting the running daemon to pick up the configured revision
- `live-rollout-required` or `smoke-rollout-required`
  - for `real-user shadow smoke`, `apply` can inline smoke-only rollout enablement
  - for live configs, fix rollout readiness first, then return to `apply`
- `live-config-ready` or `smoke-config-ready`
  - continue to `apply`; it will run `doctor` and stop at ready, or continue into `run` if you pass `--start`
- `blocked`
  - follow the printed next action and rerun `status`

## 8. Troubleshooting

If `status` shows:

- `discovery-required`
  - no discovery artifacts exist yet for the adopted strategy path
- `discovery-ready-not-adoptable`
  - discovery artifacts exist, but the recorded reasons still block adoption
- `adoptable-ready`
  - no startup-scoped adopted strategy revision is configured yet, but adoptable input now exists
- `restart-required`
  - the configured revision differs from the daemon's active revision
- `blocked`
  - follow the printed next action first; if you need lineage detail, fall back to `targets status` / `targets show-current`

If `targets status` shows:

- `configured_operator_strategy_revision = unavailable`
  - no startup-scoped strategy revision is currently configured
- `provenance = unavailable`
  - the configured revision exists but cannot be resolved back through durable provenance
- `restart_needed = true`
  - adoption or rollback succeeded, but a controlled restart is still required

If `targets adopt` or `targets rollback` fails:

- re-run `targets candidates` to verify the available revision ids
- re-run `targets show-current` to inspect the current provenance chain
- verify the config path is the same file you intend `doctor` and `run` to use

## 9. Safety Rules

- `operator_strategy_revision` is the only startup and restore authority
- `strategy_candidate_revision` and `adoptable_revision` are selection and provenance context only
- do not treat `targets adopt` as a live reload
- do not hand-edit raw route payloads for the normal adopted-revision startup path
- always check `configured` vs `active` before assuming a new revision is in effect
