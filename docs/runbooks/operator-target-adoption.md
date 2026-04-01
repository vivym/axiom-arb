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

## 2. Check Current Configured Vs Active State

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

## 3. Adopt A New Target Revision

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

## 4. Restart To Activate The Newly Adopted Revision

Adoption is startup-scoped. It does not change the target revision inside a running daemon.

After a successful adopt:

1. re-run `targets status`
2. if `restart_needed = true`, perform a controlled restart
3. run `doctor`
4. start `run`

```bash
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

After the daemon comes back up, confirm the active revision caught up:

```bash
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

## 5. Roll Back

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
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- targets show-current --config config/axiom-arb.local.toml
```

If `restart_needed = true`, perform a controlled restart before expecting the daemon to use the rolled-back revision.

## 6. Recommended Operator Flow

For day-to-day target control-plane operations, use this sequence:

```bash
cargo run -p app-live -- targets candidates --config config/axiom-arb.local.toml
cargo run -p app-live -- targets adopt --config config/axiom-arb.local.toml --adoptable-revision <adoptable-revision>
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
cargo run -p app-live -- doctor --config config/axiom-arb.local.toml
cargo run -p app-live -- run --config config/axiom-arb.local.toml
```

For rollback:

```bash
cargo run -p app-live -- targets rollback --config config/axiom-arb.local.toml
cargo run -p app-live -- targets status --config config/axiom-arb.local.toml
```

## 7. Troubleshooting

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

## 8. Safety Rules

- `operator_target_revision` is the only startup and restore authority
- `candidate_revision` and `adoptable_revision` are selection and provenance context only
- do not treat `targets adopt` as a live reload
- do not hand-edit raw target payloads for the normal adopted-target startup path
- always check `configured` vs `active` before assuming a new revision is in effect
