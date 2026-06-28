-- Collection schema foundation for definition metadata, membership ordering,
-- dynamic materialization, shelf placement, and imported/source provenance.
--
-- This migration intentionally avoids repository/query implementation. It also
-- leaves movie_collection_membership intact so existing TMDB collection facts can
-- be mapped into the new source tables by a later, explicit backfill slice.

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

CREATE TABLE IF NOT EXISTS collection_definitions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    stable_key text NOT NULL,
    external_key text,
    title text NOT NULL,
    description text,
    kind varchar(32) NOT NULL DEFAULT 'manual',
    source varchar(32) NOT NULL DEFAULT 'manual',
    owner_type varchar(32) NOT NULL DEFAULT 'system',
    owner_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    owner_device_id text,
    owner_display_name text,
    scope varchar(32) NOT NULL DEFAULT 'user',
    library_id uuid REFERENCES libraries(id) ON DELETE SET NULL,
    visibility varchar(32) NOT NULL DEFAULT 'private',
    presentation varchar(32) NOT NULL DEFAULT 'shelf',
    media_scope jsonb NOT NULL DEFAULT '{"type":"all"}'::jsonb,
    duplicate_policy varchar(32) NOT NULL DEFAULT 'reject_duplicates',
    artwork jsonb NOT NULL DEFAULT '{}'::jsonb,
    theme jsonb NOT NULL DEFAULT '{}'::jsonb,
    provenance jsonb NOT NULL DEFAULT '{"source":"manual"}'::jsonb,
    contract_version integer NOT NULL DEFAULT 1,
    revision bigint NOT NULL DEFAULT 0,
    etag text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    archived_by uuid REFERENCES users(id) ON DELETE SET NULL,
    archive_reason text,
    deleted_at timestamptz,
    deleted_by uuid REFERENCES users(id) ON DELETE SET NULL,
    delete_reason text,
    CONSTRAINT collection_definitions_stable_key_check CHECK (length(btrim(stable_key)) > 0),
    CONSTRAINT collection_definitions_title_check CHECK (length(btrim(title)) > 0),
    CONSTRAINT collection_definitions_kind_check CHECK (kind IN ('manual', 'dynamic_rule', 'tmdb_list', 'tmdb_collection', 'system')),
    CONSTRAINT collection_definitions_source_check CHECK (source IN ('manual', 'dynamic_rule', 'tmdb', 'system', 'imported')),
    CONSTRAINT collection_definitions_owner_type_check CHECK (owner_type IN ('user', 'device', 'external', 'system')),
    CONSTRAINT collection_definitions_owner_identity_check CHECK (
        (owner_type <> 'user' OR owner_user_id IS NOT NULL)
        AND (owner_type <> 'device' OR owner_device_id IS NOT NULL)
    ),
    CONSTRAINT collection_definitions_scope_check CHECK (scope IN ('user', 'global', 'library', 'shared')),
    CONSTRAINT collection_definitions_library_scope_check CHECK (scope <> 'library' OR library_id IS NOT NULL),
    CONSTRAINT collection_definitions_visibility_check CHECK (visibility IN ('private', 'shared', 'public', 'system')),
    CONSTRAINT collection_definitions_presentation_check CHECK (presentation IN ('shelf', 'grid', 'list', 'playlist', 'hero', 'hidden')),
    CONSTRAINT collection_definitions_duplicate_policy_check CHECK (duplicate_policy IN ('keep_all', 'deduplicate_media', 'deduplicate_logical', 'reject_duplicates')),
    CONSTRAINT collection_definitions_media_scope_object_check CHECK (jsonb_typeof(media_scope) = 'object'),
    CONSTRAINT collection_definitions_artwork_object_check CHECK (jsonb_typeof(artwork) = 'object'),
    CONSTRAINT collection_definitions_theme_object_check CHECK (jsonb_typeof(theme) = 'object'),
    CONSTRAINT collection_definitions_provenance_object_check CHECK (jsonb_typeof(provenance) = 'object'),
    CONSTRAINT collection_definitions_contract_version_check CHECK (contract_version > 0),
    CONSTRAINT collection_definitions_revision_check CHECK (revision >= 0),
    CONSTRAINT collection_definitions_archive_delete_order_check CHECK (
        deleted_at IS NULL OR archived_at IS NULL OR deleted_at >= archived_at
    )
);

COMMENT ON TABLE collection_definitions IS
    'Collection definition metadata only; repository behavior and UI use are added by later slices.';
COMMENT ON COLUMN collection_definitions.media_scope IS
    'JSON form of CollectionMediaScope, retained as a schemaless contract payload for mixed movie/series/season/episode scopes.';
COMMENT ON COLUMN collection_definitions.provenance IS
    'JSON form of CollectionProvenance plus import/source hints; detailed imported membership rows live in collection_sources and collection_source_memberships.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_definitions_stable_key
    ON collection_definitions USING btree (stable_key);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_definitions_external_key
    ON collection_definitions USING btree (external_key)
    WHERE external_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_collection_definitions_owner_active
    ON collection_definitions USING btree (owner_type, owner_user_id, scope, visibility, updated_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_collection_definitions_library_active
    ON collection_definitions USING btree (library_id, visibility, presentation, updated_at DESC)
    WHERE library_id IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_collection_definitions_kind_source
    ON collection_definitions USING btree (kind, source, updated_at DESC)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS collection_manual_memberships (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    collection_id uuid NOT NULL REFERENCES collection_definitions(id) ON DELETE CASCADE,
    item_key text NOT NULL,
    media_type media_type NOT NULL,
    media_id uuid NOT NULL,
    title_snapshot text,
    subtitle_snapshot text,
    position_key numeric(38, 19) NOT NULL,
    sort_key text,
    availability_status varchar(32) NOT NULL DEFAULT 'available',
    availability_reason text,
    availability_checked_at timestamptz,
    added_by uuid REFERENCES users(id) ON DELETE SET NULL,
    added_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT collection_manual_memberships_item_key_check CHECK (length(btrim(item_key)) > 0),
    CONSTRAINT collection_manual_memberships_media_type_check CHECK (
        media_type IN ('movie'::media_type, 'series'::media_type, 'season'::media_type, 'episode'::media_type)
    ),
    CONSTRAINT collection_manual_memberships_position_key_check CHECK (position_key >= 0),
    CONSTRAINT collection_manual_memberships_availability_check CHECK (availability_status IN ('available', 'pending', 'missing', 'unavailable', 'tombstoned', 'archived')),
    CONSTRAINT collection_manual_memberships_metadata_object_check CHECK (jsonb_typeof(metadata) = 'object')
);

COMMENT ON TABLE collection_manual_memberships IS
    'Manual collection membership ordered by stable position keys. It intentionally has no media/file foreign key, so file unavailability or media re-indexing cannot cascade-delete collection membership.';
COMMENT ON COLUMN collection_manual_memberships.item_key IS
    'Stable API member key such as movie:<uuid>, series:<uuid>, season:<uuid>, or episode:<uuid>.';
COMMENT ON COLUMN collection_manual_memberships.position_key IS
    'Sparse, stable ordering key used for deterministic manual reordering without relying on dense row numbers.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_manual_memberships_item_key
    ON collection_manual_memberships USING btree (collection_id, item_key);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_manual_memberships_media
    ON collection_manual_memberships USING btree (collection_id, media_type, media_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_manual_memberships_position
    ON collection_manual_memberships USING btree (collection_id, position_key);

CREATE INDEX IF NOT EXISTS idx_collection_manual_memberships_ordered
    ON collection_manual_memberships USING btree (collection_id, position_key, id);

CREATE INDEX IF NOT EXISTS idx_collection_manual_memberships_media_lookup
    ON collection_manual_memberships USING btree (media_type, media_id, collection_id);

CREATE TABLE IF NOT EXISTS collection_dynamic_rules (
    collection_id uuid PRIMARY KEY REFERENCES collection_definitions(id) ON DELETE CASCADE,
    rule_json jsonb NOT NULL,
    rule_schema_version integer NOT NULL DEFAULT 1,
    rule_hash text NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    last_validated_at timestamptz,
    last_validation_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT collection_dynamic_rules_rule_object_check CHECK (jsonb_typeof(rule_json) = 'object'),
    CONSTRAINT collection_dynamic_rules_schema_version_check CHECK (rule_schema_version > 0),
    CONSTRAINT collection_dynamic_rules_hash_check CHECK (length(btrim(rule_hash)) > 0)
);

COMMENT ON TABLE collection_dynamic_rules IS
    'Stores the versioned JSON rule contract and hash for dynamic collections; rule evaluation remains out of this migration.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_dynamic_rules_hash
    ON collection_dynamic_rules USING btree (collection_id, rule_hash);

CREATE INDEX IF NOT EXISTS idx_collection_dynamic_rules_enabled_hash
    ON collection_dynamic_rules USING btree (enabled, rule_hash, updated_at DESC);

CREATE TABLE IF NOT EXISTS collection_materializations (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    collection_id uuid NOT NULL REFERENCES collection_definitions(id) ON DELETE CASCADE,
    materialization_scope varchar(16) NOT NULL DEFAULT 'global',
    materialization_key text NOT NULL DEFAULT 'global',
    user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    rule_hash text NOT NULL,
    rule_schema_version integer NOT NULL DEFAULT 1,
    materialization_schema_version integer NOT NULL DEFAULT 1,
    state varchar(32) NOT NULL DEFAULT 'pending',
    evaluated_at timestamptz,
    stale_at timestamptz,
    stale_reason text,
    error_at timestamptz,
    error_code text,
    error_message text,
    total_count integer NOT NULL DEFAULT 0,
    visible_count integer NOT NULL DEFAULT 0,
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT collection_materializations_scope_check CHECK (materialization_scope IN ('global', 'user')),
    CONSTRAINT collection_materializations_scope_identity_check CHECK (
        (materialization_scope = 'global' AND user_id IS NULL)
        OR (materialization_scope = 'user' AND user_id IS NOT NULL)
    ),
    CONSTRAINT collection_materializations_key_check CHECK (length(btrim(materialization_key)) > 0),
    CONSTRAINT collection_materializations_hash_check CHECK (length(btrim(rule_hash)) > 0),
    CONSTRAINT collection_materializations_rule_schema_check CHECK (rule_schema_version > 0),
    CONSTRAINT collection_materializations_schema_check CHECK (materialization_schema_version > 0),
    CONSTRAINT collection_materializations_state_check CHECK (state IN ('not_materialized', 'pending', 'refreshing', 'ready', 'stale', 'failed')),
    CONSTRAINT collection_materializations_counts_check CHECK (total_count >= 0 AND visible_count >= 0 AND visible_count <= total_count),
    CONSTRAINT collection_materializations_state_metadata_check CHECK (
        (state <> 'ready' OR evaluated_at IS NOT NULL)
        AND (state <> 'stale' OR stale_at IS NOT NULL)
        AND (state <> 'failed' OR (error_at IS NOT NULL AND error_message IS NOT NULL))
    )
);

COMMENT ON TABLE collection_materializations IS
    'Per-rule materialization state. materialization_key supports global and per-user cached results without changing the collection definition row.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_materializations_key
    ON collection_materializations USING btree (collection_id, materialization_key);

CREATE INDEX IF NOT EXISTS idx_collection_materializations_user_state
    ON collection_materializations USING btree (collection_id, user_id, state, updated_at DESC)
    WHERE materialization_scope = 'user';

CREATE INDEX IF NOT EXISTS idx_collection_materializations_stale
    ON collection_materializations USING btree (state, stale_at, updated_at DESC)
    WHERE state IN ('stale', 'failed', 'pending');

CREATE TABLE IF NOT EXISTS collection_materialized_items (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    materialization_id uuid NOT NULL REFERENCES collection_materializations(id) ON DELETE CASCADE,
    collection_id uuid NOT NULL REFERENCES collection_definitions(id) ON DELETE CASCADE,
    materialization_key text NOT NULL,
    item_key text NOT NULL,
    media_type media_type NOT NULL,
    media_id uuid NOT NULL,
    position integer NOT NULL,
    order_key text NOT NULL,
    visible boolean NOT NULL DEFAULT true,
    hidden_reason text,
    source_membership_id uuid,
    evaluated_at timestamptz NOT NULL DEFAULT now(),
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT collection_materialized_items_item_key_check CHECK (length(btrim(item_key)) > 0),
    CONSTRAINT collection_materialized_items_media_type_check CHECK (
        media_type IN ('movie'::media_type, 'series'::media_type, 'season'::media_type, 'episode'::media_type)
    ),
    CONSTRAINT collection_materialized_items_position_check CHECK (position >= 0),
    CONSTRAINT collection_materialized_items_order_key_check CHECK (length(btrim(order_key)) > 0),
    CONSTRAINT collection_materialized_items_metadata_object_check CHECK (jsonb_typeof(metadata) = 'object')
);

COMMENT ON TABLE collection_materialized_items IS
    'Ordered dynamic collection output rows. Rows reference collection/materialization state only, not media files, so unavailable files do not erase materialized history.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_materialized_items_item_key
    ON collection_materialized_items USING btree (materialization_id, item_key);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_materialized_items_position
    ON collection_materialized_items USING btree (materialization_id, position);

CREATE INDEX IF NOT EXISTS idx_collection_materialized_items_ordered
    ON collection_materialized_items USING btree (collection_id, materialization_key, position, id);

CREATE INDEX IF NOT EXISTS idx_collection_materialized_items_visible
    ON collection_materialized_items USING btree (collection_id, materialization_key, position)
    WHERE visible = true;

CREATE INDEX IF NOT EXISTS idx_collection_materialized_items_media_lookup
    ON collection_materialized_items USING btree (media_type, media_id, collection_id);

CREATE TABLE IF NOT EXISTS collection_shelf_placements (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    schema_version integer NOT NULL DEFAULT 1,
    collection_id uuid NOT NULL,
    collection_stable_key text,
    surface varchar(32) NOT NULL DEFAULT 'home',
    shelf_key text NOT NULL,
    placement_scope varchar(16) NOT NULL DEFAULT 'global',
    placement_scope_key text NOT NULL DEFAULT 'global',
    scope_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    scope_library_id uuid REFERENCES libraries(id) ON DELETE SET NULL,
    visibility varchar(32) NOT NULL DEFAULT 'private',
    presentation varchar(32) NOT NULL DEFAULT 'shelf',
    pinned boolean NOT NULL DEFAULT false,
    pinned_at timestamptz,
    pinned_by uuid REFERENCES users(id) ON DELETE SET NULL,
    position integer NOT NULL,
    position_key numeric(38, 19) NOT NULL,
    reordered_at timestamptz,
    reordered_by uuid REFERENCES users(id) ON DELETE SET NULL,
    reorder_revision bigint NOT NULL DEFAULT 0,
    hidden_at timestamptz,
    hidden_by uuid REFERENCES users(id) ON DELETE SET NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT collection_shelf_placements_schema_version_check CHECK (schema_version > 0),
    CONSTRAINT collection_shelf_placements_surface_check CHECK (surface IN ('home', 'library', 'collection_detail', 'search', 'admin')),
    CONSTRAINT collection_shelf_placements_shelf_key_check CHECK (length(btrim(shelf_key)) > 0),
    CONSTRAINT collection_shelf_placements_scope_check CHECK (placement_scope IN ('global', 'user', 'library', 'device')),
    CONSTRAINT collection_shelf_placements_scope_key_check CHECK (length(btrim(placement_scope_key)) > 0),
    CONSTRAINT collection_shelf_placements_scope_identity_check CHECK (
        (placement_scope <> 'user' OR scope_user_id IS NOT NULL)
        AND (placement_scope <> 'library' OR scope_library_id IS NOT NULL)
    ),
    CONSTRAINT collection_shelf_placements_visibility_check CHECK (visibility IN ('private', 'shared', 'public', 'system')),
    CONSTRAINT collection_shelf_placements_presentation_check CHECK (presentation IN ('shelf', 'grid', 'list', 'playlist', 'hero', 'hidden')),
    CONSTRAINT collection_shelf_placements_position_check CHECK (position >= 0 AND position_key >= 0),
    CONSTRAINT collection_shelf_placements_reorder_revision_check CHECK (reorder_revision >= 0),
    CONSTRAINT collection_shelf_placements_metadata_object_check CHECK (jsonb_typeof(metadata) = 'object')
);

COMMENT ON TABLE collection_shelf_placements IS
    'Pinned/reordered shelf placement records. collection_id is intentionally not a foreign key so placements can survive collection archive/delete workflows and be reconciled by higher layers.';
COMMENT ON COLUMN collection_shelf_placements.placement_scope_key IS
    'Stable scope discriminator such as global, user:<uuid>, library:<uuid>, or device:<id> used for deterministic uniqueness with nullable owner columns.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_shelf_placements_collection
    ON collection_shelf_placements USING btree (surface, shelf_key, placement_scope, placement_scope_key, collection_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_shelf_placements_position
    ON collection_shelf_placements USING btree (surface, shelf_key, placement_scope, placement_scope_key, position_key);

CREATE INDEX IF NOT EXISTS idx_collection_shelf_placements_ordered
    ON collection_shelf_placements USING btree (surface, shelf_key, placement_scope, placement_scope_key, pinned DESC, position_key, id);

CREATE INDEX IF NOT EXISTS idx_collection_shelf_placements_visible
    ON collection_shelf_placements USING btree (surface, shelf_key, visibility, position_key)
    WHERE hidden_at IS NULL;

CREATE TABLE IF NOT EXISTS collection_sources (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    collection_id uuid REFERENCES collection_definitions(id) ON DELETE SET NULL,
    provider varchar(32) NOT NULL DEFAULT 'tmdb',
    source_kind varchar(32) NOT NULL,
    source_key text NOT NULL,
    source_scope_key text NOT NULL DEFAULT 'global',
    external_owner text,
    title text,
    description text,
    source_url text,
    imported_at timestamptz NOT NULL DEFAULT now(),
    refreshed_at timestamptz,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT collection_sources_provider_check CHECK (provider IN ('tmdb', 'ferrex', 'imported', 'manual', 'system')),
    CONSTRAINT collection_sources_kind_check CHECK (source_kind IN ('tmdb_collection', 'tmdb_list', 'tmdb_keyword', 'imported_list', 'manual', 'system')),
    CONSTRAINT collection_sources_source_key_check CHECK (length(btrim(source_key)) > 0),
    CONSTRAINT collection_sources_scope_key_check CHECK (length(btrim(source_scope_key)) > 0),
    CONSTRAINT collection_sources_payload_object_check CHECK (jsonb_typeof(payload) = 'object')
);

COMMENT ON TABLE collection_sources IS
    'Imported/source provenance for collections, including TMDB collections/lists/keywords. Rows may outlive collection_definitions rows for audit/backfill purposes.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_sources_external
    ON collection_sources USING btree (provider, source_kind, source_key, source_scope_key);

CREATE INDEX IF NOT EXISTS idx_collection_sources_collection
    ON collection_sources USING btree (collection_id, provider, source_kind, updated_at DESC)
    WHERE collection_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS collection_source_memberships (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    source_id uuid NOT NULL REFERENCES collection_sources(id) ON DELETE CASCADE,
    collection_id uuid REFERENCES collection_definitions(id) ON DELETE SET NULL,
    item_key text,
    media_type media_type,
    media_id uuid,
    external_media_type varchar(32) NOT NULL DEFAULT 'movie',
    external_id text NOT NULL,
    external_position integer,
    source_order_key text NOT NULL,
    title text,
    poster_path text,
    backdrop_path text,
    match_status varchar(32) NOT NULL DEFAULT 'pending',
    matched_at timestamptz,
    legacy_movie_collection_movie_id uuid,
    legacy_movie_collection_tmdb_id bigint,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    imported_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT collection_source_memberships_item_key_check CHECK (item_key IS NULL OR length(btrim(item_key)) > 0),
    CONSTRAINT collection_source_memberships_media_pair_check CHECK ((media_type IS NULL AND media_id IS NULL) OR (media_type IS NOT NULL AND media_id IS NOT NULL)),
    CONSTRAINT collection_source_memberships_media_type_check CHECK (
        media_type IS NULL OR media_type IN ('movie'::media_type, 'series'::media_type, 'season'::media_type, 'episode'::media_type)
    ),
    CONSTRAINT collection_source_memberships_external_media_type_check CHECK (external_media_type IN ('movie', 'series', 'season', 'episode', 'person', 'list', 'collection', 'keyword')),
    CONSTRAINT collection_source_memberships_external_id_check CHECK (length(btrim(external_id)) > 0),
    CONSTRAINT collection_source_memberships_external_position_check CHECK (external_position IS NULL OR external_position >= 0),
    CONSTRAINT collection_source_memberships_order_key_check CHECK (length(btrim(source_order_key)) > 0),
    CONSTRAINT collection_source_memberships_match_status_check CHECK (match_status IN ('pending', 'matched', 'missing', 'skipped', 'failed')),
    CONSTRAINT collection_source_memberships_payload_object_check CHECK (jsonb_typeof(payload) = 'object')
);

COMMENT ON TABLE collection_source_memberships IS
    'Imported membership/provenance rows for TMDB and other source data. Matching to Ferrex media is optional and non-cascading.';
COMMENT ON COLUMN collection_source_memberships.legacy_movie_collection_movie_id IS
    'Optional pointer to movie_collection_membership.movie_id used by a later backfill; intentionally not a foreign key so legacy data remains untouched.';
COMMENT ON COLUMN collection_source_memberships.legacy_movie_collection_tmdb_id IS
    'Optional copy of movie_collection_membership.collection_id used by a later backfill.';
COMMENT ON TABLE movie_collection_membership IS
    'Legacy TMDB movie collection cache retained for future mapping into collection_sources and collection_source_memberships; this migration does not move or delete existing rows.';

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_source_memberships_external
    ON collection_source_memberships USING btree (source_id, external_media_type, external_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_source_memberships_order
    ON collection_source_memberships USING btree (source_id, source_order_key);

CREATE INDEX IF NOT EXISTS idx_collection_source_memberships_collection
    ON collection_source_memberships USING btree (collection_id, media_type, media_id)
    WHERE collection_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_collection_source_memberships_match_status
    ON collection_source_memberships USING btree (source_id, match_status, external_position);

CREATE INDEX IF NOT EXISTS idx_collection_source_memberships_legacy_movie_collection
    ON collection_source_memberships USING btree (legacy_movie_collection_tmdb_id, legacy_movie_collection_movie_id)
    WHERE legacy_movie_collection_tmdb_id IS NOT NULL;
