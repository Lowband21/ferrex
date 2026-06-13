-- Durable watcher payloads and idempotent replay support.
--
-- This migration upgrades the legacy file_watch_events queue into the
-- normalized event log consumed by FsWatchService. It is intentionally
-- idempotent because early development databases may already include some of
-- these columns from regenerated full-schema snapshots.

CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'file_watch_events'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

ALTER TABLE file_watch_events
    ADD COLUMN IF NOT EXISTS event_version integer DEFAULT 1 NOT NULL,
    ADD COLUMN IF NOT EXISTS library_root_id integer DEFAULT 0 NOT NULL,
    ADD COLUMN IF NOT EXISTS root_path text,
    ADD COLUMN IF NOT EXISTS path_key text,
    ADD COLUMN IF NOT EXISTS fingerprint text,
    ADD COLUMN IF NOT EXISTS file_modified_at timestamp with time zone,
    ADD COLUMN IF NOT EXISTS correlation_id uuid,
    ADD COLUMN IF NOT EXISTS idempotency_key text;

UPDATE file_watch_events
SET root_path = COALESCE(root_path, '')
WHERE root_path IS NULL;

UPDATE file_watch_events
SET path_key = COALESCE(path_key, file_path)
WHERE path_key IS NULL;

UPDATE file_watch_events
SET idempotency_key = COALESCE(idempotency_key, id::text)
WHERE idempotency_key IS NULL;

ALTER TABLE file_watch_events
    ALTER COLUMN root_path SET NOT NULL,
    ALTER COLUMN path_key SET NOT NULL,
    ALTER COLUMN idempotency_key SET NOT NULL;

ALTER TABLE file_watch_events
    DROP CONSTRAINT IF EXISTS file_watch_events_event_type_check,
    DROP CONSTRAINT IF EXISTS valid_move_event,
    DROP CONSTRAINT IF EXISTS file_watch_events_library_root_id_check;

ALTER TABLE file_watch_events
    ADD CONSTRAINT file_watch_events_event_type_check
        CHECK (event_type::text = ANY (ARRAY[
            'created'::varchar,
            'modified'::varchar,
            'deleted'::varchar,
            'moved'::varchar,
            'overflow'::varchar
        ]::text[])),
    ADD CONSTRAINT file_watch_events_library_root_id_check
        CHECK (library_root_id >= 0);

CREATE UNIQUE INDEX IF NOT EXISTS idx_file_watch_events_idempotency_key
    ON file_watch_events USING btree (idempotency_key);

CREATE INDEX IF NOT EXISTS idx_file_watch_events_path_key
    ON file_watch_events USING btree (path_key);

DROP INDEX IF EXISTS idx_file_watch_events_unprocessed;
CREATE INDEX idx_file_watch_events_unprocessed
    ON file_watch_events USING btree (library_id, detected_at ASC, id ASC)
    WHERE processed = false;

CREATE INDEX IF NOT EXISTS idx_fwe_library_root_detected
    ON file_watch_events USING btree (library_id, library_root_id, detected_at ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_fwe_library_detected
    ON file_watch_events USING btree (library_id, detected_at ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_fwe_event_type
    ON file_watch_events USING btree (event_type);

CREATE TABLE IF NOT EXISTS file_watch_consumer_offsets (
    group_name text NOT NULL,
    library_id uuid NOT NULL,
    last_event_id uuid NULL,
    last_detected_at timestamptz NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT file_watch_consumer_offsets_pkey PRIMARY KEY (group_name, library_id),
    CONSTRAINT fk_fwco_library FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT fk_fwco_last_event FOREIGN KEY (last_event_id) REFERENCES file_watch_events(id) ON DELETE SET NULL
);

COMMENT ON TABLE file_watch_consumer_offsets IS 'Durable per-group, per-library offsets for file change event streaming';
COMMENT ON COLUMN file_watch_consumer_offsets.group_name IS 'Consumer group name (logical subscriber id)';
COMMENT ON COLUMN file_watch_consumer_offsets.last_event_id IS 'Last acknowledged event id for this group and library';
COMMENT ON COLUMN file_watch_consumer_offsets.last_detected_at IS 'Detected-at timestamp of the last acknowledged event';
