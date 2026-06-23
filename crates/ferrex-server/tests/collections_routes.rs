use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use ferrex_core::api::routes::{utils::replace_param, v1};
use ferrex_core::api::types::collections::*;
use ferrex_core::types::{MediaID, MovieID};
use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use tempfile::TempDir;
use uuid::Uuid;

mod common;
use common::build_test_app_with_hooks;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn data_field<'a>(body: &'a serde_json::Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} missing"))
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

async fn register_named_user(
    server: &TestServer,
    username: &str,
) -> Result<(Uuid, String)> {
    let response = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": format!("{username} display"),
            "password": "Password#123"
        }))
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let user_id = Uuid::parse_str(data_field(&body, "user_id"))?;
    Ok((user_id, data_field(&body, "access_token").to_string()))
}

async fn register_user(server: &TestServer) -> Result<String> {
    let (_user_id, token) =
        register_named_user(server, "collection_route_user").await?;
    Ok(token)
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

async fn seed_movie(
    pool: &PgPool,
    movie_id: Uuid,
    file_id: Uuid,
    tmdb_id: i64,
    title: &str,
    is_available: bool,
) -> Result<Uuid> {
    let library_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")?;

    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, 'collection routes', 'movies', ARRAY['/tmp'])
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(library_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename,
            file_size, is_available, tombstone_reason
        ) VALUES ($1, $2, $3, 'movie', $4, $5, 123, $6, $7)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(format!("/tmp/{file_id}.mkv"))
    .bind(format!("{file_id}.mkv"))
    .bind(is_available)
    .bind((!is_available).then_some("QA tombstone"))
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
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await?;

    Ok(movie_id)
}

async fn insert_preserved_tombstoned_member(
    pool: &PgPool,
    collection_id: Uuid,
    movie_id: Uuid,
    title: &str,
    position: u32,
) -> Result<()> {
    let media_id = MediaID::Movie(MovieID(movie_id));
    let item_key = CollectionMemberKey::for_media(&media_id);
    sqlx::query(
        r#"
        INSERT INTO collection_manual_memberships (
            collection_id,
            item_key,
            media_type,
            media_id,
            title_snapshot,
            position_key,
            availability_status,
            availability_reason,
            availability_checked_at
        ) VALUES (
            $1, $2, 'movie', $3, $4, ($5::text)::numeric,
            'tombstoned', 'QA tombstone', NOW()
        )
        "#,
    )
    .bind(collection_id)
    .bind(item_key.as_str())
    .bind(movie_id)
    .bind(title)
    .bind(position.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

fn manual_collection_request(title: &str) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: Some("Route parity collection".to_string()),
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: CollectionOwner::default(),
        scope: CollectionScope::User,
        visibility: CollectionVisibility::Private,
        presentation: CollectionPresentationMode::Grid,
        media_scope: CollectionMediaScope::All,
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
}

fn owned_manual_collection_request(
    title: &str,
    owner_id: Uuid,
    visibility: CollectionVisibility,
) -> CreateCollectionRequest {
    let mut request = manual_collection_request(title);
    request.owner = CollectionOwner {
        owner_type: CollectionOwnerType::User,
        user_id: Some(owner_id),
        device_id: None,
        display_name: Some(format!("owner-{owner_id}")),
    };
    request.visibility = visibility;
    request
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_routes_enforce_auth_and_cover_manual_flow(
    pool: PgPool,
) -> Result<()> {
    let first_movie = seed_movie(
        &pool,
        Uuid::parse_str("11111111-1111-4111-8111-111111111111")?,
        Uuid::parse_str("22222222-2222-4222-8222-222222222222")?,
        4242,
        "First Route Movie",
        true,
    )
    .await?;
    let second_movie = seed_movie(
        &pool,
        Uuid::parse_str("33333333-3333-4333-8333-333333333333")?,
        Uuid::parse_str("44444444-4444-4444-8444-444444444444")?,
        4343,
        "Second Route Movie",
        true,
    )
    .await?;
    let tombstoned_movie = seed_movie(
        &pool,
        Uuid::parse_str("55555555-5555-4555-8555-555555555555")?,
        Uuid::parse_str("66666666-6666-4666-8666-666666666666")?,
        4444,
        "Tombstoned Route Movie",
        false,
    )
    .await?;
    let (server, _state, _tempdir) = build_server(pool.clone()).await?;

    let unauthenticated = server.get(v1::collections::COLLECTION).await;
    unauthenticated.assert_status(StatusCode::UNAUTHORIZED);

    let token = register_user(&server).await?;
    let create = server
        .post(v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&token))
        .json(&manual_collection_request("Route collection"))
        .await;
    create.assert_status_ok();
    let body: serde_json::Value = create.json();
    let collection_id = Uuid::parse_str(
        body["data"]["collection"]["summary"]["identity"]["id"]
            .as_str()
            .expect("collection id"),
    )?;

    let item_path = replace_param(
        v1::collections::ITEMS,
        "{collection_id}",
        collection_id.to_string(),
    );
    let add_path = replace_param(
        v1::collections::MANUAL_ADD_ITEMS,
        "{collection_id}",
        collection_id.to_string(),
    );
    let add = server
        .post(&add_path)
        .add_header("Authorization", bearer(&token))
        .json(&ManualAddCollectionItemsRequest {
            items: vec![
                CollectionManualAddItem {
                    media_id: MediaID::Movie(MovieID(first_movie)),
                    title_override: None,
                    position: None,
                },
                CollectionManualAddItem {
                    media_id: MediaID::Movie(MovieID(second_movie)),
                    title_override: None,
                    position: None,
                },
            ],
            duplicate_policy: None,
            expected_revision: Some(0),
        })
        .await;
    add.assert_status_ok();
    let body: serde_json::Value = add.json();
    assert_eq!(body["data"]["results"][0]["status"].as_str(), Some("added"));
    assert_eq!(body["data"]["results"][1]["status"].as_str(), Some("added"));

    let duplicate = server
        .post(&add_path)
        .add_header("Authorization", bearer(&token))
        .json(&ManualAddCollectionItemsRequest {
            items: vec![CollectionManualAddItem {
                media_id: MediaID::Movie(MovieID(first_movie)),
                title_override: None,
                position: None,
            }],
            duplicate_policy: None,
            expected_revision: Some(1),
        })
        .await;
    duplicate.assert_status_ok();
    let body: serde_json::Value = duplicate.json();
    assert_eq!(
        body["data"]["results"][0]["status"].as_str(),
        Some("duplicate_skipped")
    );

    insert_preserved_tombstoned_member(
        &pool,
        collection_id,
        tombstoned_movie,
        "Tombstoned Route Movie",
        3,
    )
    .await?;

    let paged_items = server
        .get(&format!("{item_path}?limit=1"))
        .add_header("Authorization", bearer(&token))
        .await;
    paged_items.assert_status_ok();
    let body: serde_json::Value = paged_items.json();
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["items"][0]["title"], "First Route Movie");
    assert_eq!(body["data"]["page"]["next_cursor"], "1");

    let available_items = server
        .get(&format!("{item_path}?availability=available"))
        .add_header("Authorization", bearer(&token))
        .await;
    available_items.assert_status_ok();
    let body: serde_json::Value = available_items.json();
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 2);

    let tombstoned_items = server
        .get(&format!("{item_path}?availability=tombstoned"))
        .add_header("Authorization", bearer(&token))
        .await;
    tombstoned_items.assert_status_ok();
    let body: serde_json::Value = tombstoned_items.json();
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["items"][0]["title"], "Tombstoned Route Movie");
    assert_eq!(
        body["data"]["items"][0]["availability"]["status"],
        "tombstoned"
    );

    let stale_update_path = replace_param(
        v1::collections::ITEM,
        "{collection_id}",
        collection_id.to_string(),
    );
    let stale_update = server
        .put(&stale_update_path)
        .add_header("Authorization", bearer(&token))
        .json(&UpdateCollectionRequest {
            title: Some("Stale".to_string()),
            expected_revision: Some(0),
            ..Default::default()
        })
        .await;
    stale_update.assert_status(StatusCode::CONFLICT);

    let archive_path = replace_param(
        v1::collections::ARCHIVE,
        "{collection_id}",
        collection_id.to_string(),
    );
    let archive = server
        .post(&archive_path)
        .add_header("Authorization", bearer(&token))
        .json(&ArchiveCollectionRequest {
            expected_revision: Some(1),
            ..ArchiveCollectionRequest::default()
        })
        .await;
    archive.assert_status_ok();

    let delete = server
        .delete(&stale_update_path)
        .add_header("Authorization", bearer(&token))
        .json(&DeleteCollectionRequest {
            reason: Some("route cleanup".to_string()),
            expected_revision: Some(2),
        })
        .await;
    delete.assert_status_ok();

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_mutation_routes_require_collection_owner_or_admin(
    pool: PgPool,
) -> Result<()> {
    let first_movie = seed_movie(
        &pool,
        Uuid::parse_str("11111111-1111-4111-8111-111111111112")?,
        Uuid::parse_str("22222222-2222-4222-8222-222222222223")?,
        5252,
        "Owner Route Movie",
        true,
    )
    .await?;
    let second_movie = seed_movie(
        &pool,
        Uuid::parse_str("33333333-3333-4333-8333-333333333334")?,
        Uuid::parse_str("44444444-4444-4444-8444-444444444445")?,
        5353,
        "Other Route Movie",
        true,
    )
    .await?;
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let (owner_id, owner_token) =
        register_named_user(&server, "collection_owner").await?;
    let (_other_id, other_token) =
        register_named_user(&server, "collection_intruder").await?;
    let (admin_id, admin_token) =
        register_named_user(&server, "collection_admin").await?;
    grant_admin(&state, admin_id).await?;

    let collection = state
        .unit_of_work()
        .collections
        .create_collection(owned_manual_collection_request(
            "Public owner collection",
            owner_id,
            CollectionVisibility::Public,
        ))
        .await?;
    let collection_id = collection.summary.identity.id;
    let item_key =
        CollectionMemberKey::for_media(&MediaID::Movie(MovieID(first_movie)));
    state
        .unit_of_work()
        .collections
        .manual_add_collection_items(
            collection_id,
            ManualAddCollectionItemsRequest {
                items: vec![CollectionManualAddItem {
                    media_id: MediaID::Movie(MovieID(first_movie)),
                    title_override: None,
                    position: None,
                }],
                duplicate_policy: None,
                expected_revision: None,
            },
            Some(owner_id),
        )
        .await?;

    let item_path = replace_param(
        v1::collections::ITEM,
        "{collection_id}",
        collection_id.to_string(),
    );
    let archive_path = replace_param(
        v1::collections::ARCHIVE,
        "{collection_id}",
        collection_id.to_string(),
    );
    let add_path = replace_param(
        v1::collections::MANUAL_ADD_ITEMS,
        "{collection_id}",
        collection_id.to_string(),
    );
    let remove_path = replace_param(
        v1::collections::MANUAL_REMOVE_ITEMS,
        "{collection_id}",
        collection_id.to_string(),
    );
    let reorder_path = replace_param(
        v1::collections::MANUAL_REORDER_ITEMS,
        "{collection_id}",
        collection_id.to_string(),
    );
    let refresh_path = replace_param(
        v1::collections::RULE_REFRESH,
        "{collection_id}",
        collection_id.to_string(),
    );

    server
        .put(&item_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&UpdateCollectionRequest {
            title: Some("Intruder rename".to_string()),
            ..Default::default()
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(&archive_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&ArchiveCollectionRequest::default())
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .delete(&item_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&DeleteCollectionRequest::default())
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(&add_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&ManualAddCollectionItemsRequest {
            items: vec![CollectionManualAddItem {
                media_id: MediaID::Movie(MovieID(second_movie)),
                title_override: None,
                position: None,
            }],
            duplicate_policy: None,
            expected_revision: None,
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(&remove_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&ManualRemoveCollectionItemsRequest {
            item_keys: vec![item_key.clone()],
            expected_revision: None,
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(&reorder_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&ManualReorderCollectionItemsRequest {
            ordering: vec![CollectionManualOrder {
                item_key: item_key.clone(),
                position: 0,
            }],
            expected_revision: None,
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(&refresh_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&RefreshCollectionRuleRequest::default())
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let pin_request = PinShelfPlacementRequest {
        collection_id,
        surface: ShelfSurface::Home,
        shelf_key: "home:owner-route".to_string(),
        pinned: true,
        position: Some(0),
        presentation: None,
    };
    server
        .post(v1::shelves::PIN_PLACEMENT)
        .add_header("Authorization", bearer(&other_token))
        .json(&pin_request)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let owner_pin = server
        .post(v1::shelves::PIN_PLACEMENT)
        .add_header("Authorization", bearer(&owner_token))
        .json(&pin_request)
        .await;
    owner_pin.assert_status_ok();
    let body: serde_json::Value = owner_pin.json();
    let placement_id = ShelfPlacementId::from(Uuid::parse_str(
        body["data"]["placement"]["id"]
            .as_str()
            .expect("placement id"),
    )?);

    server
        .post(v1::shelves::REORDER_PLACEMENTS)
        .add_header("Authorization", bearer(&other_token))
        .json(&ReorderShelfPlacementsRequest {
            ordering: vec![ShelfPlacementOrder {
                placement_id,
                position: 0,
            }],
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let private_collection = state
        .unit_of_work()
        .collections
        .create_collection(owned_manual_collection_request(
            "Private owner collection",
            owner_id,
            CollectionVisibility::Private,
        ))
        .await?;
    let private_path = replace_param(
        v1::collections::ITEM,
        "{collection_id}",
        private_collection.summary.identity.id.to_string(),
    );
    server
        .put(&private_path)
        .add_header("Authorization", bearer(&other_token))
        .json(&UpdateCollectionRequest {
            title: Some("Hidden rename".to_string()),
            ..Default::default()
        })
        .await
        .assert_status(StatusCode::NOT_FOUND);

    server
        .put(&item_path)
        .add_header("Authorization", bearer(&admin_token))
        .json(&UpdateCollectionRequest {
            title: Some("Admin rename".to_string()),
            ..Default::default()
        })
        .await
        .assert_status_ok();

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_create_and_import_routes_constrain_caller_scope(
    pool: PgPool,
) -> Result<()> {
    let (server, _state, _tempdir) = build_server(pool).await?;
    let (user_id, user_token) =
        register_named_user(&server, "collection_creator").await?;
    let (other_id, _other_token) =
        register_named_user(&server, "collection_target").await?;

    let create = server
        .post(v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&user_token))
        .json(&manual_collection_request("Owned default collection"))
        .await;
    create.assert_status_ok();
    let body: serde_json::Value = create.json();
    assert_eq!(
        body["data"]["collection"]["summary"]["owner"]["user_id"],
        user_id.to_string()
    );
    assert_eq!(
        body["data"]["collection"]["summary"]["visibility"],
        "private"
    );

    let mut other_owner = manual_collection_request("Spoofed owner");
    other_owner.owner = CollectionOwner {
        owner_type: CollectionOwnerType::User,
        user_id: Some(other_id),
        device_id: None,
        display_name: Some("target".to_string()),
    };
    server
        .post(v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&user_token))
        .json(&other_owner)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let mut global_scope = manual_collection_request("Global spoof");
    global_scope.scope = CollectionScope::Global;
    server
        .post(v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&user_token))
        .json(&global_scope)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    let mut shared_visibility = manual_collection_request("Shared spoof");
    shared_visibility.visibility = CollectionVisibility::Shared;
    server
        .post(v1::collections::COLLECTION)
        .add_header("Authorization", bearer(&user_token))
        .json(&shared_visibility)
        .await
        .assert_status(StatusCode::FORBIDDEN);

    server
        .post(v1::collections::tmdb::IMPORT)
        .add_header("Authorization", bearer(&user_token))
        .json(&TmdbImportCollectionRequest {
            tmdb_id: "12345".to_string(),
            import_kind: TmdbCollectionImportKind::Collection,
            title_override: None,
            owner: CollectionOwner {
                owner_type: CollectionOwnerType::User,
                user_id: Some(other_id),
                device_id: None,
                display_name: Some("target".to_string()),
            },
            visibility: CollectionVisibility::Public,
            presentation: CollectionPresentationMode::Shelf,
            duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
            media_scope: CollectionMediaScope::All,
            refresh_existing: false,
        })
        .await
        .assert_status(StatusCode::FORBIDDEN);

    Ok(())
}
