-- Durable scan run observability read models.
--
-- Adds append-only event/failure read models for scan timelines while leaving
-- legacy scan_state, orchestrator_jobs, and cursor tables intact.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'scan_state'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

CREATE TABLE IF NOT EXISTS scan_runs (
    id uuid PRIMARY KEY,
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    scan_state_id uuid NULL REFERENCES scan_state(id) ON DELETE SET NULL,
    source varchar(32) NOT NULL,
    status varchar(32) NOT NULL,
    correlation_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    sequence bigint NOT NULL DEFAULT 0,
    started_at timestamp with time zone NOT NULL DEFAULT NOW(),
    last_event_at timestamp with time zone NOT NULL DEFAULT NOW(),
    terminal_at timestamp with time zone,
    current_path text,
    completed_items bigint NOT NULL DEFAULT 0,
    total_items bigint NOT NULL DEFAULT 0,
    retrying_items bigint NOT NULL DEFAULT 0,
    dead_lettered_items bigint NOT NULL DEFAULT 0,
    terminal_summary jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamp with time zone NOT NULL DEFAULT NOW(),
    updated_at timestamp with time zone NOT NULL DEFAULT NOW(),
    CONSTRAINT scan_runs_source_check CHECK (source IN ('manual', 'maintenance', 'watcher', 'retry', 'orchestrator')),
    CONSTRAINT scan_runs_status_check CHECK (status IN ('pending', 'running', 'paused', 'completed', 'failed', 'canceled')),
    CONSTRAINT scan_runs_sequence_check CHECK (sequence >= 0),
    CONSTRAINT scan_runs_counter_check CHECK (
        completed_items >= 0
        AND total_items >= 0
        AND retrying_items >= 0
        AND dead_lettered_items >= 0
    )
);

COMMENT ON TABLE scan_runs IS 'Durable read model of scan/activity windows used for scan timeline observability';
COMMENT ON COLUMN scan_runs.source IS 'Origin of the scan/activity window: manual, maintenance, watcher, retry, or orchestrator';
COMMENT ON COLUMN scan_runs.scan_state_id IS 'Optional bridge to legacy scan_state; kept nullable to avoid unsafe backfill assumptions';
COMMENT ON COLUMN scan_runs.sequence IS 'Last allocated durable event sequence for this run';
COMMENT ON COLUMN scan_runs.terminal_summary IS 'Safe aggregate terminal counters and message codes; raw per-subject details live in scan_run_failures';

CREATE UNIQUE INDEX IF NOT EXISTS idx_scan_runs_correlation_id
    ON scan_runs (correlation_id);

CREATE INDEX IF NOT EXISTS idx_scan_runs_active
    ON scan_runs (library_id, status, last_event_at DESC)
    WHERE status IN ('pending', 'running', 'paused');

CREATE INDEX IF NOT EXISTS idx_scan_runs_recent
    ON scan_runs (library_id, COALESCE(terminal_at, last_event_at, started_at) DESC);

CREATE INDEX IF NOT EXISTS idx_scan_runs_terminal_retention
    ON scan_runs (terminal_at)
    WHERE terminal_at IS NOT NULL AND status NOT IN ('pending', 'running', 'paused');

CREATE TABLE IF NOT EXISTS scan_run_events (
    id uuid PRIMARY KEY,
    run_id uuid NOT NULL REFERENCES scan_runs(id) ON DELETE CASCADE,
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    event_version integer NOT NULL DEFAULT 1,
    event_kind varchar(64) NOT NULL,
    status varchar(64) NOT NULL,
    correlation_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    sequence bigint NOT NULL,
    subject_key text,
    current_path text,
    occurred_at timestamp with time zone NOT NULL DEFAULT NOW(),
    completed_items bigint NOT NULL DEFAULT 0,
    total_items bigint NOT NULL DEFAULT 0,
    retrying_items bigint NOT NULL DEFAULT 0,
    dead_lettered_items bigint NOT NULL DEFAULT 0,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamp with time zone NOT NULL DEFAULT NOW(),
    CONSTRAINT scan_run_events_sequence_check CHECK (sequence > 0),
    CONSTRAINT scan_run_events_counter_check CHECK (
        completed_items >= 0
        AND total_items >= 0
        AND retrying_items >= 0
        AND dead_lettered_items >= 0
    )
);

COMMENT ON TABLE scan_run_events IS 'Ordered durable scan timeline events, including scan progress and orchestrator job milestones';
COMMENT ON COLUMN scan_run_events.idempotency_key IS 'Source idempotency/dedupe key retained for safe replay diagnostics';
COMMENT ON COLUMN scan_run_events.payload IS 'Structured event payload; safe status payloads are separate from raw failure details';

CREATE UNIQUE INDEX IF NOT EXISTS idx_scan_run_events_run_sequence
    ON scan_run_events (run_id, sequence);

CREATE INDEX IF NOT EXISTS idx_scan_run_events_library_time
    ON scan_run_events (library_id, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_scan_run_events_correlation
    ON scan_run_events (correlation_id, sequence);

CREATE INDEX IF NOT EXISTS idx_scan_run_events_subject
    ON scan_run_events (library_id, subject_key)
    WHERE subject_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS scan_run_failures (
    id uuid PRIMARY KEY,
    run_id uuid NOT NULL REFERENCES scan_runs(id) ON DELETE CASCADE,
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    subject_key text NOT NULL,
    category varchar(64) NOT NULL,
    message_code varchar(128) NOT NULL,
    raw_debug_details jsonb NOT NULL DEFAULT '{}'::jsonb,
    last_error text,
    occurrences integer NOT NULL DEFAULT 1,
    first_seen_at timestamp with time zone NOT NULL DEFAULT NOW(),
    last_seen_at timestamp with time zone NOT NULL DEFAULT NOW(),
    retryable boolean NOT NULL DEFAULT false,
    job_id uuid,
    idempotency_key text NOT NULL,
    created_at timestamp with time zone NOT NULL DEFAULT NOW(),
    updated_at timestamp with time zone NOT NULL DEFAULT NOW(),
    CONSTRAINT scan_run_failures_occurrences_check CHECK (occurrences > 0)
);

COMMENT ON TABLE scan_run_failures IS 'Per-subject failure summaries with safe user message codes and raw debug details retained separately';
COMMENT ON COLUMN scan_run_failures.message_code IS 'Stable safe message code suitable for UI/API display';
COMMENT ON COLUMN scan_run_failures.raw_debug_details IS 'Raw/debug failure context for operator diagnostics; not intended for direct user display';

CREATE UNIQUE INDEX IF NOT EXISTS idx_scan_run_failures_subject_category
    ON scan_run_failures (run_id, subject_key, category, message_code);

CREATE INDEX IF NOT EXISTS idx_scan_run_failures_library_recent
    ON scan_run_failures (library_id, last_seen_at DESC);

CREATE INDEX IF NOT EXISTS idx_scan_run_failures_job
    ON scan_run_failures (job_id)
    WHERE job_id IS NOT NULL;
