-- Agent-safe intelligence read models and durable artifact/run audit skeletons.
--
-- This intentionally stores bounded text excerpts and metadata/provenance only.
-- Embedding vectors and transcript segment persistence are deferred to later phases.

CREATE SCHEMA IF NOT EXISTS ferrex;
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;

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

CREATE TABLE IF NOT EXISTS intelligence_media_context (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    media_id uuid NOT NULL,
    media_type media_type NOT NULL,
    file_id uuid REFERENCES media_files(id) ON DELETE SET NULL,
    context_kind varchar(32) NOT NULL DEFAULT 'metadata',
    status varchar(32) NOT NULL DEFAULT 'active',
    title varchar(512) NOT NULL,
    sort_title varchar(512),
    summary varchar(4096),
    excerpt varchar(2048),
    release_date date,
    runtime_seconds integer,
    source_system varchar(64) NOT NULL DEFAULT 'ferrex',
    source_revision bigint NOT NULL DEFAULT 0,
    source_updated_at timestamptz,
    content_hash text NOT NULL,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_media_context_media_type_check CHECK (
        media_type = ANY (ARRAY[
            'movie'::media_type,
            'series'::media_type,
            'season'::media_type,
            'episode'::media_type
        ])
    ),
    CONSTRAINT intelligence_media_context_kind_check CHECK (
        context_kind::text = ANY (ARRAY[
            'metadata'::varchar,
            'technical'::varchar,
            'watch_state'::varchar,
            'availability'::varchar,
            'combined'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_media_context_status_check CHECK (
        status::text = ANY (ARRAY[
            'active'::varchar,
            'stale'::varchar,
            'invalidated'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_media_context_hash_check CHECK (
        content_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_media_context_revision_check CHECK (source_revision >= 0),
    CONSTRAINT intelligence_media_context_runtime_check CHECK (
        runtime_seconds IS NULL OR runtime_seconds >= 0
    ),
    CONSTRAINT intelligence_media_context_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT intelligence_media_context_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    )
);

COMMENT ON TABLE intelligence_media_context IS
    'Bounded per-media intelligence read model used as safe LLM context; vector and transcript segment storage are deferred.';
COMMENT ON COLUMN intelligence_media_context.user_id IS
    'Optional user owner for user-specific context such as watch-state summaries; NULL means global/library context.';
COMMENT ON COLUMN intelligence_media_context.content_hash IS
    'SHA-256 hex digest of the bounded context fields and structured metadata used to detect stale read models.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_media_context_global
    ON intelligence_media_context USING btree (library_id, media_type, media_id, context_kind)
    WHERE user_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_media_context_user
    ON intelligence_media_context USING btree (library_id, user_id, media_type, media_id, context_kind)
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_media_context_lookup
    ON intelligence_media_context USING btree (library_id, media_type, media_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_intelligence_media_context_invalidation
    ON intelligence_media_context USING btree (library_id, source_system, source_revision, updated_at)
    WHERE invalidated_at IS NULL;

CREATE TABLE IF NOT EXISTS intelligence_search_documents (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    library_id uuid NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    media_context_id uuid REFERENCES intelligence_media_context(id) ON DELETE SET NULL,
    media_id uuid NOT NULL,
    media_type media_type NOT NULL,
    document_kind varchar(32) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'active',
    title varchar(512) NOT NULL,
    summary varchar(4096),
    search_excerpt varchar(2048),
    search_text varchar(16000) NOT NULL,
    language varchar(16) NOT NULL DEFAULT 'simple',
    source_system varchar(64) NOT NULL DEFAULT 'ferrex',
    source_revision bigint NOT NULL DEFAULT 0,
    source_updated_at timestamptz,
    content_hash text NOT NULL,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    search_vector tsvector GENERATED ALWAYS AS (
        to_tsvector(
            'simple'::regconfig,
            coalesce(title, '') || ' ' ||
            coalesce(summary, '') || ' ' ||
            coalesce(search_excerpt, '') || ' ' ||
            coalesce(search_text, '')
        )
    ) STORED,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_search_documents_media_type_check CHECK (
        media_type = ANY (ARRAY[
            'movie'::media_type,
            'series'::media_type,
            'season'::media_type,
            'episode'::media_type
        ])
    ),
    CONSTRAINT intelligence_search_documents_kind_check CHECK (
        document_kind::text = ANY (ARRAY[
            'title'::varchar,
            'overview'::varchar,
            'credits'::varchar,
            'technical'::varchar,
            'watch_state'::varchar,
            'artifact'::varchar,
            'combined'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_search_documents_status_check CHECK (
        status::text = ANY (ARRAY[
            'active'::varchar,
            'stale'::varchar,
            'invalidated'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_search_documents_hash_check CHECK (
        content_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_search_documents_revision_check CHECK (source_revision >= 0),
    CONSTRAINT intelligence_search_documents_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT intelligence_search_documents_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    )
);

COMMENT ON TABLE intelligence_search_documents IS
    'Library-scoped FTS/trigram search read model for bounded media context; pgvector is intentionally not required.';
COMMENT ON COLUMN intelligence_search_documents.search_text IS
    'Bounded aggregate text used for lexical search until embedding/vector storage is introduced.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_search_documents_global
    ON intelligence_search_documents USING btree (library_id, media_type, media_id, document_kind)
    WHERE user_id IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_search_documents_user
    ON intelligence_search_documents USING btree (library_id, user_id, media_type, media_id, document_kind)
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_library_active
    ON intelligence_search_documents USING btree (library_id, document_kind, media_type, updated_at DESC)
    WHERE status::text = 'active' AND invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_media_lookup
    ON intelligence_search_documents USING btree (library_id, media_type, media_id, document_kind);

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_fts
    ON intelligence_search_documents USING gin (search_vector)
    WHERE status::text = 'active' AND invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_title_trgm
    ON intelligence_search_documents USING gin ((title::text) public.gin_trgm_ops)
    WHERE status::text = 'active' AND invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_excerpt_trgm
    ON intelligence_search_documents USING gin ((search_excerpt::text) public.gin_trgm_ops)
    WHERE search_excerpt IS NOT NULL
      AND status::text = 'active'
      AND invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_search_documents_invalidation
    ON intelligence_search_documents USING btree (library_id, source_system, source_revision, updated_at)
    WHERE invalidated_at IS NULL;

CREATE TABLE IF NOT EXISTS intelligence_runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_kind varchar(32) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'queued',
    library_id uuid REFERENCES libraries(id) ON DELETE SET NULL,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    media_id uuid,
    media_type media_type,
    correlation_id uuid NOT NULL DEFAULT uuidv7(),
    idempotency_key text,
    provider_name varchar(128),
    model_name varchar(128),
    request_hash text,
    prompt_excerpt varchar(2048),
    result_summary varchar(4096),
    error_excerpt varchar(2048),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    started_at timestamptz,
    finished_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_runs_kind_check CHECK (
        run_kind::text = ANY (ARRAY[
            'index'::varchar,
            'search'::varchar,
            'summarize'::varchar,
            'recommend'::varchar,
            'answer'::varchar,
            'maintenance'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_runs_status_check CHECK (
        status::text = ANY (ARRAY[
            'queued'::varchar,
            'running'::varchar,
            'succeeded'::varchar,
            'failed'::varchar,
            'cancelled'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_runs_media_pair_check CHECK (
        (media_id IS NULL AND media_type IS NULL)
        OR (media_id IS NOT NULL AND media_type IS NOT NULL)
    ),
    CONSTRAINT intelligence_runs_media_type_check CHECK (
        media_type IS NULL OR media_type = ANY (ARRAY[
            'movie'::media_type,
            'series'::media_type,
            'season'::media_type,
            'episode'::media_type
        ])
    ),
    CONSTRAINT intelligence_runs_request_hash_check CHECK (
        request_hash IS NULL OR request_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_runs_finished_at_check CHECK (
        (status::text IN ('succeeded', 'failed', 'cancelled') AND finished_at IS NOT NULL)
        OR status::text NOT IN ('succeeded', 'failed', 'cancelled')
    ),
    CONSTRAINT intelligence_runs_time_order_check CHECK (
        finished_at IS NULL OR started_at IS NULL OR finished_at >= started_at
    ),
    CONSTRAINT intelligence_runs_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    )
);

COMMENT ON TABLE intelligence_runs IS
    'Durable audit skeleton for future intelligence jobs; does not imply provider or tool-loop execution support.';
COMMENT ON COLUMN intelligence_runs.prompt_excerpt IS
    'Bounded prompt/request excerpt for audit and debugging without persisting unbounded LLM inputs.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_runs_idempotency_key
    ON intelligence_runs USING btree (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_runs_status_updated
    ON intelligence_runs USING btree (status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_intelligence_runs_library_kind_created
    ON intelligence_runs USING btree (library_id, run_kind, created_at DESC)
    WHERE library_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_runs_user_created
    ON intelligence_runs USING btree (user_id, created_at DESC)
    WHERE user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS intelligence_tool_calls (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    run_id uuid NOT NULL REFERENCES intelligence_runs(id) ON DELETE CASCADE,
    sequence integer NOT NULL,
    tool_kind varchar(32) NOT NULL,
    tool_name varchar(128) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'queued',
    idempotency_key text,
    input_hash text,
    output_hash text,
    arguments jsonb NOT NULL DEFAULT '{}'::jsonb,
    result jsonb,
    error_excerpt varchar(2048),
    started_at timestamptz,
    finished_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_tool_calls_run_sequence_key UNIQUE (run_id, sequence),
    CONSTRAINT intelligence_tool_calls_sequence_check CHECK (sequence >= 0),
    CONSTRAINT intelligence_tool_calls_kind_check CHECK (
        tool_kind::text = ANY (ARRAY[
            'search'::varchar,
            'read_model'::varchar,
            'artifact'::varchar,
            'external'::varchar,
            'system'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_tool_calls_status_check CHECK (
        status::text = ANY (ARRAY[
            'queued'::varchar,
            'running'::varchar,
            'succeeded'::varchar,
            'failed'::varchar,
            'skipped'::varchar,
            'cancelled'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_tool_calls_input_hash_check CHECK (
        input_hash IS NULL OR input_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_tool_calls_output_hash_check CHECK (
        output_hash IS NULL OR output_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_tool_calls_finished_at_check CHECK (
        (status::text IN ('succeeded', 'failed', 'skipped', 'cancelled') AND finished_at IS NOT NULL)
        OR status::text NOT IN ('succeeded', 'failed', 'skipped', 'cancelled')
    ),
    CONSTRAINT intelligence_tool_calls_time_order_check CHECK (
        finished_at IS NULL OR started_at IS NULL OR finished_at >= started_at
    ),
    CONSTRAINT intelligence_tool_calls_arguments_object_check CHECK (
        jsonb_typeof(arguments) = 'object'
    )
);

COMMENT ON TABLE intelligence_tool_calls IS
    'Durable per-run tool-call audit records without any provider/tool execution wiring.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_intelligence_tool_calls_idempotency_key
    ON intelligence_tool_calls USING btree (run_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_tool_calls_run_sequence
    ON intelligence_tool_calls USING btree (run_id, sequence);

CREATE INDEX IF NOT EXISTS idx_intelligence_tool_calls_status_updated
    ON intelligence_tool_calls USING btree (status, updated_at DESC);

CREATE TABLE IF NOT EXISTS intelligence_artifacts (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    artifact_kind varchar(32) NOT NULL,
    scope varchar(16) NOT NULL DEFAULT 'global',
    status varchar(32) NOT NULL DEFAULT 'draft',
    library_id uuid REFERENCES libraries(id) ON DELETE CASCADE,
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    media_id uuid,
    media_type media_type,
    run_id uuid REFERENCES intelligence_runs(id) ON DELETE SET NULL,
    supersedes_artifact_id uuid REFERENCES intelligence_artifacts(id) ON DELETE SET NULL,
    title varchar(512) NOT NULL,
    summary varchar(4096),
    excerpt varchar(2048),
    content_hash text NOT NULL,
    content jsonb NOT NULL DEFAULT '{}'::jsonb,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    source_system varchar(64) NOT NULL DEFAULT 'ferrex',
    source_revision bigint NOT NULL DEFAULT 0,
    source_updated_at timestamptz,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT intelligence_artifacts_kind_check CHECK (
        artifact_kind::text = ANY (ARRAY[
            'summary'::varchar,
            'recommendation'::varchar,
            'search_answer'::varchar,
            'watch_plan'::varchar,
            'collection'::varchar,
            'note'::varchar,
            'analysis'::varchar,
            'index_manifest'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_artifacts_scope_check CHECK (
        (scope::text = 'global' AND user_id IS NULL)
        OR (scope::text = 'user' AND user_id IS NOT NULL)
    ),
    CONSTRAINT intelligence_artifacts_status_check CHECK (
        status::text = ANY (ARRAY[
            'draft'::varchar,
            'active'::varchar,
            'stale'::varchar,
            'superseded'::varchar,
            'invalidated'::varchar,
            'deleted'::varchar,
            'failed'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_artifacts_media_pair_check CHECK (
        (media_id IS NULL AND media_type IS NULL)
        OR (media_id IS NOT NULL AND media_type IS NOT NULL)
    ),
    CONSTRAINT intelligence_artifacts_media_type_check CHECK (
        media_type IS NULL OR media_type = ANY (ARRAY[
            'movie'::media_type,
            'series'::media_type,
            'season'::media_type,
            'episode'::media_type
        ])
    ),
    CONSTRAINT intelligence_artifacts_hash_check CHECK (
        content_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_artifacts_revision_check CHECK (source_revision >= 0),
    CONSTRAINT intelligence_artifacts_invalidated_check CHECK (
        (status::text IN ('invalidated', 'deleted') AND invalidated_at IS NOT NULL)
        OR status::text NOT IN ('invalidated', 'deleted')
    ),
    CONSTRAINT intelligence_artifacts_metadata_object_check CHECK (
        jsonb_typeof(metadata) = 'object'
    )
);

COMMENT ON TABLE intelligence_artifacts IS
    'Persistent global and user-scoped intelligence artifacts with bounded summaries, content hashes, and invalidation metadata.';
COMMENT ON COLUMN intelligence_artifacts.scope IS
    'global artifacts have NULL user_id; user artifacts must reference their owning user.';

CREATE INDEX IF NOT EXISTS idx_intelligence_artifacts_active
    ON intelligence_artifacts USING btree (library_id, artifact_kind, updated_at DESC)
    WHERE status::text = 'active' AND invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifacts_user_scoped
    ON intelligence_artifacts USING btree (user_id, artifact_kind, updated_at DESC)
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifacts_media
    ON intelligence_artifacts USING btree (library_id, media_type, media_id, artifact_kind, updated_at DESC)
    WHERE media_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifacts_invalidation
    ON intelligence_artifacts USING btree (source_system, source_revision, updated_at)
    WHERE invalidated_at IS NULL;

CREATE TABLE IF NOT EXISTS intelligence_artifact_sources (
    artifact_id uuid NOT NULL REFERENCES intelligence_artifacts(id) ON DELETE CASCADE,
    source_ordinal integer NOT NULL DEFAULT 0,
    source_kind varchar(32) NOT NULL,
    status varchar(32) NOT NULL DEFAULT 'active',
    source_media_context_id uuid REFERENCES intelligence_media_context(id) ON DELETE SET NULL,
    source_search_document_id uuid REFERENCES intelligence_search_documents(id) ON DELETE SET NULL,
    source_artifact_id uuid REFERENCES intelligence_artifacts(id) ON DELETE SET NULL,
    source_run_id uuid REFERENCES intelligence_runs(id) ON DELETE SET NULL,
    source_tool_call_id uuid REFERENCES intelligence_tool_calls(id) ON DELETE SET NULL,
    source_library_id uuid REFERENCES libraries(id) ON DELETE SET NULL,
    source_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    source_media_id uuid,
    source_media_type media_type,
    source_revision bigint NOT NULL DEFAULT 0,
    source_content_hash text,
    source_excerpt varchar(2048),
    source_locator jsonb NOT NULL DEFAULT '{}'::jsonb,
    invalidated_at timestamptz,
    invalidation_reason varchar(512),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (artifact_id, source_ordinal),
    CONSTRAINT intelligence_artifact_sources_ordinal_check CHECK (source_ordinal >= 0),
    CONSTRAINT intelligence_artifact_sources_kind_check CHECK (
        source_kind::text = ANY (ARRAY[
            'media'::varchar,
            'media_context'::varchar,
            'search_document'::varchar,
            'artifact'::varchar,
            'run'::varchar,
            'tool_call'::varchar,
            'manual'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_artifact_sources_status_check CHECK (
        status::text = ANY (ARRAY[
            'active'::varchar,
            'stale'::varchar,
            'invalidated'::varchar
        ]::text[])
    ),
    CONSTRAINT intelligence_artifact_sources_reference_check CHECK (
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
        OR source_kind::text = 'manual'
    ),
    CONSTRAINT intelligence_artifact_sources_media_type_check CHECK (
        source_media_type IS NULL OR source_media_type = ANY (ARRAY[
            'movie'::media_type,
            'series'::media_type,
            'season'::media_type,
            'episode'::media_type
        ])
    ),
    CONSTRAINT intelligence_artifact_sources_hash_check CHECK (
        source_content_hash IS NULL OR source_content_hash ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT intelligence_artifact_sources_revision_check CHECK (source_revision >= 0),
    CONSTRAINT intelligence_artifact_sources_invalidated_check CHECK (
        (status::text = 'invalidated' AND invalidated_at IS NOT NULL)
        OR status::text <> 'invalidated'
    ),
    CONSTRAINT intelligence_artifact_sources_locator_object_check CHECK (
        jsonb_typeof(source_locator) = 'object'
    )
);

COMMENT ON TABLE intelligence_artifact_sources IS
    'Provenance edges from artifacts to media, search documents, other artifacts, runs, tool calls, or manual sources.';
COMMENT ON COLUMN intelligence_artifact_sources.source_locator IS
    'Structured locator such as field names, offsets, paths, or external IDs for the cited source.';

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_artifact
    ON intelligence_artifact_sources USING btree (artifact_id, source_ordinal);

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_media
    ON intelligence_artifact_sources USING btree (source_library_id, source_media_type, source_media_id)
    WHERE source_media_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_search_document
    ON intelligence_artifact_sources USING btree (source_search_document_id)
    WHERE source_search_document_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_source_artifact
    ON intelligence_artifact_sources USING btree (source_artifact_id)
    WHERE source_artifact_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_intelligence_artifact_sources_invalidation
    ON intelligence_artifact_sources USING btree (source_kind, source_revision, updated_at)
    WHERE invalidated_at IS NULL;

DO $$
DECLARE
    table_name text;
    trigger_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'intelligence_media_context',
        'intelligence_search_documents',
        'intelligence_runs',
        'intelligence_tool_calls',
        'intelligence_artifacts',
        'intelligence_artifact_sources'
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
