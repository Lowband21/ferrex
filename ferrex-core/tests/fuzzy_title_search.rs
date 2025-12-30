//! Integration coverage for the server-side fuzzy title search ordering.

use ferrex_core::database::repositories::query::PostgresQueryRepository;
use ferrex_core::database::repository_ports::query::QueryRepository;
use ferrex_core::player_prelude::*;
use ferrex_core::query::MediaQueryBuilder;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_library(pool: &PgPool, id: Uuid, library_type: &str) {
    sqlx::query(
        r#"
        INSERT INTO libraries (id, name, library_type, paths)
        VALUES ($1, $2, $3, ARRAY['/tmp'])
        "#,
    )
    .bind(id)
    .bind(format!("test-{library_type}"))
    .bind(library_type)
    .execute(pool)
    .await
    .expect("insert library");
}

async fn seed_movie(
    pool: &PgPool,
    library_id: Uuid,
    movie_id: Uuid,
    file_id: Uuid,
    tmdb_id: i64,
    title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO media_files (id, library_id, media_id, media_type, file_path, filename, file_size)
        VALUES ($1, $2, $3, 'movie', $4, $5, 123)
        "#,
    )
    .bind(file_id)
    .bind(library_id)
    .bind(movie_id)
    .bind(format!("/tmp/{movie_id}.mkv"))
    .bind(format!("{movie_id}.mkv"))
    .execute(pool)
    .await
    .expect("insert media_file");

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
    .await
    .expect("insert movie_reference");
}

async fn seed_series(
    pool: &PgPool,
    library_id: Uuid,
    series_id: Uuid,
    tmdb_id: i64,
    title: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO series (id, library_id, tmdb_id, title)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(series_id)
    .bind(library_id)
    .bind(tmdb_id)
    .bind(title)
    .execute(pool)
    .await
    .expect("insert series");
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn title_search_prefers_intuitive_fuzzy_hits(pool: PgPool) {
    let movies_lib = Uuid::new_v4();
    let tv_lib = Uuid::new_v4();
    seed_library(&pool, movies_lib, "movies").await;
    seed_library(&pool, tv_lib, "tvshows").await;

    let star_wars = Uuid::new_v4();
    seed_movie(
        &pool,
        movies_lib,
        star_wars,
        Uuid::new_v4(),
        11,
        "Star Wars",
    )
    .await;

    let star_is_born = Uuid::new_v4();
    seed_movie(
        &pool,
        movies_lib,
        star_is_born,
        Uuid::new_v4(),
        12,
        "A Star Is Born",
    )
    .await;

    let lotr = Uuid::new_v4();
    seed_movie(
        &pool,
        movies_lib,
        lotr,
        Uuid::new_v4(),
        13,
        "The Lord of the Rings",
    )
    .await;

    let star_trek = Uuid::new_v4();
    seed_series(&pool, tv_lib, star_trek, 21, "Star Trek").await;

    let repo = PostgresQueryRepository::new(pool);

    // Abbreviation-style fuzzy query should still surface the intended title.
    let results = repo
        .query_media(&MediaQueryBuilder::new().search("lotr").limit(10).build())
        .await
        .expect("query_media");

    assert!(
        !results.is_empty(),
        "expected at least one result for 'lotr'"
    );

    assert_eq!(
        results[0].id,
        MediaID::Movie(MovieID(lotr)),
        "expected 'The Lord of the Rings' to be ranked first for 'lotr'"
    );

    // fzf/skim-like subsequence query should prioritize the most relevant title.
    let results = repo
        .query_media(&MediaQueryBuilder::new().search("swr").limit(10).build())
        .await
        .expect("query_media");

    assert_eq!(
        results[0].id,
        MediaID::Movie(MovieID(star_wars)),
        "expected 'Star Wars' to be ranked first for 'swr'"
    );

    // Partial input that is an exact prefix/substring should be treated as a strong signal.
    let results = repo
        .query_media(
            &MediaQueryBuilder::new().search("star w").limit(10).build(),
        )
        .await
        .expect("query_media");

    assert_eq!(
        results[0].id,
        MediaID::Movie(MovieID(star_wars)),
        "expected 'Star Wars' to be ranked first for 'star w'"
    );

    // Ensure other 'star*' titles are still surfaced within the top results.
    let top_ids: Vec<MediaID> = results.iter().take(5).map(|r| r.id).collect();

    assert!(
        top_ids.contains(&MediaID::Movie(MovieID(star_is_born)))
            || top_ids.contains(&MediaID::Series(SeriesID(star_trek))),
        "expected other 'star' items to be present in the top results"
    );
}
