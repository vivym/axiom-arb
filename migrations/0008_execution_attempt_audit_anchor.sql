ALTER TABLE execution_attempts
ADD COLUMN route TEXT NOT NULL DEFAULT 'unknown',
ADD COLUMN scope TEXT NOT NULL DEFAULT 'legacy',
ADD COLUMN matched_rule_id TEXT;

WITH legacy_negrisk_attempts AS (
  SELECT
    attempt_id,
    COALESCE(
      (regexp_match(plan_id, '^request-bound:[0-9]+:.*:(negrisk-submit-family:.*)$'))[1],
      (regexp_match(plan_id, '^(negrisk-submit-family:.*)$'))[1]
    ) AS legacy_plan_id
  FROM execution_attempts
  WHERE route = 'unknown'
    AND scope = 'legacy'
)
UPDATE execution_attempts attempts
SET route = 'neg-risk',
    scope = split_part(legacy_negrisk_attempts.legacy_plan_id, ':', 2)
FROM legacy_negrisk_attempts
WHERE attempts.attempt_id = legacy_negrisk_attempts.attempt_id
  AND legacy_negrisk_attempts.legacy_plan_id IS NOT NULL;
