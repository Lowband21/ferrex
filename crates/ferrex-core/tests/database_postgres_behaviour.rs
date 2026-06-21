//! Database behavioral tests for Postgres-backed repositories.

use std::collections::HashSet;

use anyhow::Result;
use chrono::{Duration, Utc};
use ferrex_core::database::postgres::PostgresDatabase;
use ferrex_core::database::repositories::folder_inventory::PostgresFolderInventoryRepository;
use ferrex_core::database::repositories::media::PostgresMediaRepository;
use ferrex_core::database::repositories::query::PostgresQueryRepository;
use ferrex_core::database::repository_ports::folder_inventory::FolderInventoryRepository;
use ferrex_core::database::repository_ports::media_files::{
    MediaFileFilter, MediaFileSort, MediaFileSortField, MediaFilesReadPort,
    Page,
};
use ferrex_core::database::repository_ports::processing_status::ProcessingStatusRepository;
use ferrex_core::database::repository_ports::query::QueryRepository;
use ferrex_core::database::traits::{
    FolderProcessingStatus, FolderScanFilters, MediaProcessingStatus,
};
use ferrex_core::domain::scan::orchestration::{
    PostgresCursorRepository, ScanCursor, ScanCursorId, ScanCursorRepository,
};
use ferrex_core::player_prelude::{MediaID, MovieID};
use ferrex_core::query::{
    MediaQueryBuilder,
    types::{SortBy, SortOrder},
};
use ferrex_core::types::LibraryId;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

fn fixture_library_id() -> LibraryId {
    LibraryId(Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap())
}

fn fixture_media_file(id: &str) -> Uuid {
    Uuid::parse_str(id).unwrap()
}

async fn seed_query_movie(
    pool: &PgPool,
    library_id: LibraryId,
    movie_id: Uuid,
    file_id: Uuid,
    tmdb_id: i64,
    title: &str,
    genre_id: i64,
    genre: &str,
) -> Result<()> {
    let filename = format!("{}.mkv", title.to_ascii_lowercase());
    let file_path = format!("/fixture/query/{filename}");

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
            is_available
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, 100, TRUE)
        "#,
    )
    .bind(file_id)
    .bind(library_id.as_uuid())
    .bind(movie_id)
    .bind(file_path)
    .bind(filename)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO movie_references (
            id,
            library_id,
            file_id,
            tmdb_id,
            title,
            batch_id
        )
        VALUES ($1, $2, $3, $4, $5, 1)
        "#,
    )
    .bind(movie_id)
    .bind(library_id.as_uuid())
    .bind(file_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO movie_genres (movie_id, library_id, batch_id, genre_id, name)
        VALUES ($1, $2, 1, $3, $4)
        "#,
    )
    .bind(movie_id)
    .bind(library_id.as_uuid())
    .bind(genre_id)
    .bind(genre)
    .execute(pool)
    .await?;

    Ok(())
}

async fn seed_folder_inventory_path(
    pool: &PgPool,
    library_id: LibraryId,
    folder_id: Uuid,
    path: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO folder_inventory (id, library_id, folder_path, folder_type)
        VALUES ($1, $2, $3, 'movie')
        "#,
    )
    .bind(folder_id)
    .bind(library_id.as_uuid())
    .bind(path)
    .execute(pool)
    .await?;

    Ok(())
}

fn scan_cursor_for_path(library_id: LibraryId, path: &str) -> ScanCursor {
    let paths = vec![PathBuf::from(path)];
    ScanCursor {
        id: ScanCursorId::new(library_id, &paths),
        folder_path_norm: path.to_owned(),
        listing_hash: format!("hash:{path}"),
        entry_count: 1,
        last_scan_at: Utc::now(),
        last_modified_at: None,
        device_id: None,
    }
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
    let folder_repo = PostgresFolderInventoryRepository::new(pool.clone());
    let library_id = fixture_library_id();

    let mut filters = FolderScanFilters {
        library_id: Some(library_id),
        ..Default::default()
    };

    let all_candidates = FolderInventoryRepository::get_folders_needing_scan(
        &folder_repo,
        &filters,
    )
    .await?;
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
    let pending_only = FolderInventoryRepository::get_folders_needing_scan(
        &folder_repo,
        &filters,
    )
    .await?;
    assert_eq!(pending_only.len(), 1);
    assert_eq!(
        pending_only[0].processing_status,
        FolderProcessingStatus::Pending
    );

    filters.processing_status = Some(FolderProcessingStatus::Failed);
    filters.max_attempts = Some(2);
    let retryable = FolderInventoryRepository::get_folders_needing_scan(
        &folder_repo,
        &filters,
    )
    .await?;
    assert_eq!(retryable.len(), 1);
    assert_eq!(retryable[0].processing_attempts, 1);

    filters.processing_status = None;
    filters.max_attempts = None;
    filters.stale_after_hours = Some(24);
    let stale = FolderInventoryRepository::get_folders_needing_scan(
        &folder_repo,
        &filters,
    )
    .await?;
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].folder_path, "/fixture/library/a/pending");

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn query_repository_movie_filters_sorting_and_pagination_are_bound(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresQueryRepository::new(pool.clone());
    let library_a = fixture_library_id();
    let library_b =
        LibraryId(Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")?);
    let alpha_movie = Uuid::parse_str("40000000-0000-0000-0000-000000000001")?;
    let beta_movie = Uuid::parse_str("40000000-0000-0000-0000-000000000002")?;
    let gamma_movie = Uuid::parse_str("40000000-0000-0000-0000-000000000003")?;
    let other_library_movie =
        Uuid::parse_str("40000000-0000-0000-0000-000000000004")?;

    seed_query_movie(
        &pool,
        library_a,
        alpha_movie,
        Uuid::parse_str("50000000-0000-0000-0000-000000000001")?,
        20_001,
        "Alpha Action",
        1,
        "Action",
    )
    .await?;
    seed_query_movie(
        &pool,
        library_a,
        beta_movie,
        Uuid::parse_str("50000000-0000-0000-0000-000000000002")?,
        20_002,
        "Beta Action",
        1,
        "Action",
    )
    .await?;
    seed_query_movie(
        &pool,
        library_a,
        gamma_movie,
        Uuid::parse_str("50000000-0000-0000-0000-000000000003")?,
        20_003,
        "Gamma Drama",
        2,
        "Drama",
    )
    .await?;
    seed_query_movie(
        &pool,
        library_b,
        other_library_movie,
        Uuid::parse_str("50000000-0000-0000-0000-000000000004")?,
        20_004,
        "Zeta Action",
        1,
        "Action",
    )
    .await?;

    let sorted_action = repo
        .query_media(
            &MediaQueryBuilder::new()
                .movies_only()
                .in_library(library_a)
                .genre("Action")
                .sort_by(SortBy::Title, SortOrder::Descending)
                .limit(10)
                .build(),
        )
        .await?;
    let sorted_ids: Vec<_> = sorted_action.iter().map(|row| row.id).collect();
    assert_eq!(
        sorted_ids,
        vec![
            MediaID::Movie(MovieID(beta_movie)),
            MediaID::Movie(MovieID(alpha_movie))
        ]
    );

    let paged = repo
        .query_media(
            &MediaQueryBuilder::new()
                .movies_only()
                .in_library(library_a)
                .genre("Action")
                .sort_by(SortBy::Title, SortOrder::Descending)
                .limit(1)
                .offset(1)
                .build(),
        )
        .await?;
    assert_eq!(paged.len(), 1);
    assert_eq!(paged[0].id, MediaID::Movie(MovieID(alpha_movie)));

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(
        path = "../fixtures",
        scripts("test_libraries", "media_processing_base")
    )
)]
async fn media_repository_filters_sorting_pagination_and_stats_are_bound(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresMediaRepository::new(pool.clone());
    let library_id = fixture_library_id();

    let filter = MediaFileFilter {
        library_id: Some(library_id),
        path_prefix: Some("/fixture/library/a/movie_t".to_owned()),
        extension_in: vec![".mkv".to_owned()],
        min_size: Some(2),
        max_size: Some(3),
        ..Default::default()
    };

    let rows = repo
        .list(
            filter.clone(),
            MediaFileSort::descending(MediaFileSortField::FileSize),
            Page {
                limit: 1,
                offset: 0,
            },
        )
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].filename, "movie_three.mkv");

    let paged = repo
        .list(
            MediaFileFilter {
                library_id: Some(library_id),
                extension_in: vec!["mkv".to_owned()],
                ..Default::default()
            },
            MediaFileSort::descending(MediaFileSortField::FileSize),
            Page {
                limit: 2,
                offset: 1,
            },
        )
        .await?;
    let paged_names: Vec<_> =
        paged.iter().map(|file| file.filename.as_str()).collect();
    assert_eq!(paged_names, vec!["movie_two.mkv", "movie_one.mkv"]);

    let stats = repo.stats(filter).await?;
    assert_eq!(stats.total_files, 2);
    assert_eq!(stats.total_size, 5);

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn folder_inventory_prefix_deletion_deletes_roots_and_children_only(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresFolderInventoryRepository::new(pool.clone());
    let library_id = fixture_library_id();

    for (idx, path) in [
        "/fixture/library/a/removed",
        "/fixture/library/a/removed/child",
        "/fixture/library/a/removed-sibling",
    ]
    .iter()
    .enumerate()
    {
        seed_folder_inventory_path(
            &pool,
            library_id,
            Uuid::from_u128(0x60000000000000000000000000000001 + idx as u128),
            path,
        )
        .await?;
    }

    let deleted = repo
        .delete_by_path_prefixes(
            library_id,
            vec!["/fixture/library/a/removed".to_owned()],
        )
        .await?;
    assert_eq!(deleted, 2);

    let remaining = repo.get_folder_inventory(library_id).await?;
    let remaining_paths: Vec<_> = remaining
        .into_iter()
        .map(|folder| folder.folder_path)
        .collect();
    assert_eq!(remaining_paths, vec!["/fixture/library/a/removed-sibling"]);

    Ok(())
}

#[sqlx::test(
    migrator = "ferrex_core::MIGRATOR",
    fixtures(path = "../fixtures", scripts("test_libraries"))
)]
async fn scan_cursor_prefix_deletion_deletes_roots_and_children_only(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresCursorRepository::new(pool.clone());
    let library_id = fixture_library_id();

    for path in [
        "/fixture/library/a/removed",
        "/fixture/library/a/removed/child",
        "/fixture/library/a/removed-sibling",
    ] {
        repo.upsert(scan_cursor_for_path(library_id, path)).await?;
    }

    let deleted = repo
        .delete_by_path_prefixes(
            library_id,
            vec!["/fixture/library/a/removed".to_owned()],
        )
        .await?;
    assert_eq!(deleted, 2);

    let remaining = repo.list_by_library(library_id).await?;
    let remaining_paths: Vec<_> = remaining
        .into_iter()
        .map(|cursor| cursor.folder_path_norm)
        .collect();
    assert_eq!(remaining_paths, vec!["/fixture/library/a/removed-sibling"]);

    Ok(())
}
