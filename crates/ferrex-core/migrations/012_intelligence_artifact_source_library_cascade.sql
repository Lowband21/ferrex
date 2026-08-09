-- Keep provenance edges and scoped collections consistent when their owning
-- source, library, or user is deleted.
--
-- The original SET NULL actions contradicted CHECK constraints that require
-- the selected source/owner/scope reference to remain non-null. PostgreSQL
-- therefore rejected deletes instead of completing the owning cascade. An
-- edge or scoped row cannot remain meaningful without that referenced owner,
-- so delete it with the owner rather than manufacturing an invalid NULL.

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

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_library_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_media_context_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_search_document_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_artifact_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_run_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_tool_call_id_fkey;

ALTER TABLE intelligence_artifact_sources
    DROP CONSTRAINT IF EXISTS intelligence_artifact_sources_source_transcript_source_id_fkey;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_library_id_fkey
    FOREIGN KEY (source_library_id)
    REFERENCES libraries(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_media_context_id_fkey
    FOREIGN KEY (source_media_context_id)
    REFERENCES intelligence_media_context(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_search_document_id_fkey
    FOREIGN KEY (source_search_document_id)
    REFERENCES intelligence_search_documents(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_artifact_id_fkey
    FOREIGN KEY (source_artifact_id)
    REFERENCES intelligence_artifacts(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_run_id_fkey
    FOREIGN KEY (source_run_id)
    REFERENCES intelligence_runs(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_tool_call_id_fkey
    FOREIGN KEY (source_tool_call_id)
    REFERENCES intelligence_tool_calls(id)
    ON DELETE CASCADE;

ALTER TABLE intelligence_artifact_sources
    ADD CONSTRAINT intelligence_artifact_sources_source_transcript_source_id_fkey
    FOREIGN KEY (source_transcript_source_id)
    REFERENCES transcript_sources(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_library_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges when their source library is deleted; media edges cannot remain valid with a NULL source_library_id.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_media_context_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their media-context source; media-context edges require a non-null source reference.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_search_document_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their search-document source; search-document edges require a non-null source reference.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_artifact_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their related-artifact source; artifact edges require a non-null source reference.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_run_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their run source; run edges require a non-null source reference.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_tool_call_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their tool-call source; tool-call edges require a non-null source reference.';

COMMENT ON CONSTRAINT intelligence_artifact_sources_source_transcript_source_id_fkey
    ON intelligence_artifact_sources IS
    'Delete provenance edges with their transcript source; transcript-source edges require a non-null source reference.';

ALTER TABLE collection_definitions
    DROP CONSTRAINT IF EXISTS collection_definitions_owner_user_id_fkey;

ALTER TABLE collection_definitions
    DROP CONSTRAINT IF EXISTS collection_definitions_library_id_fkey;

ALTER TABLE collection_definitions
    ADD CONSTRAINT collection_definitions_owner_user_id_fkey
    FOREIGN KEY (owner_user_id)
    REFERENCES users(id)
    ON DELETE CASCADE;

ALTER TABLE collection_definitions
    ADD CONSTRAINT collection_definitions_library_id_fkey
    FOREIGN KEY (library_id)
    REFERENCES libraries(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT collection_definitions_owner_user_id_fkey
    ON collection_definitions IS
    'Delete user-owned collection definitions with their owner; owner_type=user requires owner_user_id.';

COMMENT ON CONSTRAINT collection_definitions_library_id_fkey
    ON collection_definitions IS
    'Delete library-scoped collection definitions with their library; scope=library requires library_id.';

ALTER TABLE collection_shelf_placements
    DROP CONSTRAINT IF EXISTS collection_shelf_placements_scope_user_id_fkey;

ALTER TABLE collection_shelf_placements
    DROP CONSTRAINT IF EXISTS collection_shelf_placements_scope_library_id_fkey;

ALTER TABLE collection_shelf_placements
    ADD CONSTRAINT collection_shelf_placements_scope_user_id_fkey
    FOREIGN KEY (scope_user_id)
    REFERENCES users(id)
    ON DELETE CASCADE;

ALTER TABLE collection_shelf_placements
    ADD CONSTRAINT collection_shelf_placements_scope_library_id_fkey
    FOREIGN KEY (scope_library_id)
    REFERENCES libraries(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT collection_shelf_placements_scope_user_id_fkey
    ON collection_shelf_placements IS
    'Delete user-scoped shelf placements with their user; placement_scope=user requires scope_user_id.';

COMMENT ON CONSTRAINT collection_shelf_placements_scope_library_id_fkey
    ON collection_shelf_placements IS
    'Delete library-scoped shelf placements with their library; placement_scope=library requires scope_library_id.';

ALTER TABLE sync_sessions
    DROP CONSTRAINT IF EXISTS sync_sessions_host_id_fkey;

ALTER TABLE sync_sessions
    ADD CONSTRAINT sync_sessions_host_id_fkey
    FOREIGN KEY (host_id)
    REFERENCES users(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT sync_sessions_host_id_fkey
    ON sync_sessions IS
    'Delete sync sessions with their required host user.';

ALTER TABLE sync_participants
    DROP CONSTRAINT IF EXISTS sync_participants_user_id_fkey;

ALTER TABLE sync_participants
    ADD CONSTRAINT sync_participants_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT sync_participants_user_id_fkey
    ON sync_participants IS
    'Delete sync participation with its required user; session deletion already cascades through session_id.';

ALTER TABLE sync_session_history
    DROP CONSTRAINT IF EXISTS sync_session_history_user_id_fkey;

ALTER TABLE sync_session_history
    ADD CONSTRAINT sync_session_history_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(id)
    ON DELETE SET NULL;

COMMENT ON CONSTRAINT sync_session_history_user_id_fkey
    ON sync_session_history IS
    'Preserve session audit history while clearing its optional actor when that user is deleted.';

ALTER TABLE user_permissions
    DROP CONSTRAINT IF EXISTS user_permissions_granted_by_fkey;

ALTER TABLE user_permissions
    ADD CONSTRAINT user_permissions_granted_by_fkey
    FOREIGN KEY (granted_by)
    REFERENCES users(id)
    ON DELETE SET NULL;

COMMENT ON CONSTRAINT user_permissions_granted_by_fkey
    ON user_permissions IS
    'Preserve permission assignments while clearing their optional grantor when that user is deleted.';

ALTER TABLE user_roles
    DROP CONSTRAINT IF EXISTS user_roles_granted_by_fkey;

ALTER TABLE user_roles
    ADD CONSTRAINT user_roles_granted_by_fkey
    FOREIGN KEY (granted_by)
    REFERENCES users(id)
    ON DELETE SET NULL;

COMMENT ON CONSTRAINT user_roles_granted_by_fkey
    ON user_roles IS
    'Preserve role assignments while clearing their optional grantor when that user is deleted.';

DELETE FROM user_episode_state AS episode_state
WHERE NOT EXISTS (
    SELECT 1
    FROM users
    WHERE users.id = episode_state.user_id
);

ALTER TABLE user_episode_state
    DROP CONSTRAINT IF EXISTS user_episode_state_user_id_fkey;

ALTER TABLE user_episode_state
    ADD CONSTRAINT user_episode_state_user_id_fkey
    FOREIGN KEY (user_id)
    REFERENCES users(id)
    ON DELETE CASCADE;

COMMENT ON CONSTRAINT user_episode_state_user_id_fkey
    ON user_episode_state IS
    'Delete episode watch state with its owning user; pre-existing orphan state is removed before this constraint is installed.';
