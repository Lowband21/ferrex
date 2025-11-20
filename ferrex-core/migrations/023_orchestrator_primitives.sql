-- Migration 023: Orchestrator primitives (scan cursors and durable job queue)
-- This migration adds:
-- 1) scan_cursors: persistent state for incremental scanning (stores folder_path_norm for reference; uniqueness via (library_id, path_hash))
-- 2) orchestrator_jobs: durable queue storage for scan/analyze/metadata/index pipelines

-- 1) Scan cursors
-- Rationale:
-- - Use (library_id, path_hash) as the primary key (aligns with core::orchestration::scan_cursor::ScanCursorId)
-- - Keep folder_path_norm as a reference string; not used for uniqueness
-- - Multiple roots per library => multiple scan cursors per library; if roots change, cursors can be migrated/removed

CREATE TABLE IF NOT EXISTS scan_cursors (
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path_hash BIGINT NOT NULL,
    folder_path_norm TEXT NOT NULL,
    listing_hash TEXT NOT NULL,
    entry_count INTEGER NOT NULL DEFAULT 0,
    last_scan_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_modified_at TIMESTAMPTZ,
    device_id TEXT,
    PRIMARY KEY (library_id, path_hash)
);

CREATE INDEX IF NOT EXISTS idx_scan_cursors_staleness
    ON scan_cursors(library_id, last_scan_at DESC);

COMMENT ON TABLE scan_cursors IS 'Persistent scan cursor per (library, folder) for incremental scanning';
COMMENT ON COLUMN scan_cursors.path_hash IS 'Deterministic hash of normalized path(s) (see ScanCursorId) used as part of the key';
COMMENT ON COLUMN scan_cursors.folder_path_norm IS 'Normalized human-readable folder path for reference only (not unique)';
COMMENT ON COLUMN scan_cursors.listing_hash IS 'Hash of directory listing (entries + mtimes) to detect changes';
COMMENT ON COLUMN scan_cursors.entry_count IS 'Number of entries included when listing_hash was computed';

-- 2) Durable job queue (orchestrator_jobs)
-- Notes:
-- - Domain model: ferrex-core/src/orchestration/job.rs
-- - kind: 'scan' (FolderScan), 'analyze' (MediaAnalyze), 'metadata' (MetadataEnrich), 'index' (IndexUpsert)
-- - state: 'ready','deferred','leased','completed','failed','dead_letter'
-- - priority (0..3) = P0..P3
-- - available_at controls eligibility for dequeue (also used for backoff)
-- - Partial unique on dedupe_key for active states supports coalescing

CREATE TABLE IF NOT EXISTS orchestrator_jobs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    library_id UUID NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind VARCHAR(20) NOT NULL CHECK (kind IN ('scan','analyze','metadata','index')),
    payload JSONB NOT NULL,
    priority SMALLINT NOT NULL CHECK (priority BETWEEN 0 AND 3),
    state VARCHAR(20) NOT NULL CHECK (state IN ('ready','deferred','leased','completed','failed','dead_letter')),
    attempts INT NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_owner TEXT,
    lease_id UUID,
    lease_expires_at TIMESTAMPTZ,
    dedupe_key TEXT NOT NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast dequeue path by queue / priority / time
CREATE INDEX IF NOT EXISTS idx_jobs_ready_dequeue
    ON orchestrator_jobs(kind, priority, available_at, created_at)
    WHERE state = 'ready';

-- Optional: ready selection scoped by library (supports fairness)
CREATE INDEX IF NOT EXISTS idx_jobs_ready_by_library
    ON orchestrator_jobs(library_id, priority, available_at, created_at)
    WHERE state = 'ready';

-- Partial unique dedupe for active/pending jobs
CREATE UNIQUE INDEX IF NOT EXISTS uq_jobs_dedupe_active
    ON orchestrator_jobs(dedupe_key)
    WHERE state IN ('ready','deferred','leased');

-- Lease expiry scanning
CREATE INDEX IF NOT EXISTS idx_jobs_lease_expiry
    ON orchestrator_jobs(lease_expires_at)
    WHERE state = 'leased';

-- Active lease uniqueness
CREATE UNIQUE INDEX IF NOT EXISTS uq_jobs_lease_id_active
    ON orchestrator_jobs(lease_id)
    WHERE state = 'leased' AND lease_id IS NOT NULL;

-- Operational visibility
CREATE INDEX IF NOT EXISTS idx_jobs_state_kind
    ON orchestrator_jobs(state, kind);

-- Maintain updated_at timestamps
CREATE TRIGGER update_orchestrator_jobs_updated_at
    BEFORE UPDATE ON orchestrator_jobs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
