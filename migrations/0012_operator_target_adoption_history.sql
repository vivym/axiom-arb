CREATE TABLE operator_target_adoption_history (
  adoption_id TEXT PRIMARY KEY,
  action_kind TEXT NOT NULL,
  operator_target_revision TEXT NOT NULL,
  previous_operator_target_revision TEXT,
  adoptable_revision TEXT,
  candidate_revision TEXT,
  adopted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
