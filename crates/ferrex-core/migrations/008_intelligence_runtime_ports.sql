-- Phase 2 runtime storage ports for draft artifact replay and run events.
--
-- This remains provider/runtime-neutral: no provider calls are made by the
-- migration and the table only stores bounded replay metadata.

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

CREATE TABLE IF NOT EXISTS intelligence_run_events (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL REFERENCES intelligence_runs(id) ON DELETE CASCADE,
    sequence integer NOT NULL,
    event_kind varchar(48) NOT NULL,
    status varchar(32),
    tool_call_id uuid REFERENCES intelligence_tool_calls(id) ON DELETE SET NULL,
    artifact_id uuid REFERENCES intelligence_artifacts(id) ON DELETE SET NULL,
    message varchar(2048),
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    error_code varchar(64),
    error jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_run_events_run_sequence_key UNIQUE (run_id, sequence),
    CONSTRAINT intelligence_run_events_sequence_check CHECK (sequence >= 0),
    CONSTRAINT intelligence_run_events_kind_check CHECK (
        event_kind::text = ANY (ARRAY[
            'queued'::varchar,
            'started'::varchar,
            'status_changed'::varchar,
            'model_token'::varchar,
            'tool_call_started'::varchar,
            'tool_call_finished'::varchar,
            'draft_artifact_created'::varchar,
            'draft_artifact_updated'::varchar,
            'cancel_requested'::varchar,
            'cancelled'::varchar,
            'completed'::varchar,
            'failed'::varchar,
            'heartbeat'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_run_events_status_check CHECK (
        status IS NULL OR status::text = ANY (ARRAY[
            'queued'::varchar,
            'running'::varchar,
            'succeeded'::varchar,
            'failed'::varchar,
            'cancelled'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_run_events_payload_object_check CHECK (
        jsonb_typeof(payload) = 'object'
    ),
    CONSTRAINT intelligence_run_events_error_object_check CHECK (
        error IS NULL OR jsonb_typeof(error) = 'object'
    )
);

COMMENT ON TABLE intelligence_run_events IS
    'Ordered, durable runtime event log for future intelligence run replay and streaming surfaces.';
COMMENT ON COLUMN intelligence_run_events.sequence IS
    'Monotonic per-run event sequence used for idempotent replay after reconnects.';
COMMENT ON COLUMN intelligence_run_events.payload IS
    'Bounded structured event payload; large model/tool outputs remain capped by runtime config.';

CREATE INDEX IF NOT EXISTS idx_intelligence_run_events_run_sequence
    ON intelligence_run_events USING btree (run_id, sequence);

CREATE INDEX IF NOT EXISTS idx_intelligence_run_events_kind_created
    ON intelligence_run_events USING btree (event_kind, created_at DESC);
