-- Subtitle-first timed-text corpus storage.
--
-- This migration adds transcript source manifests, timestamped segment rows,
-- processing status, safe artifact references, and lexical indexes without
-- exposing local subtitle paths or raw transcript bodies through intelligence
-- artifact summaries.

CREATE SCHEMA IF NOT EXISTS ferrex;
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;
CREATE EXTENSION IF NOT EXISTS btree_gist WITH SCHEMA public;

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

-- Let transcript tables enforce that their denormalized media identity matches
-- the owning media file. The primary key on id already guarantees uniqueness;
-- the wider key exists only for composite foreign keys.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'media_files_library_file_media_identity_key'
          AND conrelid = 'media_files'::regclass
    ) THEN
        ALTER TABLE media_files
            ADD CONSTRAINT media_files_library_file_media_identity_key
            UNIQUE (library_id, id, media_id, media_type);
    END IF;
END $$;

ALTER TABLE intelligence_artifacts
    DROP CONSTRAINT IF EXISTS intelligence_artifacts_kind_check;

ALTER TABLE intelligence_artifacts
    ADD CONSTRAINT intelligence_artifacts_kind_check CHECK (
        artifact_kind::text = ANY (ARRAY[
            'summary'::varchar,
            'recommendation'::varchar,
            'search_answer'::varchar,
            'watch_plan'::varchar,
            'collection'::varchar,
            'note'::varchar,
            'analysis'::varchar,
            'index_manifest'::varchar,
            'transcript_source'::varchar,
            'transcript_segment'::varchar
        ]::text[])
    );

CREATE TABLE IF NOT EXISTS transcript_processing_status (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    library_id uuid NOT NULL,
    media_id uuid NOT NULL,
    media_type media_type NOT NULL,
    media_file_id uuid NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'pending',
    source_count integer NOT NULL DEFAULT 0,
    segment_count integer NOT NULL DEFAULT 0,
    attempt_count integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL DEFAULT 3,
    last_error_excerpt varchar(2048),
    last_run_correlation_id uuid,
    next_retry_at timestamptz,
    started_at timestamptz,
    finished_at timestamptz,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    purged_at timestamptz,
    purge_reason varchar(512),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT transcript_processing_status_media_file_fkey
        FOREIGN KEY (library_id, media_file_id, media_id, media_type)
        REFERENCES media_files(library_id, id, media_id, media_type)
        ON DELETE CASCADE,
    CONSTRAINT transcript_processing_status_media_type_check CHECK (
        media_type = ANY (ARRAY['movie'::media_type, 'episode'::media_type])
    ),
    CONSTRAINT transcript_processing_status_status_check CHECK (
        status::text = ANY (ARRAY[
            'pending'::varchar,
            'queued'::varchar,
            'running'::varchar,
            'succeeded'::varchar,
            'failed'::varchar,
            'skipped'::varchar,
            'cancelled'::varchar,
            'invalidated'::varchar,
            'purged'::varchar
        ]::text[])
    ),
    CONSTRAINT transcript_processing_status_counts_check CHECK (
        source_count >= 0 AND segment_count >= 0
    ),
    CONSTRAINT transcript_processing_status_attempts_check CHECK (
        attempt_count >= 0 AND max_attempts >= 0
    ),
    CONSTRAINT transcript_processing_status_finished_check CHECK (
        (status::text IN ('succeeded', 'failed', 'skipped', 'cancelled', 'invalidated', 'purged') AND finished_at IS NOT NULL)
        OR status::text NOT IN ('succeeded', 'failed', 'skipped', 'cancelled', 'invalidated', 'purged')
    ),
    CONSTRAINT transcript_processing_status_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT transcript_processing_status_purged_check CHECK (
        (status::text = 'purged' AND purged_at IS NOT NULL)
        OR status::text <> 'purged'
    ),
    CONSTRAINT transcript_processing_status_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    ),
    CONSTRAINT transcript_processing_status_library_file_key UNIQUE (library_id, media_file_id)
);

COMMENT ON TABLE transcript_processing_status IS
    'Per-media-file transcript extraction lifecycle status with retry, invalidation, and purge metadata.';
COMMENT ON COLUMN transcript_processing_status.last_error_excerpt IS
    'Bounded failure excerpt for operators; raw transcript text and local subtitle paths must not be stored here.';

CREATE TABLE IF NOT EXISTS transcript_sources (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    library_id uuid NOT NULL,
    media_id uuid NOT NULL,
    media_type media_type NOT NULL,
    media_file_id uuid NOT NULL,
    source_kind varchar(32) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'active',
    language_code varchar(16) NOT NULL DEFAULT 'und',
    source_key varchar(256) NOT NULL,
    source_name varchar(512),
    stream_index integer,
    source_path_hash text,
    source_content_hash text NOT NULL,
    normalized_content_hash text,
    artifact_id uuid REFERENCES intelligence_artifacts(id) ON DELETE SET NULL,
    duration_ms bigint,
    segment_count integer NOT NULL DEFAULT 0,
    extracted_at timestamptz,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    purged_at timestamptz,
    purge_reason varchar(512),
    source_locator jsonb NOT NULL DEFAULT '{}'::jsonb,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT transcript_sources_media_file_fkey
        FOREIGN KEY (library_id, media_file_id, media_id, media_type)
        REFERENCES media_files(library_id, id, media_id, media_type)
        ON DELETE CASCADE,
    CONSTRAINT transcript_sources_identity_key UNIQUE (id, library_id, media_file_id, media_id, media_type),
    CONSTRAINT transcript_sources_media_type_check CHECK (
        media_type = ANY (ARRAY['movie'::media_type, 'episode'::media_type])
    ),
    CONSTRAINT transcript_sources_kind_check CHECK (
        source_kind::text = ANY (ARRAY[
            'embedded'::varchar,
            'sidecar'::varchar,
            'manual'::varchar,
            'generated'::varchar
        ]::text[])
    ),
    CONSTRAINT transcript_sources_status_check CHECK (
        status::text = ANY (ARRAY[
            'pending'::varchar,
            'active'::varchar,
            'stale'::varchar,
            'invalidated'::varchar,
            'purged'::varchar,
            'failed'::varchar,
            'skipped'::varchar
        ]::text[])
    ),
    CONSTRAINT transcript_sources_language_check CHECK (
        language_code = 'und'
        OR language_code ~ '^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$'
    ),
    CONSTRAINT transcript_sources_source_key_check CHECK (
        length(source_key) BETWEEN 1 AND 256
    ),
    CONSTRAINT transcript_sources_stream_index_check CHECK (
        stream_index IS NULL OR stream_index >= 0
    ),
    CONSTRAINT transcript_sources_source_kind_locator_check CHECK (
        (source_kind::text = 'embedded' AND stream_index IS NOT NULL)
        OR (source_kind::text = 'sidecar' AND source_path_hash IS NOT NULL)
        OR source_kind::text IN ('manual', 'generated')
    ),
    CONSTRAINT transcript_sources_hash_check CHECK (
        source_content_hash ~ '^[0-9a-f]{64}$'
        AND (normalized_content_hash IS NULL OR normalized_content_hash ~ '^[0-9a-f]{64}$')
        AND (source_path_hash IS NULL OR source_path_hash ~ '^[0-9a-f]{64}$')
    ),
    CONSTRAINT transcript_sources_segment_count_check CHECK (segment_count >= 0),
    CONSTRAINT transcript_sources_duration_check CHECK (
        duration_ms IS NULL OR duration_ms >= 0
    ),
    CONSTRAINT transcript_sources_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT transcript_sources_purged_check CHECK (
        (status::text = 'purged' AND purged_at IS NOT NULL)
        OR status::text <> 'purged'
    ),
    CONSTRAINT transcript_sources_locator_object_check CHECK (
        jsonb_typeof(source_locator) = 'object'
    ),
    CONSTRAINT transcript_sources_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    ),
    CONSTRAINT transcript_sources_unique_source_key UNIQUE (
        library_id,
        media_file_id,
        source_kind,
        language_code,
        source_key
    )
);

COMMENT ON TABLE transcript_sources IS
    'Subtitle/transcript source manifests keyed by library, media, media file, language, and safe source locator.';
COMMENT ON COLUMN transcript_sources.source_key IS
    'Caller-provided deterministic key such as embedded stream id or sidecar path hash; must not contain local paths.';
COMMENT ON COLUMN transcript_sources.artifact_id IS
    'Optional source-level intelligence artifact id returned with snippets instead of per-segment artifact rows.';

CREATE TABLE IF NOT EXISTS transcript_segments (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    transcript_source_id uuid NOT NULL,
    library_id uuid NOT NULL,
    media_id uuid NOT NULL,
    media_type media_type NOT NULL,
    media_file_id uuid NOT NULL,
    language_code varchar(16) NOT NULL DEFAULT 'und',
    cue_index integer NOT NULL,
    start_ms bigint NOT NULL,
    end_ms bigint NOT NULL,
    cue_text varchar(4000) NOT NULL,
    segment_hash text NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'active',
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    purged_at timestamptz,
    purge_reason varchar(512),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    search_vector tsvector GENERATED ALWAYS AS (
        to_tsvector('simple'::regconfig, coalesce(cue_text, ''))
    ) STORED,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT transcript_segments_source_identity_fkey
        FOREIGN KEY (transcript_source_id, library_id, media_file_id, media_id, media_type)
        REFERENCES transcript_sources(id, library_id, media_file_id, media_id, media_type)
        ON DELETE CASCADE,
    CONSTRAINT transcript_segments_media_type_check CHECK (
        media_type = ANY (ARRAY['movie'::media_type, 'episode'::media_type])
    ),
    CONSTRAINT transcript_segments_language_check CHECK (
        language_code = 'und'
        OR language_code ~ '^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$'
    ),
    CONSTRAINT transcript_segments_status_check CHECK (
        status::text = ANY (ARRAY[
            'active'::varchar,
            'invalidated'::varchar,
            'purged'::varchar
        ]::text[])
    ),
    CONSTRAINT transcript_segments_order_check CHECK (
        cue_index >= 0 AND start_ms >= 0 AND end_ms > start_ms
    ),
    CONSTRAINT transcript_segments_text_check CHECK (
        length(cue_text) BETWEEN 1 AND 4000
    ),
    CONSTRAINT transcript_segments_hash_check CHECK (
        segment_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT transcript_segments_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT transcript_segments_purged_check CHECK (
        (status::text = 'purged' AND purged_at IS NOT NULL)
        OR status::text <> 'purged'
    ),
    CONSTRAINT transcript_segments_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    ),
    CONSTRAINT transcript_segments_source_cue_key UNIQUE (transcript_source_id, cue_index),
    CONSTRAINT transcript_segments_no_active_overlap EXCLUDE USING gist (
        transcript_source_id WITH =,
        int8range(start_ms, end_ms, '[)') WITH &&
    ) WHERE (status::text = 'active')
);

COMMENT ON TABLE transcript_segments IS
    'Timestamped subtitle cues with bounded redacted text, deterministic hashes, FTS vectors, and per-source non-overlap validation.';
COMMENT ON COLUMN transcript_segments.cue_text IS
    'Redacted, bounded cue text used for lexical search; APIs must return bounded snippets rather than full source bodies.';

CREATE INDEX IF NOT EXISTS idx_transcript_processing_status_library_status
    ON transcript_processing_status USING btree (library_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_transcript_processing_status_media
    ON transcript_processing_status USING btree (library_id, media_type, media_id, media_file_id);

CREATE INDEX IF NOT EXISTS idx_transcript_sources_media_active
    ON transcript_sources USING btree (library_id, media_type, media_id, language_code, source_kind, updated_at DESC)
    WHERE status::text = 'active' AND invalidated_at IS NULL AND purged_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_transcript_sources_file
    ON transcript_sources USING btree (library_id, media_file_id, source_kind, language_code);

CREATE INDEX IF NOT EXISTS idx_transcript_sources_artifact
    ON transcript_sources USING btree (artifact_id)
    WHERE artifact_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_transcript_sources_invalidation
    ON transcript_sources USING btree (library_id, status, updated_at DESC)
    WHERE invalidated_at IS NULL AND purged_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_transcript_segments_media_time
    ON transcript_segments USING btree (library_id, media_type, media_id, start_ms, end_ms)
    WHERE status::text = 'active' AND invalidated_at IS NULL AND purged_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_transcript_segments_source_order
    ON transcript_segments USING btree (transcript_source_id, cue_index, start_ms);

CREATE INDEX IF NOT EXISTS idx_transcript_segments_fts
    ON transcript_segments USING gin (search_vector)
    WHERE status::text = 'active' AND invalidated_at IS NULL AND purged_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_transcript_segments_text_trgm
    ON transcript_segments USING gin ((cue_text::text) public.gin_trgm_ops)
    WHERE status::text = 'active' AND invalidated_at IS NULL AND purged_at IS NULL;

ALTER TABLE intelligence_artifact_sources
    ADD COLUMN IF NOT EXISTS source_transcript_source_id uuid REFERENCES transcript_sources(id) ON DELETE SET NULL;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_kind_check;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_kind_check CHECK (
        source_kind::text = ANY (ARRAY[
            'media'::varchar,
            'media_context'::varchar,
            'search_document'::varchar,
            'artifact'::varchar,
            'run'::varchar,
            'tool_call'::varchar,
            'manual'::varchar,
            'transcript_source'::varchar
        ]::text[])
    );

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_reference_check;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_reference_check CHECK (
        (
            source_kind::text = 'media'
            AND source_library_id IS NOT NULL
            AND source_media_id IS NOT NULL
            AND source_media_type IS NOT NULL
        )
        OR (source_kind::text = 'media_context' AND source_media_context_id IS NOT NULL)
        OR (source_kind::text = 'search_document' AND source_search_document_id IS NOT NULL)
        OR (source_kind::text = 'artifact' AND source_artifact_id IS NOT NULL)
        OR (source_kind::text = 'run' AND source_run_id IS NOT NULL)
        OR (source_kind::text = 'tool_call' AND source_tool_call_id IS NOT NULL)
        OR (source_kind::text = 'transcript_source' AND source_transcript_source_id IS NOT NULL)
        OR source_kind::text = 'manual'
    );

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_transcript_source
    ON intelligence_artifact_sources USING btree (source_transcript_source_id)
    WHERE source_transcript_source_id IS NOT NULL;

DO $$
DECLARE
    table_name text;
    trigger_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'transcript_processing_status',
        'transcript_sources',
        'transcript_segments'
    ]
    LOOP
        trigger_name := 'update_' || table_name || '_updated_at';

        IF NOT EXISTS (
            SELECT 1
            FROM pg_trigger
            WHERE tgname = trigger_name
              AND tgrelid = to_regclass(table_name)
        ) THEN
            EXECUTE format(
                'CREATE TRIGGER %I BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION update_updated_at_column()',
                trigger_name,
                table_name
            );
        END IF;
    END LOOP;
END $$;
