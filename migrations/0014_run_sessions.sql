CREATE TABLE run_sessions (
  run_session_id TEXT PRIMARY KEY,
  invoked_by TEXT NOT NULL,
  mode TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('starting', 'running', 'exited', 'failed')),
  started_at TIMESTAMPTZ NOT NULL,
  last_seen_at TIMESTAMPTZ NOT NULL,
  ended_at TIMESTAMPTZ,
  exit_status TEXT,
  exit_reason TEXT,
  config_path TEXT NOT NULL,
  config_fingerprint TEXT NOT NULL,
  target_source_kind TEXT NOT NULL,
  startup_target_revision_at_start TEXT NOT NULL,
  configured_operator_target_revision TEXT,
  active_operator_target_revision_at_start TEXT,
  rollout_state_at_start TEXT,
  real_user_shadow_smoke BOOLEAN NOT NULL
);

ALTER TABLE runtime_apply_progress
  ADD COLUMN active_run_session_id TEXT REFERENCES run_sessions(run_session_id);

ALTER TABLE execution_attempts
  ADD COLUMN run_session_id TEXT REFERENCES run_sessions(run_session_id);

CREATE INDEX run_sessions_state_started_idx ON run_sessions (state, started_at DESC);
CREATE INDEX execution_attempts_run_session_idx ON execution_attempts (run_session_id, created_at DESC);
