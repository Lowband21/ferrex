//! Ensures TMDB conflicts converge to a canonical movie reference ID.

use chrono::Utc;
use ferrex_core::database::repositories::media_references::PostgresMediaReferencesRepository;
use ferrex_core::{
    database::repository_ports::media_references::MediaReferencesRepository,
    types::{
        details::{EnhancedMovieDetails, ExternalIds},
        files::MediaFile,
        ids::{LibraryId, MovieID},
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

fn build_movie_reference(
    library_id: LibraryId,
    movie_id: MovieID,
    tmdb_id: u64,
    file_path: &str,
) -> MovieReference {
    let details = EnhancedMovieDetails {
        id: tmdb_id,
        title: format!("Test Movie {}", tmdb_id),
        original_title: None,
        overview: None,
        release_date: None,
        runtime: None,
        vote_average: None,
        vote_count: None,
        popularity: None,
        content_rating: None,
        content_ratings: Vec::new(),
        release_dates: Vec::new(),
        genres: Vec::new(),
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
        primary_poster_iid: None,
        primary_backdrop_iid: None,
        images: MediaImages::default(),
        cast: Vec::new(),
        crew: Vec::new(),
        videos: Vec::new(),
        keywords: Vec::new(),
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
        title: MovieTitle::new(format!("Test Movie {}", tmdb_id))
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
async fn movie_reference_conflict_returns_canonical_id(pool: PgPool) {
    let repo = PostgresMediaReferencesRepository::new(pool.clone());

    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&pool, library_id).await;

    let tmdb_id = 42;

    let first_id = MovieID::new();
    let movie1 = build_movie_reference(
        library_id,
        first_id,
        tmdb_id,
        "/test/movies/movie-a.mkv",
    );
    let stored1 = repo
        .store_movie_reference(&movie1)
        .await
        .expect("store first movie ref");
    assert_eq!(stored1, MediaID::Movie(first_id));

    // Same TMDB movie + library, but with a different generated UUID and file.
    let second_id = MovieID::new();
    let movie2 = build_movie_reference(
        library_id,
        second_id,
        tmdb_id,
        "/test/movies/movie-b.mkv",
    );
    let stored2 = repo
        .store_movie_reference(&movie2)
        .await
        .expect("store conflicting movie ref");

    // `(tmdb_id, library_id)` conflict keeps the existing row id, so the
    // returned MediaID must be the canonical (first) id.
    assert_eq!(stored2, MediaID::Movie(first_id));

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM movie_references WHERE tmdb_id = $1 AND library_id = $2",
    )
        .bind(tmdb_id as i64)
        .bind(library_id.to_uuid())
        .fetch_one(&pool)
        .await
        .expect("count movie_references rows");
    assert_eq!(count, 1);

    let persisted_movie_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM movie_references WHERE tmdb_id = $1 AND library_id = $2",
    )
        .bind(tmdb_id as i64)
        .bind(library_id.to_uuid())
        .fetch_one(&pool)
        .await
        .expect("fetch canonical movie reference id");
    assert_eq!(persisted_movie_id, first_id.to_uuid());

    // The media_files row for the second file must also be aligned to the
    // canonical movie id (not the newly generated second_id).
    let file_media_id: Uuid = sqlx::query_scalar(
        "SELECT media_id FROM media_files WHERE file_path = $1",
    )
    .bind("/test/movies/movie-b.mkv")
    .fetch_one(&pool)
    .await
    .expect("fetch media_files.media_id for second file");
    assert_eq!(file_media_id, first_id.to_uuid());
}
