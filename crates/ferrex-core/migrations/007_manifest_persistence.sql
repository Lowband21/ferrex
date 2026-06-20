-- Persist scanner manifest runs, entries, diagnostics, partition cursors,
-- deferred watch hints, and legacy backfill state.
--
-- The baseline schema may live in either `ferrex` or `public` on upgraded
-- development databases. Resolve the owning app schema once and create new
-- objects there, matching the compatibility pattern used by prior migrations.
CREATE SCHEMA IF NOT EXISTS ferrex;

DO $$
DECLARE
    app_schema text;
BEGIN
    SELECT n.nspname
    INTO app_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.relname = 'media_files'
      AND c.relkind IN ('r', 'p')
      AND n.nspname IN ('ferrex', 'public')
    ORDER BY CASE WHEN n.nspname = 'ferrex' THEN 0 ELSE 1 END
    LIMIT 1;

    IF app_schema IS NULL THEN
        app_schema := 'ferrex';
    END IF;

    PERFORM set_config('search_path', format('%I, public', app_schema), false);
END $$;

CREATE TABLE IF NOT EXISTS manifest_runs (
    run_id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    library_type text NOT NULL,
    scope_kind text NOT NULL,
    root_id integer NOT NULL,
    root_path_norm text NOT NULL,
    partition_id integer,
    partition_prefix_norm text,
    status text NOT NULL DEFAULT 'pending',
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    entries_seen bigint DEFAULT 0 NOT NULL,
    diagnostics_seen bigint DEFAULT 0 NOT NULL,
    error_message text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT manifest_runs_pkey PRIMARY KEY (run_id),
    CONSTRAINT manifest_runs_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT manifest_runs_library_type_check CHECK (library_type = ANY (ARRAY['movies', 'tvshows'])),
    CONSTRAINT manifest_runs_scope_kind_check CHECK (scope_kind = ANY (ARRAY['root', 'partition'])),
    CONSTRAINT manifest_runs_root_id_check CHECK (root_id >= 0 AND root_id <= 65535),
    CONSTRAINT manifest_runs_partition_id_check CHECK (partition_id IS NULL OR (partition_id >= 0 AND partition_id <= 65535)),
    CONSTRAINT manifest_runs_status_check CHECK (status = ANY (ARRAY[
        'pending',
        'running',
        'completed',
        'completed_with_diagnostics',
        'failed',
        'canceled',
        'stalled'
    ])),
    CONSTRAINT manifest_runs_scope_partition_shape CHECK (
        (scope_kind = 'root' AND partition_id IS NULL AND partition_prefix_norm IS NULL)
        OR (scope_kind = 'partition' AND partition_id IS NOT NULL)
    ),
    CONSTRAINT manifest_runs_counts_check CHECK (entries_seen >= 0 AND diagnostics_seen >= 0)
);

COMMENT ON TABLE manifest_runs IS 'Durable lifecycle records for scanner manifest runs';
COMMENT ON COLUMN manifest_runs.scope_kind IS 'Manifest scope covered by the run: root or partition';
COMMENT ON COLUMN manifest_runs.root_id IS 'Stable ordinal for the configured library root';
COMMENT ON COLUMN manifest_runs.partition_id IS 'Stable bounded partition id inside a root when scope_kind = partition';

CREATE TABLE IF NOT EXISTS manifest_entries (
    library_id uuid NOT NULL,
    path_norm text NOT NULL,
    entry_kind text NOT NULL,
    library_type text NOT NULL,
    root_id integer NOT NULL,
    root_path_norm text NOT NULL,
    partition_id integer,
    partition_prefix_norm text,
    relative_path text NOT NULL,
    classification_status text NOT NULL,
    classification_kind text NOT NULL,
    classification_payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    fingerprint_device_id text,
    fingerprint_inode text,
    fingerprint_size bigint DEFAULT 0 NOT NULL,
    fingerprint_mtime_ms bigint,
    fingerprint_weak_hash text,
    first_seen_run_id uuid,
    last_seen_run_id uuid,
    first_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    availability text DEFAULT 'available' NOT NULL,
    source text DEFAULT 'manifest' NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT manifest_entries_pkey PRIMARY KEY (library_id, path_norm),
    CONSTRAINT manifest_entries_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT manifest_entries_first_seen_run_id_fkey FOREIGN KEY (first_seen_run_id) REFERENCES manifest_runs(run_id) ON DELETE SET NULL,
    CONSTRAINT manifest_entries_last_seen_run_id_fkey FOREIGN KEY (last_seen_run_id) REFERENCES manifest_runs(run_id) ON DELETE SET NULL,
    CONSTRAINT manifest_entries_entry_kind_check CHECK (entry_kind = ANY (ARRAY['file', 'directory'])),
    CONSTRAINT manifest_entries_library_type_check CHECK (library_type = ANY (ARRAY['movies', 'tvshows'])),
    CONSTRAINT manifest_entries_root_id_check CHECK (root_id >= 0 AND root_id <= 65535),
    CONSTRAINT manifest_entries_partition_id_check CHECK (partition_id IS NULL OR (partition_id >= 0 AND partition_id <= 65535)),
    CONSTRAINT manifest_entries_classification_status_check CHECK (classification_status = ANY (ARRAY['supported', 'ignored', 'unsupported'])),
    CONSTRAINT manifest_entries_fingerprint_size_check CHECK (fingerprint_size >= 0),
    CONSTRAINT manifest_entries_availability_check CHECK (availability = ANY (ARRAY['available', 'missing', 'unknown'])),
    CONSTRAINT manifest_entries_source_check CHECK (source = ANY (ARRAY['manifest', 'backfill']))
);

COMMENT ON TABLE manifest_entries IS 'Latest manifest view of filesystem entries by normalized path';
COMMENT ON COLUMN manifest_entries.source IS 'manifest for observed scanner runs, backfill for legacy media/folder inventory imports';
COMMENT ON COLUMN manifest_entries.availability IS 'Entry availability from manifest reconciliation; backfill always records available/known legacy state only';

CREATE TABLE IF NOT EXISTS manifest_diagnostics (
    id uuid DEFAULT uuidv7() NOT NULL,
    run_id uuid NOT NULL,
    library_id uuid NOT NULL,
    root_id integer NOT NULL,
    partition_id integer,
    path_norm text NOT NULL,
    reason text NOT NULL,
    code text NOT NULL,
    severity text NOT NULL,
    remediation text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT manifest_diagnostics_pkey PRIMARY KEY (id),
    CONSTRAINT manifest_diagnostics_run_id_fkey FOREIGN KEY (run_id) REFERENCES manifest_runs(run_id) ON DELETE CASCADE,
    CONSTRAINT manifest_diagnostics_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT manifest_diagnostics_root_id_check CHECK (root_id >= 0 AND root_id <= 65535),
    CONSTRAINT manifest_diagnostics_partition_id_check CHECK (partition_id IS NULL OR (partition_id >= 0 AND partition_id <= 65535)),
    CONSTRAINT manifest_diagnostics_severity_check CHECK (severity = ANY (ARRAY['info', 'warning', 'error']))
);

COMMENT ON TABLE manifest_diagnostics IS 'Operator-visible diagnostics emitted by manifest classification or reconciliation';

CREATE TABLE IF NOT EXISTS manifest_partition_cursors (
    library_id uuid NOT NULL,
    library_type text NOT NULL,
    root_id integer NOT NULL,
    root_path_norm text NOT NULL,
    partition_key text NOT NULL,
    partition_id integer,
    prefix_norm text,
    last_successful_run_id uuid,
    last_successful_at timestamp with time zone,
    last_observed_at timestamp with time zone,
    entries_seen bigint DEFAULT 0 NOT NULL,
    diagnostics_seen bigint DEFAULT 0 NOT NULL,
    supported_media_seen bigint DEFAULT 0 NOT NULL,
    first_path_norm text,
    last_path_norm text,
    legacy_scan_path_hash bigint,
    backfilled_from_legacy boolean DEFAULT false NOT NULL,
    backfilled_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT manifest_partition_cursors_pkey PRIMARY KEY (library_id, root_id, partition_key),
    CONSTRAINT manifest_partition_cursors_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT manifest_partition_cursors_last_run_fkey FOREIGN KEY (last_successful_run_id) REFERENCES manifest_runs(run_id) ON DELETE SET NULL,
    CONSTRAINT manifest_partition_cursors_library_type_check CHECK (library_type = ANY (ARRAY['movies', 'tvshows'])),
    CONSTRAINT manifest_partition_cursors_root_id_check CHECK (root_id >= 0 AND root_id <= 65535),
    CONSTRAINT manifest_partition_cursors_partition_id_check CHECK (partition_id IS NULL OR (partition_id >= 0 AND partition_id <= 65535)),
    CONSTRAINT manifest_partition_cursors_counts_check CHECK (
        entries_seen >= 0 AND diagnostics_seen >= 0 AND supported_media_seen >= 0
    )
);

COMMENT ON TABLE manifest_partition_cursors IS 'Durable per-root/partition manifest cursors and legacy scan cursor imports';
COMMENT ON COLUMN manifest_partition_cursors.last_successful_run_id IS 'Only successful manifest runs populate this field; legacy backfill leaves it NULL so tombstones remain gated on a real root observation';
COMMENT ON COLUMN manifest_partition_cursors.partition_key IS 'root for root-level cursors, partition id text for manifest partitions, or legacy:<scan path hash> for imported scan_cursors';

CREATE TABLE IF NOT EXISTS manifest_deferred_watch_hints (
    id uuid DEFAULT uuidv7() NOT NULL,
    library_id uuid NOT NULL,
    root_id integer NOT NULL,
    root_path_norm text NOT NULL,
    path_norm text NOT NULL,
    hint_kind text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    status text DEFAULT 'pending' NOT NULL,
    idempotency_key text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    available_at timestamp with time zone DEFAULT now() NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT manifest_deferred_watch_hints_pkey PRIMARY KEY (id),
    CONSTRAINT manifest_deferred_watch_hints_library_id_fkey FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE,
    CONSTRAINT manifest_deferred_watch_hints_root_id_check CHECK (root_id >= 0 AND root_id <= 65535),
    CONSTRAINT manifest_deferred_watch_hints_status_check CHECK (status = ANY (ARRAY['pending', 'applied', 'dropped'])),
    CONSTRAINT manifest_deferred_watch_hints_attempts_check CHECK (attempts >= 0)
);

COMMENT ON TABLE manifest_deferred_watch_hints IS 'Deferred filesystem-watch hints waiting for manifest partition/root reconciliation';
COMMENT ON COLUMN manifest_deferred_watch_hints.idempotency_key IS 'Stable watcher hint key used to merge duplicate watch notifications';

CREATE INDEX IF NOT EXISTS idx_manifest_runs_library_started
    ON manifest_runs (library_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_manifest_runs_status
    ON manifest_runs (status, started_at ASC);

CREATE INDEX IF NOT EXISTS idx_manifest_entries_last_seen_run
    ON manifest_entries (last_seen_run_id);

CREATE INDEX IF NOT EXISTS idx_manifest_entries_library_root_available
    ON manifest_entries (library_id, root_id, availability, path_norm);

CREATE INDEX IF NOT EXISTS idx_manifest_entries_classification
    ON manifest_entries (library_id, classification_status, classification_kind);

CREATE UNIQUE INDEX IF NOT EXISTS idx_manifest_diagnostics_run_path_code
    ON manifest_diagnostics (run_id, path_norm, code);

CREATE INDEX IF NOT EXISTS idx_manifest_diagnostics_library_created
    ON manifest_diagnostics (library_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_manifest_partition_cursors_stale
    ON manifest_partition_cursors (library_id, last_successful_at ASC NULLS FIRST, updated_at ASC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_manifest_partition_cursors_legacy_hash
    ON manifest_partition_cursors (library_id, legacy_scan_path_hash)
    WHERE legacy_scan_path_hash IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_manifest_deferred_watch_hints_idempotency
    ON manifest_deferred_watch_hints (library_id, idempotency_key);

CREATE INDEX IF NOT EXISTS idx_manifest_deferred_watch_hints_pending
    ON manifest_deferred_watch_hints (library_id, available_at ASC, created_at ASC)
    WHERE status = 'pending';

DO $$
BEGIN
    IF to_regproc('update_updated_at_column') IS NOT NULL THEN
        DROP TRIGGER IF EXISTS update_manifest_runs_updated_at ON manifest_runs;
        CREATE TRIGGER update_manifest_runs_updated_at
            BEFORE UPDATE ON manifest_runs
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

        DROP TRIGGER IF EXISTS update_manifest_entries_updated_at ON manifest_entries;
        CREATE TRIGGER update_manifest_entries_updated_at
            BEFORE UPDATE ON manifest_entries
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

        DROP TRIGGER IF EXISTS update_manifest_partition_cursors_updated_at ON manifest_partition_cursors;
        CREATE TRIGGER update_manifest_partition_cursors_updated_at
            BEFORE UPDATE ON manifest_partition_cursors
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

        DROP TRIGGER IF EXISTS update_manifest_deferred_watch_hints_updated_at ON manifest_deferred_watch_hints;
        CREATE TRIGGER update_manifest_deferred_watch_hints_updated_at
            BEFORE UPDATE ON manifest_deferred_watch_hints
            FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
    END IF;
END $$;
