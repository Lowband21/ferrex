//! PostgreSQL library/user-deletion integrity regressions.

use ferrex_core::database::{
    repositories::{
        library::PostgresLibraryRepository, users::PostgresUsersRepository,
    },
    repository_ports::{library::LibraryRepository, users::UsersRepository},
};
use ferrex_core::types::LibraryId;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_library(pool: &PgPool, id: Uuid, name: &str) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, 'movies', ARRAY[$3]::varchar[])
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(format!("/fixtures/{name}"))
    .execute(pool)
    .await
    .expect("seed library");
}

async fn seed_user(pool: &PgPool, id: Uuid, username: &str) {
    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(id)
    .bind(username)
    .bind(format!("Deletion fixture {username}"))
    .execute(pool)
    .await
    .expect("seed user");
}

async fn seed_artifact(
    pool: &PgPool,
    id: Uuid,
    library_id: Option<Uuid>,
    user_id: Option<Uuid>,
    title: &str,
) {
    let scope = if user_id.is_some() { "user" } else { "global" };
    sqlx::query(
        r#"
        INSERT INTO intelligence_artifacts (
            id, artifact_kind, scope, status, library_id, user_id, title,
            content_hash
        )
        VALUES ($1, 'note', $2, 'active', $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(scope)
    .bind(library_id)
    .bind(user_id)
    .bind(title)
    .bind("1".repeat(64))
    .execute(pool)
    .await
    .expect("seed artifact");
}

async fn seed_media_context(
    pool: &PgPool,
    id: Uuid,
    library_id: Uuid,
    user_id: Option<Uuid>,
    media_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO intelligence_media_context (
            id, library_id, user_id, media_id, media_type, context_kind,
            title, content_hash
        )
        VALUES ($1, $2, $3, $4, 'movie', 'metadata', $5, $6)
        "#,
    )
    .bind(id)
    .bind(library_id)
    .bind(user_id)
    .bind(media_id)
    .bind(format!("Context {id}"))
    .bind("2".repeat(64))
    .execute(pool)
    .await
    .expect("seed media context");
}

async fn seed_search_document(
    pool: &PgPool,
    id: Uuid,
    library_id: Uuid,
    user_id: Option<Uuid>,
    media_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO intelligence_search_documents (
            id, library_id, user_id, media_id, media_type, document_kind,
            title, search_text, content_hash
        )
        VALUES ($1, $2, $3, $4, 'movie', 'combined', $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(library_id)
    .bind(user_id)
    .bind(media_id)
    .bind(format!("Document {id}"))
    .bind("bounded deletion regression document")
    .bind("3".repeat(64))
    .execute(pool)
    .await
    .expect("seed search document");
}

async fn seed_transcript_source(
    pool: &PgPool,
    id: Uuid,
    library_id: Uuid,
    media_id: Uuid,
) {
    let media_file_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, 1)
        "#,
    )
    .bind(media_file_id)
    .bind(library_id)
    .bind(media_id)
    .bind(format!("/fixtures/{media_file_id}.mkv"))
    .bind(format!("{media_file_id}.mkv"))
    .execute(pool)
    .await
    .expect("seed transcript media file");

    sqlx::query(
        r#"
        INSERT INTO transcript_sources (
            id, library_id, media_id, media_type, media_file_id, source_kind,
            status, language_code, source_key, source_content_hash
        )
        VALUES ($1, $2, $3, 'movie', $4, 'manual', 'active', 'und', $5, $6)
        "#,
    )
    .bind(id)
    .bind(library_id)
    .bind(media_id)
    .bind(media_file_id)
    .bind(format!("manual:{id}"))
    .bind("4".repeat(64))
    .execute(pool)
    .await
    .expect("seed transcript source");
}

async fn seed_library_scoped_collection(
    pool: &PgPool,
    collection_id: Uuid,
    placement_id: Uuid,
    library_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO collection_definitions (
            id, stable_key, title, owner_type, scope, library_id
        )
        VALUES ($1, $2, $3, 'system', 'library', $4)
        "#,
    )
    .bind(collection_id)
    .bind(format!("library-delete-{collection_id}"))
    .bind("Library deletion collection")
    .bind(library_id)
    .execute(pool)
    .await
    .expect("seed library-scoped collection");

    sqlx::query(
        r#"
        INSERT INTO collection_shelf_placements (
            id, collection_id, surface, shelf_key, placement_scope,
            placement_scope_key, scope_library_id, position, position_key
        )
        VALUES ($1, $2, 'library', $3, 'library', $4, $5, 0, 0)
        "#,
    )
    .bind(placement_id)
    .bind(collection_id)
    .bind(format!("library-delete-{collection_id}"))
    .bind(format!("library:{library_id}"))
    .bind(library_id)
    .execute(pool)
    .await
    .expect("seed library-scoped shelf placement");
}

async fn seed_user_scoped_collection(
    pool: &PgPool,
    collection_id: Uuid,
    placement_id: Uuid,
    user_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO collection_definitions (
            id, stable_key, title, owner_type, owner_user_id, scope
        )
        VALUES ($1, $2, $3, 'user', $4, 'user')
        "#,
    )
    .bind(collection_id)
    .bind(format!("user-delete-{collection_id}"))
    .bind("User deletion collection")
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user-owned collection");

    sqlx::query(
        r#"
        INSERT INTO collection_shelf_placements (
            id, collection_id, surface, shelf_key, placement_scope,
            placement_scope_key, scope_user_id, position, position_key
        )
        VALUES ($1, $2, 'home', $3, 'user', $4, $5, 0, 0)
        "#,
    )
    .bind(placement_id)
    .bind(collection_id)
    .bind(format!("user-delete-{collection_id}"))
    .bind(format!("user:{user_id}"))
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed user-scoped shelf placement");
}

async fn seed_intelligence_run_and_tool_call(
    pool: &PgPool,
    run_id: Uuid,
    tool_call_id: Uuid,
) {
    sqlx::query(
        r#"
        INSERT INTO intelligence_runs (id, run_kind, status)
        VALUES ($1, 'search', 'queued')
        "#,
    )
    .bind(run_id)
    .execute(pool)
    .await
    .expect("seed intelligence run");

    sqlx::query(
        r#"
        INSERT INTO intelligence_tool_calls (
            id, run_id, sequence, tool_kind, tool_name, status
        )
        VALUES ($1, $2, 0, 'search', 'deletion-regression', 'queued')
        "#,
    )
    .bind(tool_call_id)
    .bind(run_id)
    .execute(pool)
    .await
    .expect("seed intelligence tool call");
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn deleting_library_removes_its_provenance_edges_without_deleting_artifact(
    pool: PgPool,
) {
    let deleted_library_id = Uuid::now_v7();
    let retained_library_id = Uuid::now_v7();
    let artifact_id = Uuid::now_v7();

    seed_library(&pool, deleted_library_id, "delete-source-library").await;
    seed_library(&pool, retained_library_id, "retain-source-library").await;

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifacts (
            id, artifact_kind, scope, status, title, content_hash
        )
        VALUES ($1, 'note', 'global', 'active', $2, $3)
        "#,
    )
    .bind(artifact_id)
    .bind("Cross-library artifact")
    .bind("0".repeat(64))
    .execute(&pool)
    .await
    .expect("seed global artifact");

    for (source_ordinal, source_library_id) in
        [(0_i32, deleted_library_id), (1_i32, retained_library_id)]
    {
        sqlx::query(
            r#"
            INSERT INTO intelligence_artifact_sources (
                artifact_id,
                source_ordinal,
                source_kind,
                source_library_id,
                source_media_id,
                source_media_type
            )
            VALUES ($1, $2, 'media', $3, $4, 'movie')
            "#,
        )
        .bind(artifact_id)
        .bind(source_ordinal)
        .bind(source_library_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .expect("seed media provenance edge");
    }

    PostgresLibraryRepository::new(pool.clone())
        .delete_library(LibraryId(deleted_library_id))
        .await
        .expect("library deletion should cascade through provenance edges");

    let deleted_library_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM libraries WHERE id = $1",
    )
    .bind(deleted_library_id)
    .fetch_one(&pool)
    .await
    .expect("count deleted library");
    assert_eq!(deleted_library_count, 0);

    let remaining_source_libraries = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT source_library_id
        FROM intelligence_artifact_sources
        WHERE artifact_id = $1
        ORDER BY source_ordinal
        "#,
    )
    .bind(artifact_id)
    .fetch_all(&pool)
    .await
    .expect("load remaining provenance edges");
    assert_eq!(remaining_source_libraries, vec![retained_library_id]);

    let artifact_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifacts WHERE id = $1",
    )
    .bind(artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count preserved artifact");
    assert_eq!(artifact_count, 1);
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn deleting_run_and_tool_call_removes_their_provenance_edges(
    pool: PgPool,
) {
    let artifact_id = Uuid::now_v7();
    let run_id = Uuid::now_v7();
    let tool_call_id = Uuid::now_v7();

    seed_artifact(
        &pool,
        artifact_id,
        None,
        None,
        "Preserved run provenance owner",
    )
    .await;
    seed_intelligence_run_and_tool_call(&pool, run_id, tool_call_id).await;

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_run_id
        )
        VALUES ($1, 0, 'run', $2)
        "#,
    )
    .bind(artifact_id)
    .bind(run_id)
    .execute(&pool)
    .await
    .expect("seed run provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_tool_call_id
        )
        VALUES ($1, 1, 'tool_call', $2)
        "#,
    )
    .bind(artifact_id)
    .bind(tool_call_id)
    .execute(&pool)
    .await
    .expect("seed tool-call provenance");

    sqlx::query("DELETE FROM intelligence_tool_calls WHERE id = $1")
        .bind(tool_call_id)
        .execute(&pool)
        .await
        .expect("delete intelligence tool call");

    let remaining_ordinals = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT source_ordinal
        FROM intelligence_artifact_sources
        WHERE artifact_id = $1
        ORDER BY source_ordinal
        "#,
    )
    .bind(artifact_id)
    .fetch_all(&pool)
    .await
    .expect("load provenance after tool-call deletion");
    assert_eq!(remaining_ordinals, vec![0]);

    sqlx::query("DELETE FROM intelligence_runs WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await
        .expect("delete intelligence run");

    let remaining_source_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifact_sources WHERE artifact_id = $1",
    )
    .bind(artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count provenance after run deletion");
    assert_eq!(remaining_source_count, 0);

    let artifact_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifacts WHERE id = $1",
    )
    .bind(artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count preserved run provenance owner");
    assert_eq!(artifact_count, 1);
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn deleting_library_cascades_indirect_provenance_and_scoped_collections(
    pool: PgPool,
) {
    let library_id = Uuid::now_v7();
    let owner_artifact_id = Uuid::now_v7();
    let related_artifact_id = Uuid::now_v7();
    let media_id = Uuid::now_v7();
    let media_context_id = Uuid::now_v7();
    let search_document_id = Uuid::now_v7();
    let transcript_source_id = Uuid::now_v7();
    let collection_id = Uuid::now_v7();
    let placement_id = Uuid::now_v7();

    seed_library(&pool, library_id, "cascade-all-library-edges").await;
    seed_artifact(
        &pool,
        owner_artifact_id,
        None,
        None,
        "Preserved cross-library artifact",
    )
    .await;
    seed_artifact(
        &pool,
        related_artifact_id,
        Some(library_id),
        None,
        "Deleted library artifact",
    )
    .await;
    seed_media_context(&pool, media_context_id, library_id, None, media_id)
        .await;
    seed_search_document(&pool, search_document_id, library_id, None, media_id)
        .await;
    seed_transcript_source(&pool, transcript_source_id, library_id, media_id)
        .await;
    seed_library_scoped_collection(
        &pool,
        collection_id,
        placement_id,
        library_id,
    )
    .await;

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_library_id,
            source_media_id, source_media_type
        )
        VALUES ($1, 0, 'media', $2, $3, 'movie')
        "#,
    )
    .bind(owner_artifact_id)
    .bind(library_id)
    .bind(media_id)
    .execute(&pool)
    .await
    .expect("seed direct library provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_media_context_id
        )
        VALUES ($1, 1, 'media_context', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(media_context_id)
    .execute(&pool)
    .await
    .expect("seed media-context provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind,
            source_search_document_id
        )
        VALUES ($1, 2, 'search_document', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(search_document_id)
    .execute(&pool)
    .await
    .expect("seed search-document provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_artifact_id
        )
        VALUES ($1, 3, 'artifact', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(related_artifact_id)
    .execute(&pool)
    .await
    .expect("seed related-artifact provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind,
            source_transcript_source_id
        )
        VALUES ($1, 4, 'transcript_source', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(transcript_source_id)
    .execute(&pool)
    .await
    .expect("seed transcript-source provenance");

    let seeded_source_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifact_sources WHERE artifact_id = $1",
    )
    .bind(owner_artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count seeded provenance");
    assert_eq!(seeded_source_count, 5);

    PostgresLibraryRepository::new(pool.clone())
        .delete_library(LibraryId(library_id))
        .await
        .expect("library deletion should cascade every owned edge and scope");

    let remaining_sources = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifact_sources WHERE artifact_id = $1",
    )
    .bind(owner_artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count remaining provenance");
    assert_eq!(remaining_sources, 0);

    let owner_artifact_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifacts WHERE id = $1",
    )
    .bind(owner_artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count preserved owner artifact");
    assert_eq!(owner_artifact_count, 1);

    let collection_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM collection_definitions WHERE id = $1",
    )
    .bind(collection_id)
    .fetch_one(&pool)
    .await
    .expect("count library-scoped collection");
    assert_eq!(collection_count, 0);

    let placement_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM collection_shelf_placements WHERE id = $1",
    )
    .bind(placement_id)
    .fetch_one(&pool)
    .await
    .expect("count library-scoped shelf placement");
    assert_eq!(placement_count, 0);
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn deleting_user_cascades_owned_provenance_and_scoped_collections(
    pool: PgPool,
) {
    let library_id = Uuid::now_v7();
    let deleted_user_id = Uuid::now_v7();
    let retained_user_id = Uuid::now_v7();
    let owner_artifact_id = Uuid::now_v7();
    let related_artifact_id = Uuid::now_v7();
    let media_id = Uuid::now_v7();
    let media_context_id = Uuid::now_v7();
    let search_document_id = Uuid::now_v7();
    let collection_id = Uuid::now_v7();
    let placement_id = Uuid::now_v7();
    let deleted_host_session_id = Uuid::now_v7();
    let retained_host_session_id = Uuid::now_v7();
    let history_id = Uuid::now_v7();
    let permission_id = Uuid::now_v7();
    let role_id = Uuid::now_v7();

    seed_library(&pool, library_id, "retain-user-delete-library").await;
    seed_user(&pool, deleted_user_id, "deletecascadeuser").await;
    seed_user(&pool, retained_user_id, "retaincascadeuser").await;
    seed_artifact(
        &pool,
        owner_artifact_id,
        None,
        None,
        "Preserved global artifact",
    )
    .await;
    seed_artifact(
        &pool,
        related_artifact_id,
        Some(library_id),
        Some(deleted_user_id),
        "Deleted user artifact",
    )
    .await;
    seed_media_context(
        &pool,
        media_context_id,
        library_id,
        Some(deleted_user_id),
        media_id,
    )
    .await;
    seed_search_document(
        &pool,
        search_document_id,
        library_id,
        Some(deleted_user_id),
        media_id,
    )
    .await;
    seed_user_scoped_collection(
        &pool,
        collection_id,
        placement_id,
        deleted_user_id,
    )
    .await;

    for (session_id, room_code, host_id) in [
        (deleted_host_session_id, "DEL001", deleted_user_id),
        (retained_host_session_id, "KEEP01", retained_user_id),
    ] {
        sqlx::query(
            r#"
            INSERT INTO sync_sessions (
                id, room_code, host_id, expires_at, media_uuid, media_type
            )
            VALUES ($1, $2, $3, now() + interval '1 hour', $4, 0)
            "#,
        )
        .bind(session_id)
        .bind(room_code)
        .bind(host_id)
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .expect("seed sync session");
    }

    for (session_id, user_id) in [
        (deleted_host_session_id, retained_user_id),
        (retained_host_session_id, deleted_user_id),
    ] {
        sqlx::query(
            "INSERT INTO sync_participants (session_id, user_id) VALUES ($1, $2)",
        )
        .bind(session_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed sync participant");
    }

    sqlx::query(
        r#"
        INSERT INTO sync_session_history (
            id, session_id, event_type, event_data, user_id
        )
        VALUES ($1, $2, 'participant_left', '{}'::jsonb, $3)
        "#,
    )
    .bind(history_id)
    .bind(retained_host_session_id)
    .bind(deleted_user_id)
    .execute(&pool)
    .await
    .expect("seed sync-session history");

    sqlx::query(
        r#"
        INSERT INTO permissions (id, name, category)
        VALUES ($1, $2, 'deletion-regression')
        "#,
    )
    .bind(permission_id)
    .bind(format!("deletion-regression-{permission_id}"))
    .execute(&pool)
    .await
    .expect("seed permission");

    sqlx::query(
        r#"
        INSERT INTO user_permissions (
            user_id, permission_id, granted, granted_by
        )
        VALUES ($1, $2, true, $3)
        "#,
    )
    .bind(retained_user_id)
    .bind(permission_id)
    .bind(deleted_user_id)
    .execute(&pool)
    .await
    .expect("seed permission grant");

    sqlx::query(
        r#"
        INSERT INTO roles (id, name, is_system)
        VALUES ($1, $2, false)
        "#,
    )
    .bind(role_id)
    .bind(format!("del-{role_id}"))
    .execute(&pool)
    .await
    .expect("seed role");

    sqlx::query(
        r#"
        INSERT INTO user_roles (user_id, role_id, granted_by)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(retained_user_id)
    .bind(role_id)
    .bind(deleted_user_id)
    .execute(&pool)
    .await
    .expect("seed role grant");

    sqlx::query(
        r#"
        INSERT INTO user_episode_state (
            user_id, tmdb_series_id, season_number, episode_number,
            position, duration, last_watched
        )
        VALUES ($1, 12345, 1, 1, 30.0, 60.0, 1)
        "#,
    )
    .bind(deleted_user_id)
    .execute(&pool)
    .await
    .expect("seed episode watch state");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_media_context_id
        )
        VALUES ($1, 0, 'media_context', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(media_context_id)
    .execute(&pool)
    .await
    .expect("seed user media-context provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind,
            source_search_document_id
        )
        VALUES ($1, 1, 'search_document', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(search_document_id)
    .execute(&pool)
    .await
    .expect("seed user search-document provenance");

    sqlx::query(
        r#"
        INSERT INTO intelligence_artifact_sources (
            artifact_id, source_ordinal, source_kind, source_artifact_id
        )
        VALUES ($1, 2, 'artifact', $2)
        "#,
    )
    .bind(owner_artifact_id)
    .bind(related_artifact_id)
    .execute(&pool)
    .await
    .expect("seed user related-artifact provenance");

    PostgresUsersRepository::new(pool.clone())
        .delete_user(deleted_user_id)
        .await
        .expect("user deletion should cascade every owned edge and scope");

    let remaining_sources = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifact_sources WHERE artifact_id = $1",
    )
    .bind(owner_artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count remaining user provenance");
    assert_eq!(remaining_sources, 0);

    let owner_artifact_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM intelligence_artifacts WHERE id = $1",
    )
    .bind(owner_artifact_id)
    .fetch_one(&pool)
    .await
    .expect("count preserved global artifact");
    assert_eq!(owner_artifact_count, 1);

    let retained_user_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM users WHERE id = $1",
    )
    .bind(retained_user_id)
    .fetch_one(&pool)
    .await
    .expect("count retained user");
    assert_eq!(retained_user_count, 1);

    let retained_library_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM libraries WHERE id = $1",
    )
    .bind(library_id)
    .fetch_one(&pool)
    .await
    .expect("count retained library");
    assert_eq!(retained_library_count, 1);

    let collection_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM collection_definitions WHERE id = $1",
    )
    .bind(collection_id)
    .fetch_one(&pool)
    .await
    .expect("count user-owned collection");
    assert_eq!(collection_count, 0);

    let placement_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM collection_shelf_placements WHERE id = $1",
    )
    .bind(placement_id)
    .fetch_one(&pool)
    .await
    .expect("count user-scoped shelf placement");
    assert_eq!(placement_count, 0);

    let deleted_host_session_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM sync_sessions WHERE id = $1",
    )
    .bind(deleted_host_session_id)
    .fetch_one(&pool)
    .await
    .expect("count deleted host session");
    assert_eq!(deleted_host_session_count, 0);

    let retained_host_session_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM sync_sessions WHERE id = $1",
    )
    .bind(retained_host_session_id)
    .fetch_one(&pool)
    .await
    .expect("count retained host session");
    assert_eq!(retained_host_session_count, 1);

    let stale_participant_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT count(*)
        FROM sync_participants
        WHERE user_id = $1 OR session_id = $2
        "#,
    )
    .bind(deleted_user_id)
    .bind(deleted_host_session_id)
    .fetch_one(&pool)
    .await
    .expect("count stale sync participants");
    assert_eq!(stale_participant_count, 0);

    let history_actor = sqlx::query_scalar::<_, Option<Uuid>>(
        "SELECT user_id FROM sync_session_history WHERE id = $1",
    )
    .bind(history_id)
    .fetch_one(&pool)
    .await
    .expect("load retained sync-session history actor");
    assert_eq!(history_actor, None);

    let permission_grantor = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        SELECT granted_by
        FROM user_permissions
        WHERE user_id = $1 AND permission_id = $2
        "#,
    )
    .bind(retained_user_id)
    .bind(permission_id)
    .fetch_one(&pool)
    .await
    .expect("load retained permission grantor");
    assert_eq!(permission_grantor, None);

    let role_grantor = sqlx::query_scalar::<_, Option<Uuid>>(
        r#"
        SELECT granted_by
        FROM user_roles
        WHERE user_id = $1 AND role_id = $2
        "#,
    )
    .bind(retained_user_id)
    .bind(role_id)
    .fetch_one(&pool)
    .await
    .expect("load retained role grantor");
    assert_eq!(role_grantor, None);

    let episode_state_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM user_episode_state WHERE user_id = $1",
    )
    .bind(deleted_user_id)
    .fetch_one(&pool)
    .await
    .expect("count deleted episode state");
    assert_eq!(episode_state_count, 0);
}
