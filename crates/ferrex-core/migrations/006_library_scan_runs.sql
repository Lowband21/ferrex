-- Durable library scan run contract and restart-safe job correlation hints.
--
-- A previous `scan_state` table exists in the baseline schema, but the current
-- scan runtime does not use it and its `scan_type` / `status` vocabulary models
-- an older resumability concept. This migration intentionally creates an
-- explicit `library_scan_runs` table so the new public contract can track
-- library+mode single-flight identity, run_key/correlation identity, progress,
-- and terminal state without overloading legacy semantics.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'libraries'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

ALTER TABLE orchestrator_jobs
    ADD COLUMN IF NOT EXISTS correlation_id uuid;

COMMENT ON COLUMN orchestrator_jobs.correlation_id IS
    'Optional scan/job correlation persisted from EnqueueRequest so queued work can continue publishing the original correlation after process restart.';

CREATE INDEX IF NOT EXISTS idx_jobs_correlation_id
    ON orchestrator_jobs USING btree (correlation_id)
    WHERE correlation_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS library_scan_runs (
    scan_id uuid PRIMARY KEY DEFAULT uuidv7(),
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    mode varchar(32) NOT NULL,
    run_key text GENERATED ALWAYS AS (
        'library:' || library_id::text || ':mode:' || mode::text
    ) STORED,
    correlation_id uuid NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'pending',
    completed_items bigint NOT NULL DEFAULT 0,
    total_items bigint NOT NULL DEFAULT 0,
    retrying_items bigint NOT NULL DEFAULT 0,
    dead_lettered_items bigint NOT NULL DEFAULT 0,
    current_path text,
    last_error text,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    sequence bigint NOT NULL DEFAULT 0,
    started_at timestamptz NOT NULL DEFAULT now(),
    terminal_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT library_scan_runs_mode_check CHECK (
        mode::text = ANY (ARRAY[
            'manual'::varchar,
            'maintenance'::varchar,
            'resume'::varchar
        ]::text[])
    ),
    CONSTRAINT library_scan_runs_status_check CHECK (
        status::text = ANY (ARRAY[
            'pending'::varchar,
            'running'::varchar,
            'paused'::varchar,
            'completed'::varchar,
            'failed'::varchar,
            'canceled'::varchar
        ]::text[])
    ),
    CONSTRAINT library_scan_runs_counters_check CHECK (
        completed_items >= 0
        AND total_items >= 0
        AND retrying_items >= 0
        AND dead_lettered_items >= 0
        AND sequence >= 0
    ),
    CONSTRAINT library_scan_runs_terminal_at_check CHECK (
        (status::text IN ('completed','failed','canceled') AND terminal_at IS NOT NULL)
        OR (status::text NOT IN ('completed','failed','canceled'))
    )
);

COMMENT ON TABLE library_scan_runs IS
    'Durable public library scan run contract keyed by library+mode. Kept separate from legacy scan_state because scan_state is not wired into the current scan runtime and uses older scan_type/status semantics.';
COMMENT ON COLUMN library_scan_runs.mode IS
    'Public scan run mode. manual preserves the existing user-triggered bulk library scan behavior.';
COMMENT ON COLUMN library_scan_runs.run_key IS
    'Deterministic idempotency key generated from library_id and mode.';
COMMENT ON COLUMN library_scan_runs.status IS
    'Lifecycle status. pending/running/paused are active and constrained to one row per run_key.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_library_scan_runs_active_run_key
    ON library_scan_runs USING btree (run_key)
    WHERE status::text IN ('pending','running','paused');

CREATE INDEX IF NOT EXISTS idx_library_scan_runs_library_mode_started
    ON library_scan_runs USING btree (library_id, mode, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_library_scan_runs_status_updated
    ON library_scan_runs USING btree (status, updated_at DESC);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgname = 'update_library_scan_runs_updated_at'
          AND tgrelid = 'library_scan_runs'::regclass
    ) THEN
        CREATE TRIGGER update_library_scan_runs_updated_at
            BEFORE UPDATE ON library_scan_runs
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;
