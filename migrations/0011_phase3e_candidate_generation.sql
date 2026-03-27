CREATE TABLE candidate_target_sets (
  candidate_revision TEXT PRIMARY KEY,
  snapshot_id TEXT NOT NULL,
  source_revision TEXT NOT NULL,
  payload JSONB NOT NULL
);

CREATE TABLE adoptable_target_revisions (
  adoptable_revision TEXT PRIMARY KEY,
  candidate_revision TEXT NOT NULL REFERENCES candidate_target_sets (candidate_revision),
  rendered_operator_target_revision TEXT NOT NULL,
  payload JSONB NOT NULL
);

CREATE TABLE candidate_adoption_provenance (
  operator_target_revision TEXT PRIMARY KEY,
  adoptable_revision TEXT NOT NULL REFERENCES adoptable_target_revisions (adoptable_revision),
  candidate_revision TEXT NOT NULL REFERENCES candidate_target_sets (candidate_revision)
);
