-- Add persisted transcript extraction jobs to the scan orchestrator queue.
-- The migration keeps the application schema selection pattern used by later
-- migrations so upgraded databases resolve unqualified names consistently.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'orchestrator_jobs'
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

ALTER TABLE orchestrator_jobs
    DROP CONSTRAINT IF EXISTS orchestrator_jobs_kind_check;

ALTER TABLE orchestrator_jobs
    ADD CONSTRAINT orchestrator_jobs_kind_check CHECK (kind >= 0 AND kind <= 7);

CREATE INDEX IF NOT EXISTS idx_jobs_transcript_ready
    ON orchestrator_jobs USING btree (library_id, priority, available_at, created_at)
    WHERE state = 'ready' AND kind = 7;

COMMENT ON CONSTRAINT orchestrator_jobs_kind_check ON orchestrator_jobs IS
    '0 folder_scan, 1 series_resolve, 2 media_analyze, 3 metadata_enrich, 4 index_upsert, 5 image_fetch, 6 episode_match, 7 transcript_extract';
