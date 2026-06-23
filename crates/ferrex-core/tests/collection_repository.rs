//! Postgres-backed collection repository behavior tests.

#![cfg(feature = "database")]

use anyhow::Result;
use ferrex_core::api::types::collections::{
    ArchiveCollectionRequest, CollectionDuplicatePolicy, CollectionId,
    CollectionMediaKind, CollectionMediaScope,
    CollectionMemberAvailabilityStatus, CollectionOwner,
    CollectionPresentationMode, CollectionScope, CollectionSource,
    CollectionVisibility, CreateCollectionRequest, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionsRequest,
    UpdateCollectionRequest,
};
use ferrex_core::database::repositories::collections::PostgresCollectionRepository;
use ferrex_core::database::repository_ports::collections::{
    CollectionItemIdentity, CollectionReadMode, CollectionRepository,
};
use ferrex_core::error::MediaError;
use ferrex_core::types::{
    EpisodeID, LibraryId, MediaID, MovieID, SeasonID, SeriesID,
};
use sqlx::PgPool;
use uuid::Uuid;

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("valid uuid")
}

fn movie_library_id() -> LibraryId {
    LibraryId(uuid("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"))
}

fn tv_library_id() -> LibraryId {
    LibraryId(uuid("cccccccc-cccc-cccc-cccc-cccccccccccc"))
}

fn create_request(title: &str) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: None,
        kind: Default::default(),
        source: CollectionSource::Manual,
        owner: CollectionOwner::default(),
        scope: CollectionScope::User,
        visibility: CollectionVisibility::Private,
        presentation: CollectionPresentationMode::Shelf,
        media_scope: CollectionMediaScope::All,
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
}

async fn seed_movie(
    pool: &PgPool,
    movie_id: Uuid,
    file_id: Uuid,
    title: &str,
    available: bool,
) -> Result<()> {
    let library_id = movie_library_id();
    let file_path = format!("/fixture/collections/{file_id}.mkv");
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id,
            library_id,
            media_id,
            media_type,
            file_path,
            filename,
            file_size,
            is_available,
            tombstoned_at,
            tombstone_reason
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, 100, $6,
                CASE WHEN $6 THEN NULL ELSE NOW() END,
                CASE WHEN $6 THEN NULL ELSE 'fixture tombstone' END)
        "#,
    )
    .bind(file_id)
    .bind(library_id.as_uuid())
    .bind(movie_id)
    .bind(&file_path)
    .bind(format!("{title}.mkv"))
    .bind(available)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title, batch_id)
        VALUES ($1, $2, $3, $4, $5, 1)
        "#,
    )
    .bind(movie_id)
    .bind(library_id.as_uuid())
    .bind(file_id)
    .bind((movie_id.as_u128() % 10_000_000) as i64)
    .bind(title)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_tv_episode(
    pool: &PgPool,
    series_id: Uuid,
    season_id: Uuid,
    episode_id: Uuid,
    file_id: Uuid,
    season_number: i16,
    episode_number: i16,
    series_title: &str,
    episode_title: &str,
    available: bool,
) -> Result<()> {
    let library_id = tv_library_id();
    sqlx::query(
        r#"
        INSERT INTO series (id, library_id, tmdb_id, title)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(series_id)
    .bind(library_id.as_uuid())
    .bind((series_id.as_u128() % 10_000_000) as i64)
    .bind(series_title)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO season_references (
            id,
            series_id,
            season_number,
            tmdb_series_id,
            library_id
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (series_id, season_number) DO NOTHING
        "#,
    )
    .bind(season_id)
    .bind(series_id)
    .bind(season_number)
    .bind((series_id.as_u128() % 10_000_000) as i64)
    .bind(library_id.as_uuid())
    .execute(pool)
    .await?;

    let file_path = format!("/fixture/collections/{file_id}.mkv");
    sqlx::query(
        r#"
        INSERT INTO media_files (
            id,
            library_id,
            media_id,
            media_type,
            file_path,
            filename,
            file_size,
            is_available,
            tombstoned_at,
            tombstone_reason
        )
        VALUES ($1, $2, $3, 'episode', $4, $5, 100, $6,
                CASE WHEN $6 THEN NULL ELSE NOW() END,
                CASE WHEN $6 THEN NULL ELSE 'fixture tombstone' END)
        "#,
    )
    .bind(file_id)
    .bind(library_id.as_uuid())
    .bind(episode_id)
    .bind(&file_path)
    .bind(format!("{episode_title}.mkv"))
    .bind(available)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO episode_references (
            id,
            series_id,
            season_id,
            file_id,
            season_number,
            episode_number,
            tmdb_series_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(episode_id)
    .bind(series_id)
    .bind(season_id)
    .bind(file_id)
    .bind(season_number)
    .bind(episode_number)
    .bind((series_id.as_u128() % 10_000_000) as i64)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO episode_metadata (episode_id, tmdb_id, name)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(episode_id)
    .bind((episode_id.as_u128() % 10_000_000) as i64)
    .bind(episode_title)
    .execute(pool)
    .await?;

    Ok(())
}

async fn insert_manual_member(
    pool: &PgPool,
    collection_id: CollectionId,
    media_id: MediaID,
    title: &str,
    position: u32,
) -> Result<()> {
    let media_type = match media_id {
        MediaID::Movie(_) => CollectionMediaKind::Movie,
        MediaID::Series(_) => CollectionMediaKind::Series,
        MediaID::Season(_) => CollectionMediaKind::Season,
        MediaID::Episode(_) => CollectionMediaKind::Episode,
    };
    sqlx::query(
        r#"
        INSERT INTO collection_manual_memberships (
            collection_id,
            item_key,
            media_type,
            media_id,
            title_snapshot,
            position_key
        ) VALUES ($1, $2, ($3::text)::media_type, $4, $5, ($6::text)::numeric)
        "#,
    )
    .bind(collection_id.to_uuid())
    .bind(
        ferrex_core::api::types::collections::collection_media_stable_key(
            &media_id,
        ),
    )
    .bind(media_type.as_slug())
    .bind(*media_id.as_uuid())
    .bind(title)
    .bind(position.to_string())
    .execute(pool)
    .await?;

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn collection_definition_crud_versions_archive_and_pagination(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool);

    let bravo = repo.create_collection(create_request("Bravo")).await?;
    assert_eq!(bravo.summary.title, "Bravo");
    assert_eq!(bravo.summary.owner, CollectionOwner::default());
    assert_eq!(bravo.summary.scope, CollectionScope::User);
    assert_eq!(bravo.summary.visibility, CollectionVisibility::Private);
    assert_eq!(bravo.summary.version.revision, 0);

    let alpha = repo.create_collection(create_request("Alpha")).await?;
    let updated = repo
        .update_collection(
            bravo.summary.identity.id,
            UpdateCollectionRequest {
                title: Some("Bravo Updated".to_string()),
                expected_revision: Some(0),
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(updated.summary.title, "Bravo Updated");
    assert_eq!(updated.summary.version.revision, 1);

    let stale = repo
        .update_collection(
            bravo.summary.identity.id,
            UpdateCollectionRequest {
                title: Some("Stale".to_string()),
                expected_revision: Some(0),
                ..Default::default()
            },
        )
        .await
        .expect_err("stale revision should conflict");
    assert!(matches!(stale, MediaError::Conflict(_)));

    let archived = repo
        .archive_collection(
            bravo.summary.identity.id,
            ArchiveCollectionRequest {
                expected_revision: Some(1),
                reason: Some("done".to_string()),
                ..Default::default()
            },
            None,
        )
        .await?;
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.version.revision, 2);

    let visible = repo
        .list_collections(
            ListCollectionsRequest::default(),
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(visible.collections.len(), 1);
    assert_eq!(
        visible.collections[0].identity.id,
        alpha.summary.identity.id
    );

    let first_page = repo
        .list_collections(
            ListCollectionsRequest {
                page:
                    ferrex_core::api::types::collections::CollectionPagination {
                        cursor: None,
                        limit: 1,
                    },
                include_archived: true,
                ..Default::default()
            },
            CollectionReadMode::Admin,
        )
        .await?;
    assert_eq!(first_page.collections.len(), 1);
    assert_eq!(first_page.page.total, 2);
    assert!(first_page.page.next_cursor.is_some());

    let detail = repo
        .get_collection_detail(
            bravo.summary.identity.id,
            GetCollectionDetailRequest::default(),
            CollectionReadMode::Admin,
        )
        .await?
        .expect("archived collection detail is still available by id");
    assert_eq!(detail.summary.timestamps.archived_at, archived.archived_at);

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn collection_resolver_reports_mixed_media_availability(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let available_movie = uuid("10000000-0000-7000-8000-000000000001");
    let tombstoned_movie = uuid("10000000-0000-7000-8000-000000000002");
    let series_id = uuid("10000000-0000-7000-8000-000000000003");
    let available_season = uuid("10000000-0000-7000-8000-000000000004");
    let available_episode = uuid("10000000-0000-7000-8000-000000000005");
    let tombstoned_season = uuid("10000000-0000-7000-8000-000000000006");
    let tombstoned_episode = uuid("10000000-0000-7000-8000-000000000007");
    let missing_episode = uuid("10000000-0000-7000-8000-000000000008");

    seed_movie(
        &pool,
        available_movie,
        uuid("20000000-0000-7000-8000-000000000001"),
        "Available Movie",
        true,
    )
    .await?;
    seed_movie(
        &pool,
        tombstoned_movie,
        uuid("20000000-0000-7000-8000-000000000002"),
        "Tombstoned Movie",
        false,
    )
    .await?;
    seed_tv_episode(
        &pool,
        series_id,
        available_season,
        available_episode,
        uuid("20000000-0000-7000-8000-000000000003"),
        1,
        1,
        "Available Series",
        "Available Episode",
        true,
    )
    .await?;
    seed_tv_episode(
        &pool,
        series_id,
        tombstoned_season,
        tombstoned_episode,
        uuid("20000000-0000-7000-8000-000000000004"),
        2,
        1,
        "Available Series",
        "Tombstoned Episode",
        false,
    )
    .await?;

    let resolved = repo
        .resolve_collection_items(&[
            CollectionItemIdentity::new(MediaID::Movie(MovieID(
                available_movie,
            ))),
            CollectionItemIdentity::new(MediaID::Movie(MovieID(
                tombstoned_movie,
            ))),
            CollectionItemIdentity::new(MediaID::Series(SeriesID(series_id))),
            CollectionItemIdentity::new(MediaID::Season(SeasonID(
                tombstoned_season,
            ))),
            CollectionItemIdentity::new(MediaID::Episode(EpisodeID(
                missing_episode,
            ))),
        ])
        .await?;

    let statuses: Vec<_> = resolved
        .iter()
        .map(|item| item.availability.status)
        .collect();
    assert_eq!(
        statuses,
        vec![
            CollectionMemberAvailabilityStatus::Available,
            CollectionMemberAvailabilityStatus::Tombstoned,
            CollectionMemberAvailabilityStatus::Available,
            CollectionMemberAvailabilityStatus::Tombstoned,
            CollectionMemberAvailabilityStatus::Missing,
        ]
    );
    assert_eq!(resolved[2].title.as_deref(), Some("Available Series"));

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn collection_item_reads_filter_normal_and_admin_modes(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let collection = repo
        .create_collection(create_request("Mixed availability"))
        .await?;
    let available_movie = uuid("30000000-0000-7000-8000-000000000001");
    let tombstoned_movie = uuid("30000000-0000-7000-8000-000000000002");
    let missing_movie = uuid("30000000-0000-7000-8000-000000000003");

    seed_movie(
        &pool,
        available_movie,
        uuid("40000000-0000-7000-8000-000000000001"),
        "Available Movie",
        true,
    )
    .await?;
    seed_movie(
        &pool,
        tombstoned_movie,
        uuid("40000000-0000-7000-8000-000000000002"),
        "Tombstoned Movie",
        false,
    )
    .await?;

    insert_manual_member(
        &pool,
        collection.summary.identity.id,
        MediaID::Movie(MovieID(available_movie)),
        "Available Movie",
        1,
    )
    .await?;
    insert_manual_member(
        &pool,
        collection.summary.identity.id,
        MediaID::Movie(MovieID(tombstoned_movie)),
        "Tombstoned Movie",
        2,
    )
    .await?;
    insert_manual_member(
        &pool,
        collection.summary.identity.id,
        MediaID::Movie(MovieID(missing_movie)),
        "Missing Movie",
        3,
    )
    .await?;

    let normal = repo
        .list_collection_items(
            collection.summary.identity.id,
            ListCollectionItemsRequest::default(),
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(normal.items.len(), 1);
    assert_eq!(normal.items[0].title, "Available Movie");
    assert_eq!(
        normal.items[0].availability.status,
        CollectionMemberAvailabilityStatus::Available
    );

    let admin = repo
        .list_collection_items(
            collection.summary.identity.id,
            ListCollectionItemsRequest::default(),
            CollectionReadMode::Admin,
        )
        .await?;
    assert_eq!(admin.items.len(), 3);
    let statuses: Vec<_> = admin
        .items
        .iter()
        .map(|item| item.availability.status)
        .collect();
    assert_eq!(
        statuses,
        vec![
            CollectionMemberAvailabilityStatus::Available,
            CollectionMemberAvailabilityStatus::Tombstoned,
            CollectionMemberAvailabilityStatus::Missing,
        ]
    );

    Ok(())
}
