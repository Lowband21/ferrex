use std::collections::HashSet;

use anyhow::Result;
use chrono::{Duration, Utc};
use ferrex_core::database::postgres::PostgresDatabase;
use ferrex_core::database::postgres_ext::processing_status::ProcessingStatusRepository;
use ferrex_core::database::traits::{
    FolderProcessingStatus, FolderScanFilters, MediaDatabaseTrait,
    MediaProcessingStatus,
};
use ferrex_core::error::MediaError;
use ferrex_core::player_prelude::MediaIDLike;
use ferrex_core::types::{LibraryID, MediaDetailsOption, MovieID};
use sqlx::PgPool;
use uuid::Uuid;

fn fixture_library_id() -> LibraryID {
    LibraryID(Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap())
}

fn fixture_media_file(id: &str) -> Uuid {
    Uuid::parse_str(id).unwrap()
}

fn seed_status(
    media_file_id: Uuid,
    overrides: impl FnOnce(&mut MediaProcessingStatus),
) -> MediaProcessingStatus {
    let now = Utc::now();
    let mut status = MediaProcessingStatus {
        media_file_id,
        metadata_extracted: false,
        metadata_extracted_at: None,
        tmdb_matched: false,
        tmdb_matched_at: None,
        images_cached: false,
        images_cached_at: None,
        file_analyzed: false,
        file_analyzed_at: None,
        last_error: None,
        error_details: None,
        retry_count: 0,
        next_retry_at: None,
        created_at: now,
        updated_at: now,
    };

    overrides(&mut status);
    status
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "media_processing_base")
    )
)]
async fn processing_status_repository_roundtrip(pool: PgPool) -> Result<()> {
    let db = PostgresDatabase::from_pool(pool.clone());
    let repo = ProcessingStatusRepository::new(&db);
    let media_file_id =
        fixture_media_file("11111111-1111-1111-1111-111111111111");

    let inserted = seed_status(media_file_id, |status| {
        status.metadata_extracted = true;
        status.metadata_extracted_at = Some(Utc::now());
        status.last_error = Some("initial error".into());
        status.retry_count = 2;
    });

    repo.create_or_update(&inserted).await?;

    let stored = repo.get(media_file_id).await?.expect("status inserted");
    assert!(stored.metadata_extracted);
    assert_eq!(stored.retry_count, 2);
    assert_eq!(stored.last_error.as_deref(), Some("initial error"));

    let updated = seed_status(media_file_id, |status| {
        status.metadata_extracted = false;
        status.metadata_extracted_at = None;
        status.tmdb_matched = true;
        status.tmdb_matched_at = Some(Utc::now());
        status.retry_count = 0;
        status.last_error = None;
    });

    repo.create_or_update(&updated).await?;

    let refreshed =
        repo.get(media_file_id).await?.expect("status after update");
    assert!(!refreshed.metadata_extracted);
    assert!(refreshed.tmdb_matched);
    assert_eq!(refreshed.retry_count, 0);
    assert!(refreshed.last_error.is_none());

    repo.reset(media_file_id).await?;
    assert!(repo.get(media_file_id).await?.is_none());

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "media_processing_base")
    )
)]
async fn processing_status_helpers_filter_correctly(
    pool: PgPool,
) -> Result<()> {
    let db = PostgresDatabase::from_pool(pool.clone());
    let repo = ProcessingStatusRepository::new(&db);
    let library_id = fixture_library_id();

    // Initially all fixtures are unprocessed.
    let unprocessed =
        repo.fetch_unprocessed(library_id, "metadata", 10).await?;
    assert_eq!(unprocessed.len(), 3);

    // Mark the first file as fully processed and the second as awaiting TMDB.
    repo.create_or_update(&seed_status(
        fixture_media_file("11111111-1111-1111-1111-111111111111"),
        |status| {
            status.metadata_extracted = true;
            status.metadata_extracted_at = Some(Utc::now());
            status.tmdb_matched = true;
            status.tmdb_matched_at = Some(Utc::now());
            status.images_cached = true;
            status.images_cached_at = Some(Utc::now());
            status.file_analyzed = true;
            status.file_analyzed_at = Some(Utc::now());
        },
    ))
    .await?;

    repo.create_or_update(&seed_status(
        fixture_media_file("22222222-2222-2222-2222-222222222222"),
        |status| {
            status.metadata_extracted = true;
            status.metadata_extracted_at = Some(Utc::now());
            status.tmdb_matched = false;
            status.retry_count = 3;
            status.last_error = Some("needs tmdb".into());
            status.next_retry_at = Some(Utc::now() - Duration::minutes(5));
        },
    ))
    .await?;

    repo.create_or_update(&seed_status(
        fixture_media_file("33333333-3333-3333-3333-333333333333"),
        |status| {
            status.retry_count = 4;
            status.last_error = Some("exceeded retries".into());
            status.next_retry_at = Some(Utc::now() - Duration::minutes(1));
        },
    ))
    .await?;

    let status_111 = repo
        .get(fixture_media_file("11111111-1111-1111-1111-111111111111"))
        .await?
        .expect("status for 111");
    assert!(status_111.metadata_extracted, "{:?}", status_111);

    let status_222 = repo
        .get(fixture_media_file("22222222-2222-2222-2222-222222222222"))
        .await?
        .expect("status for 222");
    assert!(status_222.metadata_extracted, "{:?}", status_222);

    let status_333 = repo
        .get(fixture_media_file("33333333-3333-3333-3333-333333333333"))
        .await?
        .expect("status for 333");
    assert!(!status_333.metadata_extracted, "{:?}", status_333);

    let remaining_metadata =
        repo.fetch_unprocessed(library_id, "metadata", 10).await?;
    let metadata_ids: HashSet<_> =
        remaining_metadata.iter().map(|f| f.id).collect();
    let expected_metadata: HashSet<_> = HashSet::from([fixture_media_file(
        "33333333-3333-3333-3333-333333333333",
    )]);
    assert_eq!(metadata_ids, expected_metadata);

    let remaining_tmdb = repo.fetch_unprocessed(library_id, "tmdb", 10).await?;
    let tmdb_ids: HashSet<_> = remaining_tmdb.iter().map(|f| f.id).collect();
    assert_eq!(
        tmdb_ids,
        HashSet::from([fixture_media_file(
            "22222222-2222-2222-2222-222222222222"
        )])
    );

    let failed = repo.fetch_failed(library_id, 3).await?;
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].id,
        fixture_media_file("22222222-2222-2222-2222-222222222222")
    );

    let failed_strict = repo.fetch_failed(library_id, 2).await?;
    assert!(
        failed_strict.is_empty(),
        "retry_count threshold should filter rows"
    );

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts(
            "test_libraries",
            "media_processing_base",
            "folder_inventory_base"
        )
    )
)]
async fn folder_inventory_filters_are_bound(pool: PgPool) -> Result<()> {
    let db = PostgresDatabase::from_pool(pool.clone());
    let library_id = fixture_library_id();

    let mut filters = FolderScanFilters {
        library_id: Some(library_id),
        ..Default::default()
    };

    let all_candidates =
        MediaDatabaseTrait::get_folders_needing_scan(&db, &filters).await?;
    assert_eq!(
        all_candidates.len(),
        3,
        "future retry should be filtered out"
    );
    assert_eq!(
        all_candidates[0].processing_status,
        FolderProcessingStatus::Pending
    );

    filters.processing_status = Some(FolderProcessingStatus::Pending);
    let pending_only =
        MediaDatabaseTrait::get_folders_needing_scan(&db, &filters).await?;
    assert_eq!(pending_only.len(), 1);
    assert_eq!(
        pending_only[0].processing_status,
        FolderProcessingStatus::Pending
    );

    filters.processing_status = Some(FolderProcessingStatus::Failed);
    filters.max_attempts = Some(2);
    let retryable =
        MediaDatabaseTrait::get_folders_needing_scan(&db, &filters).await?;
    assert_eq!(retryable.len(), 1);
    assert_eq!(retryable[0].processing_attempts, 1);

    filters.processing_status = None;
    filters.max_attempts = None;
    filters.stale_after_hours = Some(24);
    let stale =
        MediaDatabaseTrait::get_folders_needing_scan(&db, &filters).await?;
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].folder_path, "/fixture/library/a/pending");

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "media_processing_base")
    )
)]
async fn movie_reference_hydration_uses_repository(pool: PgPool) -> Result<()> {
    let db = PostgresDatabase::from_pool(pool.clone());
    let movies = MediaDatabaseTrait::get_all_movie_references(&db).await?;

    let fixture_movie = movies
        .into_iter()
        .find(|movie| {
            movie.id
                == MovieID(fixture_media_file(
                    "44444444-4444-4444-4444-444444444444",
                ))
        })
        .ok_or_else(|| MediaError::NotFound("fixture movie missing".into()))?;

    assert_eq!(
        fixture_movie.file.id,
        fixture_media_file("11111111-1111-1111-1111-111111111111")
    );
    assert_eq!(fixture_movie.file.library_id, fixture_library_id());
    assert!(
        matches!(fixture_movie.details, MediaDetailsOption::Endpoint(endpoint) if endpoint.contains(&fixture_movie.id.to_uuid().to_string()))
    );

    Ok(())
}
