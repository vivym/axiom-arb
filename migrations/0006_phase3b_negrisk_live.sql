CREATE TABLE live_execution_artifacts (
  artifact_id BIGSERIAL PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES execution_attempts (attempt_id),
  stream TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE OR REPLACE FUNCTION enforce_live_execution_artifact_attempt()
RETURNS TRIGGER AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM execution_attempts
    WHERE attempt_id = NEW.attempt_id
      AND execution_mode = 'live'
  ) THEN
    RAISE EXCEPTION
      'live_execution_artifacts requires a live execution attempt for attempt_id %',
      NEW.attempt_id;
  END IF;

  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER live_execution_artifacts_enforce_live_attempt
BEFORE INSERT OR UPDATE ON live_execution_artifacts
FOR EACH ROW
EXECUTE FUNCTION enforce_live_execution_artifact_attempt();

CREATE OR REPLACE FUNCTION prevent_live_attempt_mode_drift()
RETURNS TRIGGER AS $$
BEGIN
  IF OLD.execution_mode = 'live'
     AND NEW.execution_mode <> 'live'
     AND EXISTS (
       SELECT 1
       FROM live_execution_artifacts
       WHERE attempt_id = OLD.attempt_id
     ) THEN
    RAISE EXCEPTION
      'execution_attempts with live artifacts cannot change away from live for attempt_id %',
      OLD.attempt_id;
  END IF;

  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER execution_attempts_prevent_live_mode_drift
BEFORE UPDATE ON execution_attempts
FOR EACH ROW
EXECUTE FUNCTION prevent_live_attempt_mode_drift();
