//! Availability behavior for precomputed movie index lookups.

use anyhow::Result;
use ferrex_core::{
    api::types::FilterIndicesRequest,
    database::{
        repositories::indices::PostgresIndicesRepository,
        repository_ports::indices::IndicesRepository,
    },
    query::types::{SortBy, SortOrder},
    types::LibraryId,
};
use sqlx::{Executor, PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

fn fixture_library_id() -> LibraryId {
    LibraryId(Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap())
}

async fn public_schema_pool(pool: &PgPool) -> Result<PgPool> {
    let options = (*pool.connect_options()).clone();

    Ok(PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET search_path TO public").await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await?)
}

async fn create_minimal_index_schema(pool: &PgPool) -> Result<()> {
    pool.execute(
        r#"
        CREATE TABLE public.media_files (
            id uuid PRIMARY KEY,
            library_id uuid NOT NULL,
            media_id uuid NOT NULL,
            media_type text NOT NULL,
            file_path text NOT NULL UNIQUE,
            filename character varying(1000) NOT NULL,
            file_size bigint NOT NULL,
            discovered_at timestamp with time zone DEFAULT now() NOT NULL,
            created_at timestamp with time zone DEFAULT now() NOT NULL,
            updated_at timestamp with time zone DEFAULT now() NOT NULL,
            technical_metadata jsonb,
            parsed_info jsonb
        );

        CREATE TABLE public.movie_references (
            id uuid PRIMARY KEY,
            library_id uuid NOT NULL,
            file_id uuid NOT NULL,
            tmdb_id bigint NOT NULL,
            title character varying(1000) NOT NULL,
            batch_id bigint NOT NULL,
            theme_color character varying(7)
        );

        CREATE TABLE public.movie_metadata (
            movie_id uuid PRIMARY KEY,
            library_id uuid NOT NULL,
            batch_id bigint NOT NULL,
            release_date date,
            vote_average real,
            runtime integer,
            popularity real,
            primary_certification text,
            poster_path text
        );

        CREATE TABLE public.movie_sort_positions (
            movie_id uuid PRIMARY KEY,
            library_id uuid NOT NULL,
            batch_id bigint NOT NULL,
            title_pos integer NOT NULL,
            title_pos_desc integer NOT NULL,
            date_added_pos integer NOT NULL,
            date_added_pos_desc integer NOT NULL,
            created_at_pos integer NOT NULL,
            created_at_pos_desc integer NOT NULL,
            release_date_pos integer NOT NULL,
            release_date_pos_desc integer NOT NULL,
            rating_pos integer NOT NULL,
            rating_pos_desc integer NOT NULL,
            runtime_pos integer NOT NULL,
            runtime_pos_desc integer NOT NULL,
            popularity_pos integer NOT NULL,
            popularity_pos_desc integer NOT NULL,
            bitrate_pos integer NOT NULL,
            bitrate_pos_desc integer NOT NULL,
            file_size_pos integer NOT NULL,
            file_size_pos_desc integer NOT NULL,
            content_rating_pos integer NOT NULL,
            content_rating_pos_desc integer NOT NULL,
            resolution_pos integer NOT NULL,
            resolution_pos_desc integer NOT NULL,
            updated_at timestamp with time zone DEFAULT now() NOT NULL
        );
        "#,
    )
    .await?;

    pool.execute(include_str!(
        "../migrations/004_media_file_availability.sql"
    ))
    .await?;

    Ok(())
}

async fn seed_movie(
    pool: &PgPool,
    library_id: LibraryId,
    file_id: Uuid,
    movie_id: Uuid,
    tmdb_id: i64,
    title: &str,
    file_size: i64,
) -> Result<()> {
    let filename = format!("{}.mkv", title.to_lowercase());
    let file_path = format!("/fixture/library/b/{filename}");

    sqlx::query(
        r#"
        INSERT INTO public.media_files (
            id,
            library_id,
            media_id,
            media_type,
            file_path,
            filename,
            file_size,
            technical_metadata,
            parsed_info,
            is_available
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, $6, NULL, NULL, TRUE)
        "#,
    )
    .bind(file_id)
    .bind(library_id.to_uuid())
    .bind(movie_id)
    .bind(file_path)
    .bind(filename)
    .bind(file_size)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO public.movie_references (
            id,
            library_id,
            file_id,
            tmdb_id,
            title,
            batch_id,
            theme_color
        )
        VALUES ($1, $2, $3, $4, $5, 1, '#000000')
        "#,
    )
    .bind(movie_id)
    .bind(library_id.to_uuid())
    .bind(file_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await?;

    Ok(())
}

fn filter_by_file_size_desc() -> FilterIndicesRequest {
    FilterIndicesRequest {
        media_type: None,
        genres: Vec::new(),
        year_range: None,
        rating_range: None,
        resolution_range: None,
        watch_status: None,
        search: None,
        sort: Some(SortBy::FileSize),
        order: Some(SortOrder::Descending),
    }
}

#[sqlx::test(migrations = false)]
async fn sorted_and_filtered_indices_compact_around_tombstoned_media(
    pool: PgPool,
) -> Result<()> {
    let pool = public_schema_pool(&pool).await?;
    create_minimal_index_schema(&pool).await?;

    let library_id = fixture_library_id();
    let repo = PostgresIndicesRepository::new(pool.clone());

    let alpha_file = Uuid::parse_str("10000000-0000-0000-0000-000000000001")?;
    let beta_file = Uuid::parse_str("10000000-0000-0000-0000-000000000002")?;
    let gamma_file = Uuid::parse_str("10000000-0000-0000-0000-000000000003")?;
    let alpha_movie = Uuid::parse_str("20000000-0000-0000-0000-000000000001")?;
    let beta_movie = Uuid::parse_str("20000000-0000-0000-0000-000000000002")?;
    let gamma_movie = Uuid::parse_str("20000000-0000-0000-0000-000000000003")?;

    seed_movie(
        &pool,
        library_id,
        alpha_file,
        alpha_movie,
        10_001,
        "Alpha",
        100,
    )
    .await?;
    seed_movie(
        &pool, library_id, beta_file, beta_movie, 10_002, "Beta", 200,
    )
    .await?;
    seed_movie(
        &pool,
        library_id,
        gamma_file,
        gamma_movie,
        10_003,
        "Gamma",
        300,
    )
    .await?;

    repo.rebuild_movie_sort_positions(library_id).await?;

    sqlx::query(
        r#"
        UPDATE public.media_files
           SET is_available = FALSE,
               tombstoned_at = NOW(),
               tombstone_reason = 'test tombstone'
         WHERE id = $1
        "#,
    )
    .bind(beta_file)
    .execute(&pool)
    .await?;

    let stale_position_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM public.movie_sort_positions WHERE library_id = $1",
    )
    .bind(library_id.to_uuid())
    .fetch_one(&pool)
    .await?;
    assert_eq!(stale_position_count, 3, "test should exercise stale ranks");

    let available_position_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
          FROM public.movie_sort_positions msp
          JOIN public.movie_references mr
            ON mr.id = msp.movie_id
           AND mr.library_id = msp.library_id
          JOIN public.media_files mf
            ON mf.id = mr.file_id
           AND mf.library_id = mr.library_id
         WHERE msp.library_id = $1
           AND mf.is_available = TRUE
        "#,
    )
    .bind(library_id.to_uuid())
    .fetch_one(&pool)
    .await?;
    assert_eq!(available_position_count, 2);

    let sorted = repo
        .fetch_sorted_movie_indices(
            library_id,
            SortBy::FileSize,
            SortOrder::Descending,
            None,
            None,
        )
        .await?;
    assert_eq!(sorted, vec![1, 0]);

    let filtered = repo
        .fetch_filtered_movie_indices(
            library_id,
            &filter_by_file_size_desc(),
            None,
        )
        .await?;
    assert_eq!(filtered, vec![1, 0]);

    repo.rebuild_movie_sort_positions(library_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT movie_id, title_pos
          FROM public.movie_sort_positions
         WHERE library_id = $1
         ORDER BY title_pos
        "#,
    )
    .bind(library_id.to_uuid())
    .fetch_all(&pool)
    .await?;

    let rebuilt_positions: Vec<(Uuid, i32)> = rows
        .into_iter()
        .map(|row| (row.get("movie_id"), row.get("title_pos")))
        .collect();

    assert_eq!(rebuilt_positions, vec![(alpha_movie, 1), (gamma_movie, 2)]);
    assert!(!rebuilt_positions.iter().any(|(id, _)| *id == beta_movie));

    let sorted_after_rebuild = repo
        .fetch_sorted_movie_indices(
            library_id,
            SortBy::FileSize,
            SortOrder::Descending,
            None,
            None,
        )
        .await?;
    assert_eq!(sorted_after_rebuild, vec![1, 0]);

    Ok(())
}
