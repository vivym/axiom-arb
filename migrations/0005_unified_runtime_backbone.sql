CREATE TABLE runtime_apply_progress (
  progress_key TEXT PRIMARY KEY,
  last_journal_seq BIGINT NOT NULL,
  last_state_version BIGINT NOT NULL,
  last_snapshot_id TEXT,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE snapshot_publications (
  snapshot_id TEXT PRIMARY KEY,
  state_version BIGINT NOT NULL,
  committed_journal_seq BIGINT NOT NULL,
  fullset_ready BOOLEAN NOT NULL,
  negrisk_ready BOOLEAN NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::JSONB,
  published_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE execution_attempts (
  attempt_id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  execution_mode TEXT NOT NULL,
  attempt_no INTEGER NOT NULL,
  idempotency_key TEXT NOT NULL,
  outcome TEXT,
  payload JSONB NOT NULL DEFAULT '{}'::JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE pending_reconcile_items (
  pending_ref TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  payload JSONB NOT NULL DEFAULT '{}'::JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE shadow_execution_artifacts (
  artifact_id BIGSERIAL PRIMARY KEY,
  attempt_id TEXT NOT NULL,
  stream TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
