use std::net::SocketAddr;

use anyhow::Result;
use axum::{Router, http::StatusCode};
use axum_test::TestServer;
use ferrex_core::{
    api::{routes, routes::utils as route_utils, types::collections::*},
    types::{MediaID, MovieID},
};
use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn build_server(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    Ok((server, state, tempdir))
}

async fn register_user(
    server: &TestServer,
    username: &str,
) -> Result<(Uuid, String)> {
    let response = server
        .post(routes::v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": format!("{username} display"),
            "password": "Password#123"
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    let user_id = Uuid::parse_str(
        body["data"]["user_id"]
            .as_str()
            .expect("register returns user_id"),
    )?;
    let access_token = body["data"]["access_token"]
        .as_str()
        .expect("register returns access token")
        .to_string();

    Ok((user_id, access_token))
}

async fn grant_admin(state: &AppState, user_id: Uuid) -> Result<()> {
    let roles = state.unit_of_work().rbac.get_all_roles().await?;
    let admin_role = roles
        .into_iter()
        .find(|role| role.name == "admin")
        .expect("admin role is seeded");
    state
        .unit_of_work()
        .rbac
        .assign_user_role(user_id, admin_role.id, user_id)
        .await?;
    Ok(())
}

fn user_collection_request(
    title: &str,
    owner_user_id: Uuid,
    visibility: CollectionVisibility,
) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: None,
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: CollectionOwner {
            owner_type: CollectionOwnerType::User,
            user_id: Some(owner_user_id),
            device_id: None,
            display_name: Some(format!("owner-{owner_user_id}")),
        },
        scope: CollectionScope::User,
        visibility,
        presentation: CollectionPresentationMode::Shelf,
        media_scope: CollectionMediaScope::All,
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
}

fn system_collection_request(
    title: &str,
    kind: CollectionKind,
    source: CollectionSource,
) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: Some("imported/system fixture".to_string()),
        kind,
        source,
        owner: CollectionOwner::default(),
        scope: CollectionScope::Global,
        visibility: CollectionVisibility::System,
        presentation: CollectionPresentationMode::Shelf,
        media_scope: CollectionMediaScope::All,
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: Some(CollectionProvenance {
            source,
            imported_from: Some("route-test".to_string()),
            external_id: Some(title.to_string()),
            generated_by: None,
            rule_hash: None,
            last_refreshed_at: None,
        }),
        rule: None,
    }
}

async fn seed_movie_library(pool: &PgPool) -> Result<Uuid> {
    let library_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")?;
    sqlx::query(
        r#"
        INSERT INTO libraries (
            id, name, library_type, paths, scan_interval_minutes, enabled,
            auto_scan, watch_for_changes, analyze_on_scan, max_retry_attempts
        ) VALUES ($1, 'Collection Route Movies', 'movies', ARRAY['/fixture/collections'], 60, TRUE, TRUE, TRUE, FALSE, 3)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(library_id)
    .execute(pool)
    .await?;
    Ok(library_id)
}

async fn seed_available_movie(
    pool: &PgPool,
    library_id: Uuid,
    movie_id: Uuid,
    file_id: Uuid,
    title: &str,
) -> Result<()> {
    let file_path = format!("/fixture/collections/{file_id}.mkv");
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename,
            file_size, is_available
        ) VALUES ($1, $2, $3, 'movie', $4, $5, 100, TRUE)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(&file_path)
    .bind(format!("{title}.mkv"))
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, batch_id)
        VALUES ($1, $2, $3, $4, $5, 1)
        "#,
    )
    .bind(movie_id)
    .bind(library_id)
    .bind(file_id)
    .bind((movie_id.as_u128() % 10_000_000) as i64)
    .bind(title)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_shelf_placement(
    pool: &PgPool,
    collection: &CollectionDetail,
    surface: &str,
    shelf_key: &str,
    position: u32,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO collection_shelf_placements (
            id, collection_id, collection_stable_key, surface, shelf_key,
            placement_scope, placement_scope_key, visibility, presentation,
            pinned, position, position_key
        ) VALUES (
            $1, $2, $3, $4, $5, 'global', 'global',
            'public', 'shelf', TRUE, $6, ($6::text)::numeric
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(collection.summary.identity.id.to_uuid())
    .bind(&collection.summary.identity.stable_key)
    .bind(surface)
    .bind(shelf_key)
    .bind(i32::try_from(position)?)
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed_home_shelf(
    pool: &PgPool,
    collection: &CollectionDetail,
) -> Result<()> {
    seed_shelf_placement(pool, collection, "home", "home:pinned", 0).await
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_read_routes_require_auth_and_enforce_visibility(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool).await?;
    let (owner_id, owner_token) = register_user(&server, "coll_owner").await?;
    let (_other_id, other_token) = register_user(&server, "coll_other").await?;

    let private = state
        .unit_of_work()
        .collections
        .create_collection(user_collection_request(
            "Private Owner Shelf",
            owner_id,
            CollectionVisibility::Private,
        ))
        .await?;

    server
        .get(routes::v1::collections::COLLECTION)
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    let owner_list = server
        .get(routes::v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&owner_token))
        .await;
    owner_list.assert_status_ok();
    let body: Value = owner_list.json();
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 1);

    let other_list = server
        .get(routes::v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&other_token))
        .await;
    other_list.assert_status_ok();
    let body: Value = other_list.json();
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 0);

    let detail_path = route_utils::replace_param(
        routes::v1::collections::ITEM,
        "{collection_id}",
        private.summary.identity.id.to_string(),
    );
    server
        .get(&detail_path)
        .add_header("Authorization", bearer(&other_token))
        .await
        .assert_status(StatusCode::NOT_FOUND);

    server
        .get(routes::v1::collections::COLLECTION)
        .add_query_param("include_archived", "true")
        .add_header("Authorization", bearer(&owner_token))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_detail_hides_admin_shelf_placements_from_non_admin_users(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let (owner_id, _owner_token) =
        register_user(&server, "detail_owner").await?;
    let (_viewer_id, viewer_token) =
        register_user(&server, "detail_viewer").await?;

    let collection = state
        .unit_of_work()
        .collections
        .create_collection(user_collection_request(
            "Public Detail Shelf",
            owner_id,
            CollectionVisibility::Public,
        ))
        .await?;
    seed_shelf_placement(&pool, &collection, "home", "home:detail", 0).await?;
    seed_shelf_placement(&pool, &collection, "admin", "admin:detail", 1)
        .await?;

    let detail_path = route_utils::replace_param(
        routes::v1::collections::ITEM,
        "{collection_id}",
        collection.summary.identity.id.to_string(),
    );
    let detail = server
        .get(&detail_path)
        .add_query_param("include_shelf_placements", "true")
        .add_header("Authorization", bearer(&viewer_token))
        .await;
    detail.assert_status_ok();
    let body: Value = detail.json();
    let placements = body["data"]["collection"]["shelf_placements"]
        .as_array()
        .unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0]["surface"], "home");
    assert!(
        placements
            .iter()
            .all(|placement| placement["surface"] != "admin")
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn shelf_reads_hide_admin_surfaces_from_non_admin_users(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let (user_id, user_token) = register_user(&server, "shelf_user").await?;
    let (admin_id, admin_token) = register_user(&server, "shelf_admin").await?;
    grant_admin(&state, admin_id).await?;

    let home_collection = state
        .unit_of_work()
        .collections
        .create_collection(user_collection_request(
            "Public Home Shelf",
            user_id,
            CollectionVisibility::Public,
        ))
        .await?;
    seed_shelf_placement(&pool, &home_collection, "home", "home:pinned", 0)
        .await?;

    let admin_collection = state
        .unit_of_work()
        .collections
        .create_collection(user_collection_request(
            "Public Admin Shelf",
            user_id,
            CollectionVisibility::Public,
        ))
        .await?;
    seed_shelf_placement(&pool, &admin_collection, "admin", "admin:pinned", 1)
        .await?;

    let user_shelves = server
        .get(routes::v1::shelves::PLACEMENTS)
        .add_header("Authorization", bearer(&user_token))
        .await;
    user_shelves.assert_status_ok();
    let body: Value = user_shelves.json();
    let placements = body["data"]["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0]["surface"], "home");

    server
        .get(routes::v1::shelves::PLACEMENTS)
        .add_query_param("surface", "admin")
        .add_header("Authorization", bearer(&user_token))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .get(routes::v1::collections::COLLECTION)
        .add_query_param("shelf_surface", "admin")
        .add_header("Authorization", bearer(&user_token))
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let user_admin_key_filter = server
        .get(routes::v1::collections::COLLECTION)
        .add_query_param("shelf_key", "admin:pinned")
        .add_header("Authorization", bearer(&user_token))
        .await;
    user_admin_key_filter.assert_status_ok();
    let body: Value = user_admin_key_filter.json();
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 0);

    let admin_shelves = server
        .get(routes::v1::shelves::PLACEMENTS)
        .add_query_param("surface", "admin")
        .add_header("Authorization", bearer(&admin_token))
        .await;
    admin_shelves.assert_status_ok();
    let body: Value = admin_shelves.json();
    let placements = body["data"]["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0]["surface"], "admin");

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_reads_filter_imported_shelves_and_items(
    pool: PgPool,
) -> Result<()> {
    let library_id = seed_movie_library(&pool).await?;
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let (admin_id, admin_token) = register_user(&server, "coll_admin").await?;
    grant_admin(&state, admin_id).await?;

    let imported = state
        .unit_of_work()
        .collections
        .create_collection(system_collection_request(
            "Imported TMDB List",
            CollectionKind::TmdbList,
            CollectionSource::Imported,
        ))
        .await?;
    seed_home_shelf(&pool, &imported).await?;

    let imported_list = server
        .get(routes::v1::collections::COLLECTION)
        .add_query_param("kind", "tmdb_list")
        .add_query_param("source", "imported")
        .add_header("Authorization", bearer(&admin_token))
        .await;
    imported_list.assert_status_ok();
    let body: Value = imported_list.json();
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["collections"][0]["source"], "imported");

    let shelf_filtered = server
        .get(routes::v1::collections::COLLECTION)
        .add_query_param("shelf_surface", "home")
        .add_query_param("pinned", "true")
        .add_header("Authorization", bearer(&admin_token))
        .await;
    shelf_filtered.assert_status_ok();
    let body: Value = shelf_filtered.json();
    assert_eq!(body["data"]["collections"].as_array().unwrap().len(), 1);

    let shelves = server
        .get(routes::v1::shelves::PLACEMENTS)
        .add_query_param("surface", "home")
        .add_header("Authorization", bearer(&admin_token))
        .await;
    shelves.assert_status_ok();
    let body: Value = shelves.json();
    assert_eq!(body["data"]["placements"].as_array().unwrap().len(), 1);

    let items_collection = state
        .unit_of_work()
        .collections
        .create_collection(user_collection_request(
            "Availability Items",
            admin_id,
            CollectionVisibility::Public,
        ))
        .await?;
    let movie_id = Uuid::parse_str("30000000-0000-7000-8000-000000000001")?;
    seed_available_movie(
        &pool,
        library_id,
        movie_id,
        Uuid::parse_str("40000000-0000-7000-8000-000000000001")?,
        "Available Route Movie",
    )
    .await?;
    let missing_movie_id =
        Uuid::parse_str("30000000-0000-7000-8000-000000000002")?;
    state
        .unit_of_work()
        .collections
        .manual_add_collection_items(
            items_collection.summary.identity.id,
            ManualAddCollectionItemsRequest {
                items: vec![
                    CollectionManualAddItem {
                        media_id: MediaID::Movie(MovieID(movie_id)),
                        title_override: None,
                        position: None,
                    },
                    CollectionManualAddItem {
                        media_id: MediaID::Movie(MovieID(missing_movie_id)),
                        title_override: Some("Missing Route Movie".to_string()),
                        position: None,
                    },
                ],
                duplicate_policy: None,
                expected_revision: None,
            },
            Some(admin_id),
        )
        .await?;

    let items_path = route_utils::replace_param(
        routes::v1::collections::ITEMS,
        "{collection_id}",
        items_collection.summary.identity.id.to_string(),
    );
    let normal_items = server
        .get(&items_path)
        .add_header("Authorization", bearer(&admin_token))
        .await;
    normal_items.assert_status_ok();
    let body: Value = normal_items.json();
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["page"]["total"], 1);
    assert!(body["data"]["version"]["revision"].as_u64().unwrap() >= 1);

    let admin_missing_items = server
        .get(&items_path)
        .add_query_param("mode", "admin")
        .add_query_param("availability", "missing")
        .add_header("Authorization", bearer(&admin_token))
        .await;
    admin_missing_items.assert_status_ok();
    let body: Value = admin_missing_items.json();
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["data"]["items"][0]["availability"]["status"],
        "missing"
    );

    Ok(())
}
