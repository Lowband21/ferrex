//! Exercises batch-scoped movie reference loading and batch listing queries.

use chrono::Utc;
use ferrex_core::database::repositories::media_references::PostgresMediaReferencesRepository;
use ferrex_core::{
    database::repository_ports::media_references::MediaReferencesRepository,
    types::{
        details::{EnhancedMovieDetails, ExternalIds, GenreInfo, Keyword},
        files::MediaFile,
        ids::{LibraryId, MovieBatchId, MovieID},
        image::MediaImages,
        media::MovieReference,
        titles::MovieTitle,
        urls::{MovieURL, UrlLike},
    },
};
use ferrex_model::MediaID;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_movie_library(pool: &PgPool, library_id: LibraryId) {
    let unique_name = format!("Test Library - Movies {}", library_id);
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at)
        VALUES ($1, $2, $3, $4, NOW(), NOW())
        "#,
    )
    .bind(library_id.to_uuid())
    .bind(unique_name)
    .bind(vec!["/test/movies"])
    .bind("movies")
    .execute(pool)
    .await
    .expect("seed library");
}

async fn seed_movie_primary_poster(
    pool: &PgPool,
    movie_id: MovieID,
    primary_poster_iid: Uuid,
) {
    let tmdb_path = format!(
        "test_poster_{}",
        primary_poster_iid
            .to_string()
            .split('-')
            .next()
            .expect("uuid prefix")
    );

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
        )
        VALUES (
            $1,
            'poster',
            $2,
            $3,
            'movie',
            1,
            1,
            0.0,
            0,
            true
        )
        "#,
    )
    .bind(primary_poster_iid)
    .bind(tmdb_path)
    .bind(movie_id.to_uuid())
    .execute(pool)
    .await
    .expect("insert tmdb_image_variants poster");
}

fn build_movie_reference(
    library_id: LibraryId,
    movie_id: MovieID,
    tmdb_id: u64,
    primary_poster_iid: Uuid,
    file_path: &str,
) -> MovieReference {
    let details = EnhancedMovieDetails {
        id: tmdb_id,
        title: format!("Batch Test Movie {}", tmdb_id),
        original_title: None,
        overview: Some("Overview".to_string()),
        release_date: Some("2020-01-02".to_string()),
        runtime: Some(123),
        vote_average: Some(7.7),
        vote_count: Some(55),
        popularity: Some(12.34),
        content_rating: None,
        content_ratings: Vec::new(),
        release_dates: Vec::new(),
        genres: vec![GenreInfo {
            id: 28,
            name: "Action".to_string(),
        }],
        spoken_languages: Vec::new(),
        production_companies: Vec::new(),
        production_countries: Vec::new(),
        homepage: None,
        status: None,
        tagline: None,
        budget: None,
        revenue: None,
        poster_path: None,
        backdrop_path: None,
        logo_path: None,
        primary_poster_iid: Some(primary_poster_iid),
        primary_backdrop_iid: None,
        images: MediaImages::default(),
        cast: Vec::new(),
        crew: Vec::new(),
        videos: Vec::new(),
        keywords: vec![Keyword {
            id: 999,
            name: "batch-loader".to_string(),
        }],
        external_ids: ExternalIds::default(),
        alternative_titles: Vec::new(),
        translations: Vec::new(),
        collection: None,
        recommendations: Vec::new(),
        similar: Vec::new(),
    };

    MovieReference {
        id: movie_id,
        library_id,
        batch_id: None,
        tmdb_id,
        title: MovieTitle::new(format!("Batch Test Movie {}", tmdb_id))
            .expect("valid title"),
        details,
        endpoint: MovieURL::from_string(format!("/stream/{}", Uuid::now_v7())),
        file: MediaFile {
            id: Uuid::now_v7(),
            media_id: MediaID::Movie(movie_id),
            path: std::path::PathBuf::from(file_path),
            filename: std::path::Path::new(file_path)
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .to_string(),
            size: 123,
            discovered_at: Utc::now(),
            created_at: Utc::now(),
            media_file_metadata: None,
            library_id,
        },
        theme_color: None,
    }
}

#[sqlx::test]
async fn movie_reference_batch_fetch_uses_batch_scoped_loader(pool: PgPool) {
    let repo = PostgresMediaReferencesRepository::new(pool.clone());

    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&pool, library_id).await;

    let movie_id = MovieID::new();
    let primary_poster_iid = Uuid::now_v7();
    seed_movie_primary_poster(&pool, movie_id, primary_poster_iid).await;

    let movie = build_movie_reference(
        library_id,
        movie_id,
        10101,
        primary_poster_iid,
        "/test/movies/batch-loader.mkv",
    );

    repo.store_movie_reference(&movie)
        .await
        .expect("store movie reference");

    let batch_id: i64 = sqlx::query_scalar(
        "SELECT batch_id FROM movie_references WHERE id = $1",
    )
    .bind(movie_id.to_uuid())
    .fetch_one(&pool)
    .await
    .expect("fetch batch_id");

    let batch_id = MovieBatchId::new(batch_id as u32).expect("valid batch id");

    let items = repo
        .get_movie_references_by_batch(&library_id, batch_id)
        .await
        .expect("fetch movie references by batch");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, movie_id);
    assert_eq!(items[0].library_id, library_id);
    assert_eq!(items[0].batch_id, Some(batch_id));

    assert_eq!(items[0].details.genres.len(), 1);
    assert_eq!(items[0].details.genres[0].id, 28);
    assert_eq!(items[0].details.genres[0].name, "Action");

    assert_eq!(items[0].details.keywords.len(), 1);
    assert_eq!(items[0].details.keywords[0].id, 999);
    assert_eq!(items[0].details.keywords[0].name, "batch-loader");
}

#[sqlx::test]
async fn list_movie_batches_with_movies_includes_unfinalized(pool: PgPool) {
    let repo = PostgresMediaReferencesRepository::new(pool.clone());

    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&pool, library_id).await;

    let movie_id = MovieID::new();
    let primary_poster_iid = Uuid::now_v7();
    seed_movie_primary_poster(&pool, movie_id, primary_poster_iid).await;

    let movie = build_movie_reference(
        library_id,
        movie_id,
        20202,
        primary_poster_iid,
        "/test/movies/batch-unfinalized.mkv",
    );

    repo.store_movie_reference(&movie)
        .await
        .expect("store movie reference");

    let finalized = repo
        .list_finalized_movie_reference_batches(&library_id)
        .await
        .expect("list finalized movie batches");
    assert!(
        finalized.is_empty(),
        "new library should start with an unfinalized batch"
    );

    let batch_ids = repo
        .list_movie_reference_batches_with_movies(&library_id)
        .await
        .expect("list movie batches with movies");
    assert_eq!(batch_ids.len(), 1);
    assert_eq!(batch_ids[0], MovieBatchId::new(1).expect("batch id 1"));

    let versions = repo
        .list_movie_batch_versions_with_movies(&library_id)
        .await
        .expect("list movie batch versions with movies");
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].batch_id, batch_ids[0]);
    assert_eq!(versions[0].version, 1);
}
