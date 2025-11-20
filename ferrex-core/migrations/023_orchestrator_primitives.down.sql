-- Down migration 023: Drop orchestrator primitives

-- Drop trigger first (if exists)
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'update_orchestrator_jobs_updated_at'
    ) THEN
        DROP TRIGGER update_orchestrator_jobs_updated_at ON orchestrator_jobs;
    END IF;
END $$;

-- Drop indexes (IF EXISTS guards allow re-runs)
DROP INDEX IF EXISTS idx_jobs_state_kind;
DROP INDEX IF EXISTS idx_jobs_backoff;
DROP INDEX IF EXISTS idx_jobs_lease_expiry;
DROP INDEX IF EXISTS uq_jobs_dedupe_active;
DROP INDEX IF EXISTS uq_jobs_lease_id_active;
DROP INDEX IF EXISTS idx_jobs_ready_by_library;
DROP INDEX IF EXISTS idx_jobs_ready_dequeue;

-- Drop tables
DROP TABLE IF EXISTS orchestrator_jobs;
DROP INDEX IF EXISTS idx_scan_cursors_staleness;
DROP TABLE IF EXISTS scan_cursors;