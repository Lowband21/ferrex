-- Enroll durable queue jobs into every scan run that owns their outcome.
--
-- A job can be shared by multiple runs through dedupe merging, so correlation
-- on the job row itself is not sufficient ownership state. This many-to-many
-- table makes accepted and merged enqueue outcomes independently durable.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'orchestrator_jobs'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

CREATE TABLE IF NOT EXISTS orchestrator_job_enrollments (
    correlation_id uuid NOT NULL,
    job_id uuid NOT NULL
        REFERENCES orchestrator_jobs(id)
        ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (correlation_id, job_id)
);

CREATE INDEX IF NOT EXISTS idx_job_enrollments_job_id
    ON orchestrator_job_enrollments USING btree (job_id);

INSERT INTO orchestrator_job_enrollments (correlation_id, job_id)
SELECT correlation_id, id
FROM orchestrator_jobs
WHERE correlation_id IS NOT NULL
ON CONFLICT (correlation_id, job_id) DO NOTHING;

COMMENT ON TABLE orchestrator_job_enrollments IS
    'Authoritative many-to-many ownership between scan correlations and accepted or merged orchestrator jobs.';
