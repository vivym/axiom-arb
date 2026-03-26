CREATE TABLE live_submission_records (
  submission_ref TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES execution_attempts (attempt_id),
  route TEXT NOT NULL,
  scope TEXT NOT NULL,
  provider TEXT NOT NULL,
  state TEXT NOT NULL,
  payload JSONB NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS live_submission_records_attempt_created_idx
ON live_submission_records (attempt_id, created_at, submission_ref);

CREATE OR REPLACE FUNCTION enforce_live_submission_record_attempt()
RETURNS TRIGGER AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM execution_attempts
    WHERE attempt_id = NEW.attempt_id
      AND execution_mode = 'live'
  ) THEN
    RAISE EXCEPTION
      'live_submission_records requires a live execution attempt for attempt_id %',
      NEW.attempt_id;
  END IF;

  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER live_submission_records_enforce_live_attempt
BEFORE INSERT OR UPDATE ON live_submission_records
FOR EACH ROW
EXECUTE FUNCTION enforce_live_submission_record_attempt();

CREATE OR REPLACE FUNCTION prevent_live_attempt_mode_drift()
RETURNS TRIGGER AS $$
BEGIN
  IF OLD.execution_mode = 'live'
     AND NEW.execution_mode <> 'live'
     AND (
       EXISTS (
         SELECT 1
         FROM live_execution_artifacts
         WHERE attempt_id = OLD.attempt_id
       )
       OR EXISTS (
         SELECT 1
         FROM live_submission_records
         WHERE attempt_id = OLD.attempt_id
       )
     ) THEN
    RAISE EXCEPTION
      'execution_attempts with live artifacts or live submission records cannot change away from live for attempt_id %',
      OLD.attempt_id;
  END IF;

  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
