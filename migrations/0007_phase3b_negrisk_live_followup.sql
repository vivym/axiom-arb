DO $$
DECLARE
  conflicting_record RECORD;
BEGIN
  SELECT attempt_id, stream
  INTO conflicting_record
  FROM live_execution_artifacts
  GROUP BY attempt_id, stream
  HAVING COUNT(DISTINCT payload::text) > 1
  LIMIT 1;

  IF FOUND THEN
    RAISE EXCEPTION
      'live_execution_artifacts contains conflicting payloads for attempt_id %, stream %',
      conflicting_record.attempt_id,
      conflicting_record.stream;
  END IF;
END;
$$ LANGUAGE plpgsql;

DELETE FROM live_execution_artifacts older
USING live_execution_artifacts newer
WHERE older.attempt_id = newer.attempt_id
  AND older.stream = newer.stream
  AND older.payload = newer.payload
  AND (
    older.created_at < newer.created_at
    OR (
      older.created_at = newer.created_at
      AND older.artifact_id < newer.artifact_id
    )
  );

ALTER TABLE live_execution_artifacts
DROP CONSTRAINT live_execution_artifacts_pkey;

ALTER TABLE live_execution_artifacts
DROP COLUMN artifact_id;

ALTER TABLE live_execution_artifacts
ADD PRIMARY KEY (attempt_id, stream);

CREATE INDEX IF NOT EXISTS execution_attempts_live_created_idx
ON execution_attempts (created_at, attempt_id)
WHERE execution_mode = 'live';
