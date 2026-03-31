CREATE SEQUENCE operator_target_adoption_history_history_seq_seq;

ALTER TABLE operator_target_adoption_history
  ADD COLUMN history_seq BIGINT;

ALTER TABLE operator_target_adoption_history
  ALTER COLUMN history_seq SET DEFAULT nextval('operator_target_adoption_history_history_seq_seq');

WITH ordered_history AS (
  SELECT
    ctid,
    row_number() OVER (ORDER BY adopted_at ASC, adoption_id ASC) AS history_seq
  FROM operator_target_adoption_history
)
UPDATE operator_target_adoption_history AS history
SET history_seq = ordered_history.history_seq
FROM ordered_history
WHERE history.ctid = ordered_history.ctid;

ALTER TABLE operator_target_adoption_history
  ALTER COLUMN history_seq SET NOT NULL;

ALTER SEQUENCE operator_target_adoption_history_history_seq_seq
  OWNED BY operator_target_adoption_history.history_seq;

ALTER TABLE operator_target_adoption_history
  ADD CONSTRAINT operator_target_adoption_history_action_kind_check
  CHECK (action_kind IN ('adopt', 'rollback'));

ALTER TABLE operator_target_adoption_history
  ADD CONSTRAINT operator_target_adoption_history_row_shape_check
  CHECK (
    (action_kind = 'adopt' AND adoptable_revision IS NOT NULL AND candidate_revision IS NOT NULL)
    OR (action_kind = 'rollback' AND adoptable_revision IS NULL AND candidate_revision IS NULL)
  );

SELECT setval(
  'operator_target_adoption_history_history_seq_seq',
  COALESCE((SELECT MAX(history_seq) FROM operator_target_adoption_history), 1),
  EXISTS(SELECT 1 FROM operator_target_adoption_history)
);

CREATE UNIQUE INDEX operator_target_adoption_history_history_seq_idx
  ON operator_target_adoption_history (history_seq DESC);

CREATE INDEX operator_target_adoption_history_revision_history_seq_idx
  ON operator_target_adoption_history (operator_target_revision, history_seq DESC)
  WHERE previous_operator_target_revision IS NOT NULL;
