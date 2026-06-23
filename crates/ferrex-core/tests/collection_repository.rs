//! Postgres-backed collection repository behavior tests.

#![cfg(feature = "database")]

use anyhow::Result;
use ferrex_core::api::types::collections::{
    ArchiveCollectionRequest, CollectionDuplicatePolicy, CollectionId,
    CollectionKind, CollectionLimitPolicy, CollectionMaterializationState,
    CollectionMediaKind, CollectionMediaScope,
    CollectionMemberAvailabilityStatus, CollectionOwner, CollectionPersonRole,
    CollectionPersonRuleValue, CollectionPresentationMode, CollectionRuleField,
    CollectionRuleOperator, CollectionRulePredicate, CollectionRuleValue,
    CollectionScope, CollectionSortDirection, CollectionSortField,
    CollectionSortKey, CollectionSortNulls, CollectionSortPolicy,
    CollectionSource, CollectionVisibility, CollectionWatchStatus,
    CollectionWatchStatusRuleValue, CreateCollectionRequest,
    DynamicCollectionRule, GetCollectionDetailRequest,
    ListCollectionItemsRequest, ListCollectionsRequest,
    PreviewCollectionRuleRequest, RefreshCollectionRuleRequest,
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

fn dynamic_request(
    title: &str,
    rule: DynamicCollectionRule,
) -> CreateCollectionRequest {
    CreateCollectionRequest {
        kind: CollectionKind::DynamicRule,
        source: CollectionSource::DynamicRule,
        rule: Some(rule),
        ..create_request(title)
    }
}

fn field_predicate(
    field: CollectionRuleField,
    operator: CollectionRuleOperator,
    value: CollectionRuleValue,
) -> CollectionRulePredicate {
    CollectionRulePredicate::Field {
        field,
        operator,
        value,
    }
}

fn all_rule(
    clauses: Vec<CollectionRulePredicate>,
    sort: CollectionSortPolicy,
    limit: CollectionLimitPolicy,
) -> DynamicCollectionRule {
    DynamicCollectionRule {
        predicate: CollectionRulePredicate::All { clauses },
        sort,
        limit,
        ..Default::default()
    }
}

fn sort_by(
    field: CollectionSortField,
    direction: CollectionSortDirection,
) -> CollectionSortPolicy {
    CollectionSortPolicy {
        keys: vec![CollectionSortKey {
            field,
            direction,
            nulls: CollectionSortNulls::Last,
            user_id: None,
        }],
        ..Default::default()
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

async fn set_file_discovered_at(
    pool: &PgPool,
    file_id: Uuid,
    discovered_at: &str,
) -> Result<()> {
    sqlx::query("UPDATE media_files SET discovered_at = $2 WHERE id = $1")
        .bind(file_id)
        .bind(discovered_at.parse::<chrono::DateTime<chrono::Utc>>()?)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_movie_metadata(
    pool: &PgPool,
    movie_id: Uuid,
    title: &str,
    release_date: &str,
    genres: &[&str],
) -> Result<()> {
    let image_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO tmdb_image_variants (
            id,
            image_variant,
            tmdb_path,
            media_id,
            media_type,
            width,
            height,
            vote_avg,
            vote_cnt,
            is_primary
        ) VALUES ($1, 'poster', $2, $3, 'movie', 500, 750, 1.0, 1, TRUE)
        "#,
    )
    .bind(image_id)
    .bind(format!("/{movie_id}.jpg"))
    .bind(movie_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO movie_metadata (
            movie_id,
            library_id,
            batch_id,
            tmdb_id,
            title,
            release_date,
            runtime,
            vote_average,
            popularity,
            primary_poster_image_id
        ) VALUES ($1, $2, 1, $3, $4, $5, 120, 7.5, 10.0, $6)
        "#,
    )
    .bind(movie_id)
    .bind(movie_library_id().as_uuid())
    .bind((movie_id.as_u128() % 10_000_000) as i64)
    .bind(title)
    .bind(chrono::NaiveDate::parse_from_str(release_date, "%Y-%m-%d")?)
    .bind(image_id)
    .execute(pool)
    .await?;

    for (index, genre) in genres.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO movie_genres (movie_id, library_id, batch_id, genre_id, name)
            VALUES ($1, $2, 1, $3, $4)
            "#,
        )
        .bind(movie_id)
        .bind(movie_library_id().as_uuid())
        .bind(i64::try_from(index + 1)?)
        .bind(*genre)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn seed_person(pool: &PgPool, tmdb_id: i64, name: &str) -> Result<Uuid> {
    let person_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO persons (id, tmdb_id, name)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(person_id)
    .bind(tmdb_id)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(person_id)
}

async fn seed_episode_actor(
    pool: &PgPool,
    episode_id: Uuid,
    person_id: Uuid,
    person_tmdb_id: i64,
    character: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO episode_cast (
            episode_id,
            person_tmdb_id,
            person_id,
            credit_id,
            "character",
            order_index
        ) VALUES ($1, $2, $3, $4, $5, 0)
        "#,
    )
    .bind(episode_id)
    .bind(person_tmdb_id)
    .bind(person_id)
    .bind(format!("credit-{episode_id}-{person_tmdb_id}"))
    .bind(character)
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

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn dynamic_rule_preview_evaluates_required_examples_with_availability(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());

    let action_adventure = uuid("50000000-0000-7000-8000-000000000001");
    let action_only = uuid("50000000-0000-7000-8000-000000000002");
    let tombstoned_action_adventure =
        uuid("50000000-0000-7000-8000-000000000003");
    seed_movie(
        &pool,
        action_adventure,
        uuid("51000000-0000-7000-8000-000000000001"),
        "Brave Quest",
        true,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        action_adventure,
        "Brave Quest",
        "2010-01-01",
        &["Action", "Adventure"],
    )
    .await?;
    seed_movie(
        &pool,
        action_only,
        uuid("51000000-0000-7000-8000-000000000002"),
        "Solo Action",
        true,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        action_only,
        "Solo Action",
        "2011-01-01",
        &["Action"],
    )
    .await?;
    seed_movie(
        &pool,
        tombstoned_action_adventure,
        uuid("51000000-0000-7000-8000-000000000003"),
        "Lost Quest",
        false,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        tombstoned_action_adventure,
        "Lost Quest",
        "2012-01-01",
        &["Action", "Adventure"],
    )
    .await?;

    let action_adventure_rule = all_rule(
        vec![
            field_predicate(
                CollectionRuleField::MediaType,
                CollectionRuleOperator::Equals,
                CollectionRuleValue::MediaType(CollectionMediaKind::Movie),
            ),
            field_predicate(
                CollectionRuleField::Genre,
                CollectionRuleOperator::ContainsAll,
                CollectionRuleValue::Strings(vec![
                    "Action".to_string(),
                    "Adventure".to_string(),
                ]),
            ),
        ],
        sort_by(CollectionSortField::Title, CollectionSortDirection::Asc),
        CollectionLimitPolicy::default(),
    );
    let normal = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: action_adventure_rule.clone(),
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(
        normal
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Brave Quest"]
    );
    assert_eq!(normal.materialization.total_count, 2);
    assert_eq!(normal.materialization.visible_count, 1);

    let admin = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: action_adventure_rule,
                page: Default::default(),
            },
            CollectionReadMode::Admin,
        )
        .await?;
    assert_eq!(admin.items.len(), 2);
    assert!(admin.items.iter().any(|item| {
        item.title == "Lost Quest"
            && item.availability.status
                == CollectionMemberAvailabilityStatus::Tombstoned
    }));

    let series_id = uuid("52000000-0000-7000-8000-000000000001");
    let season_id = uuid("52000000-0000-7000-8000-000000000002");
    let actor_episode = uuid("52000000-0000-7000-8000-000000000003");
    seed_tv_episode(
        &pool,
        series_id,
        season_id,
        actor_episode,
        uuid("52000000-0000-7000-8000-000000000004"),
        1,
        1,
        "Actor Show",
        "Pilot",
        true,
    )
    .await?;
    let actor_tmdb_id = 4242;
    let actor_id = seed_person(&pool, actor_tmdb_id, "Ada Actor").await?;
    seed_episode_actor(&pool, actor_episode, actor_id, actor_tmdb_id, "Hero")
        .await?;
    let actor_rule = all_rule(
        vec![
            field_predicate(
                CollectionRuleField::MediaType,
                CollectionRuleOperator::Equals,
                CollectionRuleValue::MediaType(CollectionMediaKind::Episode),
            ),
            field_predicate(
                CollectionRuleField::Person,
                CollectionRuleOperator::Contains,
                CollectionRuleValue::Person(CollectionPersonRuleValue {
                    role: CollectionPersonRole::Actor,
                    name: Some("Ada Actor".to_string()),
                    tmdb_id: None,
                }),
            ),
        ],
        sort_by(CollectionSortField::Title, CollectionSortDirection::Asc),
        CollectionLimitPolicy::default(),
    );
    let actor_preview = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: actor_rule,
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(actor_preview.items.len(), 1);
    assert_eq!(
        actor_preview.items[0].media_id,
        MediaID::Episode(EpisodeID(actor_episode))
    );

    let recent_a = uuid("53000000-0000-7000-8000-000000000001");
    let recent_b = uuid("53000000-0000-7000-8000-000000000002");
    let recent_c = uuid("53000000-0000-7000-8000-000000000003");
    let recent_a_file = uuid("53000000-0000-7000-8000-000000000011");
    let recent_b_file = uuid("53000000-0000-7000-8000-000000000012");
    let recent_c_file = uuid("53000000-0000-7000-8000-000000000013");
    for (movie, file, title, discovered_at) in [
        (recent_a, recent_a_file, "Recent A", "2030-01-01T00:00:00Z"),
        (recent_b, recent_b_file, "Recent B", "2030-01-03T00:00:00Z"),
        (recent_c, recent_c_file, "Recent C", "2030-01-02T00:00:00Z"),
    ] {
        seed_movie(&pool, movie, file, title, true).await?;
        seed_movie_metadata(&pool, movie, title, "2015-01-01", &["Drama"])
            .await?;
        set_file_discovered_at(&pool, file, discovered_at).await?;
    }
    let recent_rule = all_rule(
        vec![
            field_predicate(
                CollectionRuleField::MediaType,
                CollectionRuleOperator::Equals,
                CollectionRuleValue::MediaType(CollectionMediaKind::Movie),
            ),
            field_predicate(
                CollectionRuleField::LibraryId,
                CollectionRuleOperator::Equals,
                CollectionRuleValue::Uuid(movie_library_id().to_uuid()),
            ),
        ],
        sort_by(
            CollectionSortField::RecentlyAdded,
            CollectionSortDirection::Desc,
        ),
        CollectionLimitPolicy {
            max_items: Some(2),
            ..Default::default()
        },
    );
    let recent_preview = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: recent_rule,
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(
        recent_preview
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Recent B", "Recent C"]
    );

    sqlx::query("UPDATE episode_metadata SET air_date = '2031-05-01' WHERE episode_id = $1")
        .bind(actor_episode)
        .execute(&pool)
        .await?;
    let released_rule = DynamicCollectionRule {
        sort: sort_by(
            CollectionSortField::RecentlyReleased,
            CollectionSortDirection::Desc,
        ),
        limit: CollectionLimitPolicy {
            max_items: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let released_preview = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: released_rule,
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(released_preview.items.len(), 2);
    assert_eq!(
        released_preview.items[0].media_id,
        MediaID::Episode(EpisodeID(actor_episode))
    );

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn dynamic_rule_sorting_limits_after_null_safe_tiebreaks(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let first_tie = uuid("54000000-0000-7000-8000-000000000001");
    let second_tie = uuid("54000000-0000-7000-8000-000000000002");
    let undated = uuid("54000000-0000-7000-8000-000000000003");
    for (movie_id, file_id, title) in [
        (
            first_tie,
            uuid("54000000-0000-7000-8000-000000000011"),
            "Release Tie A",
        ),
        (
            second_tie,
            uuid("54000000-0000-7000-8000-000000000012"),
            "Release Tie B",
        ),
        (
            undated,
            uuid("54000000-0000-7000-8000-000000000013"),
            "Undated Release",
        ),
    ] {
        seed_movie(&pool, movie_id, file_id, title, true).await?;
    }
    seed_movie_metadata(
        &pool,
        first_tie,
        "Release Tie A",
        "2025-01-01",
        &["Drama"],
    )
    .await?;
    seed_movie_metadata(
        &pool,
        second_tie,
        "Release Tie B",
        "2025-01-01",
        &["Drama"],
    )
    .await?;

    let rule = DynamicCollectionRule {
        sort: CollectionSortPolicy {
            keys: vec![CollectionSortKey {
                field: CollectionSortField::ReleaseDate,
                direction: CollectionSortDirection::Desc,
                nulls: CollectionSortNulls::Last,
                user_id: None,
            }],
            ..Default::default()
        },
        limit: CollectionLimitPolicy {
            max_items: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let preview = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule,
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;

    assert_eq!(preview.items.len(), 2);
    assert_eq!(preview.materialization.total_count, 2);
    assert_eq!(
        preview
            .items
            .iter()
            .map(|item| item.media_id)
            .collect::<Vec<_>>(),
        vec![
            MediaID::Movie(MovieID(first_tie)),
            MediaID::Movie(MovieID(second_tie))
        ]
    );

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn dynamic_rule_materialization_persists_counts_and_state_transitions(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let available = uuid("60000000-0000-7000-8000-000000000001");
    let tombstoned = uuid("60000000-0000-7000-8000-000000000002");
    seed_movie(
        &pool,
        available,
        uuid("61000000-0000-7000-8000-000000000001"),
        "Materialized Available",
        true,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        available,
        "Materialized Available",
        "2020-01-01",
        &["Action", "Adventure"],
    )
    .await?;
    seed_movie(
        &pool,
        tombstoned,
        uuid("61000000-0000-7000-8000-000000000002"),
        "Materialized Tombstoned",
        false,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        tombstoned,
        "Materialized Tombstoned",
        "2021-01-01",
        &["Action", "Adventure"],
    )
    .await?;

    let rule = all_rule(
        vec![field_predicate(
            CollectionRuleField::Genre,
            CollectionRuleOperator::ContainsAll,
            CollectionRuleValue::Strings(vec![
                "Action".to_string(),
                "Adventure".to_string(),
            ]),
        )],
        sort_by(CollectionSortField::Title, CollectionSortDirection::Asc),
        CollectionLimitPolicy::default(),
    );
    let collection = repo
        .create_collection(dynamic_request(
            "Dynamic Materialized",
            rule.clone(),
        ))
        .await?;
    let refresh = repo
        .refresh_collection_rule(
            collection.summary.identity.id,
            RefreshCollectionRuleRequest {
                force: true,
                expected_rule_hash: None,
            },
        )
        .await?;
    assert_eq!(
        refresh.materialization.state,
        CollectionMaterializationState::Ready
    );
    assert_eq!(refresh.materialization.total_count, 2);
    assert_eq!(refresh.materialization.visible_count, 1);
    assert!(refresh.materialization.generated_at.is_some());
    assert_eq!(refresh.materialization.rule_hash, rule.rule_hash().ok());

    let normal = repo
        .list_collection_items(
            collection.summary.identity.id,
            ListCollectionItemsRequest::default(),
            CollectionReadMode::Normal,
        )
        .await?;
    assert_eq!(normal.items.len(), 1);
    assert_eq!(normal.items[0].title, "Materialized Available");
    assert_eq!(normal.materialization.visible_count, 1);

    let admin = repo
        .list_collection_items(
            collection.summary.identity.id,
            ListCollectionItemsRequest::default(),
            CollectionReadMode::Admin,
        )
        .await?;
    assert_eq!(admin.items.len(), 2);

    let persisted = sqlx::query!(
        r#"
        SELECT state::text AS "state!", rule_hash, evaluated_at, total_count, visible_count
        FROM collection_materializations
        WHERE collection_id = $1
        "#,
        collection.summary.identity.id.to_uuid(),
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(persisted.state, "ready");
    assert_eq!(persisted.total_count, 2);
    assert_eq!(persisted.visible_count, 1);
    assert!(persisted.evaluated_at.is_some());
    assert_eq!(persisted.rule_hash, rule.rule_hash()?);

    let materialized_items = sqlx::query!(
        r#"
        SELECT position, visible
        FROM collection_materialized_items
        WHERE collection_id = $1
        ORDER BY position
        "#,
        collection.summary.identity.id.to_uuid(),
    )
    .fetch_all(&pool)
    .await?;
    assert_eq!(materialized_items.len(), 2);
    assert_eq!(materialized_items[0].position, 0);

    let movie_only_rule = all_rule(
        vec![field_predicate(
            CollectionRuleField::MediaType,
            CollectionRuleOperator::Equals,
            CollectionRuleValue::MediaType(CollectionMediaKind::Movie),
        )],
        sort_by(CollectionSortField::Title, CollectionSortDirection::Asc),
        CollectionLimitPolicy::default(),
    );
    repo.update_collection(
        collection.summary.identity.id,
        UpdateCollectionRequest {
            rule: Some(movie_only_rule),
            expected_revision: Some(collection.summary.version.revision),
            ..Default::default()
        },
    )
    .await?;
    let stale = sqlx::query!(
        r#"
        SELECT state::text AS "state!", stale_at
        FROM collection_materializations
        WHERE collection_id = $1
        "#,
        collection.summary.identity.id.to_uuid(),
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(stale.state, "stale");
    assert!(stale.stale_at.is_some());

    let unsupported_rule = all_rule(
        vec![field_predicate(
            CollectionRuleField::HasSubtitles,
            CollectionRuleOperator::Equals,
            CollectionRuleValue::Boolean(true),
        )],
        CollectionSortPolicy::default(),
        CollectionLimitPolicy::default(),
    );
    let unsupported_collection = repo
        .create_collection(dynamic_request("Unsupported", unsupported_rule))
        .await?;
    let error = repo
        .refresh_collection_rule(
            unsupported_collection.summary.identity.id,
            RefreshCollectionRuleRequest {
                force: true,
                expected_rule_hash: None,
            },
        )
        .await
        .expect_err("unsupported evaluator field should fail");
    assert!(matches!(error, MediaError::InvalidMedia(_)));
    let failed = sqlx::query!(
        r#"
        SELECT state::text AS "state!", error_message
        FROM collection_materializations
        WHERE collection_id = $1
        "#,
        unsupported_collection.summary.identity.id.to_uuid(),
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(failed.state, "failed");
    assert!(failed.error_message.unwrap().contains("HasSubtitles"));

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn dynamic_rule_materialization_uses_per_user_keys_for_watch_state(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let user_id = uuid("70000000-0000-7000-8000-000000000001");
    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name)
        VALUES ($1, 'dynamicuser', 'Dynamic User')
        "#,
    )
    .bind(user_id)
    .execute(&pool)
    .await?;

    let movie_id = uuid("70000000-0000-7000-8000-000000000002");
    seed_movie(
        &pool,
        movie_id,
        uuid("70000000-0000-7000-8000-000000000003"),
        "Watched Dynamic",
        true,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        movie_id,
        "Watched Dynamic",
        "2020-01-01",
        &["Drama"],
    )
    .await?;
    sqlx::query(
        r#"
        INSERT INTO user_watch_progress (
            user_id,
            media_uuid,
            media_type,
            position,
            duration,
            last_watched,
            updated_at
        ) VALUES ($1, $2, 0, 50.0, 100.0, 1234, 1234)
        "#,
    )
    .bind(user_id)
    .bind(movie_id)
    .execute(&pool)
    .await?;

    let rule = all_rule(
        vec![field_predicate(
            CollectionRuleField::WatchStatus,
            CollectionRuleOperator::In,
            CollectionRuleValue::WatchStatus(CollectionWatchStatusRuleValue {
                user_id,
                statuses: vec![CollectionWatchStatus::InProgress],
            }),
        )],
        sort_by(CollectionSortField::Title, CollectionSortDirection::Asc),
        CollectionLimitPolicy::default(),
    );
    let collection = repo
        .create_collection(dynamic_request("Watch state", rule))
        .await?;
    let refresh = repo
        .refresh_collection_rule(
            collection.summary.identity.id,
            RefreshCollectionRuleRequest {
                force: true,
                expected_rule_hash: None,
            },
        )
        .await?;
    assert_eq!(refresh.materialization.visible_count, 1);

    let row = sqlx::query!(
        r#"
        SELECT materialization_scope, materialization_key, user_id
        FROM collection_materializations
        WHERE collection_id = $1
        "#,
        collection.summary.identity.id.to_uuid(),
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.materialization_scope, "user");
    assert_eq!(row.materialization_key, format!("user:{user_id}"));
    assert_eq!(row.user_id, Some(user_id));

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn dynamic_rule_text_values_are_parameters_not_sql_fragments(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCollectionRepository::new(pool.clone());
    let movie_id = uuid("80000000-0000-7000-8000-000000000001");
    seed_movie(
        &pool,
        movie_id,
        uuid("80000000-0000-7000-8000-000000000002"),
        "Safe Action",
        true,
    )
    .await?;
    seed_movie_metadata(
        &pool,
        movie_id,
        "Safe Action",
        "2020-01-01",
        &["Action"],
    )
    .await?;

    let injection_like_rule = all_rule(
        vec![field_predicate(
            CollectionRuleField::Genre,
            CollectionRuleOperator::Equals,
            CollectionRuleValue::String("Action' OR '1'='1".to_string()),
        )],
        CollectionSortPolicy::default(),
        CollectionLimitPolicy::default(),
    );
    let preview = repo
        .preview_collection_rule(
            PreviewCollectionRuleRequest {
                rule: injection_like_rule,
                page: Default::default(),
            },
            CollectionReadMode::Normal,
        )
        .await?;
    assert!(preview.items.is_empty());

    Ok(())
}
