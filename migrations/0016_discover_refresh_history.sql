CREATE TABLE discover_refresh_history (
  refresh_seq BIGSERIAL PRIMARY KEY,
  strategy_candidate_revision TEXT NOT NULL
    REFERENCES strategy_candidate_sets (strategy_candidate_revision),
  recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX discover_refresh_history_strategy_candidate_revision_refresh_seq_idx
  ON discover_refresh_history (strategy_candidate_revision, refresh_seq DESC);
