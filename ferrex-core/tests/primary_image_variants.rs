//! Verifies primary image variants (poster/backdrop) are surfaced on movie details.

use ferrex_core::database::repositories::media_references::PostgresMediaReferencesRepository;
use ferrex_core::{
    error::Result,
    types::{ImageMediaType, MediaID, MovieID, ids::LibraryId},
};
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_movie_library(pool: &PgPool, library_id: LibraryId) {
    let unique_name = format!("Test Library - Primary Images {}", library_id);
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

#[sqlx::test]
async fn movie_details_include_primary_poster_and_backdrop_iids(
    pool: PgPool,
) -> Result<()> {
    let repo = PostgresMediaReferencesRepository::new(pool.clone());

    let library_id = LibraryId(Uuid::now_v7());
    seed_movie_library(&pool, library_id).await;

    let movie_uuid = Uuid::now_v7();
    let movie_id = MovieID(movie_uuid);

    let file_id = Uuid::now_v7();
    let file_path = "/media/test/primary-image.mkv";

    sqlx::query(
        r#"
        INSERT INTO media_files (
            id, library_id, media_id, media_type, file_path, filename, file_size
        )
        VALUES ($1, $2, $3, 'movie', $4, $5, $6)
        "#,
    )
    .bind(file_id)
    .bind(library_id.to_uuid())
    .bind(movie_uuid)
    .bind(file_path)
    .bind("primary-image.mkv")
    .bind(123_i64)
    .execute(&pool)
    .await
    .expect("insert media_files");

    sqlx::query(
        r#"
        INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(movie_uuid)
    .bind(library_id.to_uuid())
    .bind(file_id)
    .bind(12345_i64)
    .bind("Primary Image Test")
    .execute(&pool)
    .await
    .expect("insert movie_references");

    let poster_iid = Uuid::now_v7();
    let backdrop_iid = Uuid::now_v7();

    sqlx::query(
        r#"
        INSERT INTO tmdb_image_variants (
            id, tmdb_path, media_id, image_variant, media_type, width, height,
            iso_lang, vote_avg, vote_cnt, is_primary
        )
        VALUES
            ($1, $2, $3, 'poster', 'movie', $4, $5, NULL, $6, $7, $8),
            ($9, $10, $11, 'backdrop', 'movie', $12, $13, NULL, $14, $15, $16)
        "#,
    )
    .bind(poster_iid)
    .bind("/poster-primary.jpg")
    .bind(movie_uuid)
    .bind(300_i16)
    .bind(450_i16)
    .bind(9.9_f32)
    .bind(100_i32)
    .bind(true)
    .bind(backdrop_iid)
    .bind("/backdrop-primary.jpg")
    .bind(movie_uuid)
    .bind(1280_i16)
    .bind(720_i16)
    .bind(9.8_f32)
    .bind(90_i32)
    .bind(true)
    .execute(&pool)
    .await
    .expect("insert tmdb_image_variants");

    sqlx::query(
        r#"
        INSERT INTO movie_metadata (
            movie_id, library_id, batch_id, tmdb_id, title,
            primary_poster_image_id, primary_backdrop_image_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(movie_uuid)
    .bind(library_id.to_uuid())
    .bind(1_i64)
    .bind(12345_i64)
    .bind("Primary Image Test")
    .bind(poster_iid)
    .bind(backdrop_iid)
    .execute(&pool)
    .await
    .expect("insert movie_metadata");

    let movie = repo.get_movie(&movie_id).await?;

    assert_eq!(movie.file.path.to_string_lossy(), file_path);
    assert_eq!(movie.file.media_id, MediaID::Movie(movie_id));
    assert_eq!(movie.details.primary_poster_iid, Some(poster_iid));
    assert_eq!(movie.details.primary_backdrop_iid, Some(backdrop_iid));

    Ok(())
}
