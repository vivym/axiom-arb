CREATE TABLE strategy_candidate_sets (
  strategy_candidate_revision TEXT PRIMARY KEY,
  snapshot_id TEXT NOT NULL,
  source_revision TEXT NOT NULL,
  payload JSONB NOT NULL
);

CREATE TABLE adoptable_strategy_revisions (
  adoptable_strategy_revision TEXT PRIMARY KEY,
  strategy_candidate_revision TEXT NOT NULL
    REFERENCES strategy_candidate_sets (strategy_candidate_revision),
  rendered_operator_strategy_revision TEXT NOT NULL,
  payload JSONB NOT NULL,
  UNIQUE (adoptable_strategy_revision, strategy_candidate_revision)
);

CREATE TABLE strategy_adoption_provenance (
  operator_strategy_revision TEXT PRIMARY KEY,
  adoptable_strategy_revision TEXT NOT NULL,
  strategy_candidate_revision TEXT NOT NULL,
  FOREIGN KEY (adoptable_strategy_revision, strategy_candidate_revision)
    REFERENCES adoptable_strategy_revisions (
      adoptable_strategy_revision,
      strategy_candidate_revision
    )
);

CREATE TABLE operator_strategy_adoption_history (
  adoption_id TEXT PRIMARY KEY,
  action_kind TEXT NOT NULL,
  operator_strategy_revision TEXT NOT NULL,
  previous_operator_strategy_revision TEXT,
  adoptable_strategy_revision TEXT,
  strategy_candidate_revision TEXT,
  adopted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  history_seq BIGSERIAL NOT NULL
);

ALTER TABLE operator_strategy_adoption_history
  ADD CONSTRAINT operator_strategy_adoption_history_action_kind_check
  CHECK (action_kind IN ('adopt', 'rollback'));

ALTER TABLE operator_strategy_adoption_history
  ADD CONSTRAINT operator_strategy_adoption_history_row_shape_check
  CHECK (
    (action_kind = 'adopt'
      AND adoptable_strategy_revision IS NOT NULL
      AND strategy_candidate_revision IS NOT NULL)
    OR (action_kind = 'rollback'
      AND adoptable_strategy_revision IS NULL
      AND strategy_candidate_revision IS NULL)
  );

CREATE UNIQUE INDEX operator_strategy_adoption_history_history_seq_idx
  ON operator_strategy_adoption_history (history_seq DESC);

CREATE INDEX operator_strategy_adoption_history_revision_history_seq_idx
  ON operator_strategy_adoption_history (operator_strategy_revision, history_seq DESC)
  WHERE previous_operator_strategy_revision IS NOT NULL;

INSERT INTO strategy_candidate_sets (
  strategy_candidate_revision,
  snapshot_id,
  source_revision,
  payload
)
SELECT candidate_revision, snapshot_id, source_revision, payload
FROM candidate_target_sets
ORDER BY candidate_revision;

INSERT INTO adoptable_strategy_revisions (
  adoptable_strategy_revision,
  strategy_candidate_revision,
  rendered_operator_strategy_revision,
  payload
)
SELECT
  adoptable_revision,
  candidate_revision,
  rendered_operator_target_revision,
  payload
FROM adoptable_target_revisions
ORDER BY adoptable_revision;

INSERT INTO strategy_adoption_provenance (
  operator_strategy_revision,
  adoptable_strategy_revision,
  strategy_candidate_revision
)
SELECT operator_target_revision, adoptable_revision, candidate_revision
FROM candidate_adoption_provenance
ORDER BY operator_target_revision;

INSERT INTO operator_strategy_adoption_history (
  adoption_id,
  action_kind,
  operator_strategy_revision,
  previous_operator_strategy_revision,
  adoptable_strategy_revision,
  strategy_candidate_revision,
  adopted_at,
  history_seq
)
SELECT
  adoption_id,
  action_kind,
  operator_target_revision,
  previous_operator_target_revision,
  adoptable_revision,
  candidate_revision,
  adopted_at,
  row_number() OVER (
    ORDER BY
      COALESCE(history_seq, 9223372036854775807),
      adopted_at ASC,
      adoption_id ASC
  ) AS history_seq
FROM operator_target_adoption_history
ORDER BY history_seq;

SELECT setval(
  'operator_strategy_adoption_history_history_seq_seq',
  COALESCE((SELECT MAX(history_seq) FROM operator_strategy_adoption_history), 1),
  EXISTS(SELECT 1 FROM operator_strategy_adoption_history)
);

ALTER TABLE runtime_apply_progress
  ADD COLUMN operator_strategy_revision TEXT;

UPDATE runtime_apply_progress
SET operator_strategy_revision = operator_target_revision
WHERE operator_strategy_revision IS NULL
  AND operator_target_revision IS NOT NULL;

ALTER TABLE run_sessions
  ADD COLUMN configured_operator_strategy_revision TEXT;

ALTER TABLE run_sessions
  ADD COLUMN active_operator_strategy_revision_at_start TEXT;

UPDATE run_sessions
SET configured_operator_strategy_revision = configured_operator_target_revision
WHERE configured_operator_strategy_revision IS NULL
  AND configured_operator_target_revision IS NOT NULL;

UPDATE run_sessions
SET active_operator_strategy_revision_at_start = active_operator_target_revision_at_start
WHERE active_operator_strategy_revision_at_start IS NULL
  AND active_operator_target_revision_at_start IS NOT NULL;
