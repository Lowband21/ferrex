use ferrex_core::{
    database::{postgres::PostgresDatabase, traits::MediaDatabaseTrait},
    query::*,
    media::*,
    LibraryType,
};
use uuid::Uuid;
use sqlx::PgPool;
use sqlx::types::BigDecimal;
use std::str::FromStr;
use chrono::Datelike;

#[cfg(test)]
mod helpers {
    use super::*;
    
    pub async fn setup_test_db() -> PostgresDatabase {
        // Use test database URL from environment or default
        let database_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://postgres:password@localhost/ferrex_test".to_string());
        
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to test database");
        
        PostgresDatabase::from_pool(pool)
    }

    pub async fn create_test_library(db: &PostgresDatabase, name: &str, library_type: LibraryType) -> Uuid {
        let library_id = Uuid::new_v4();
        // Make library name unique by appending UUID to avoid conflicts in concurrent tests
        let unique_name = format!("{}-{}", name, library_id);
        
        sqlx::query!(
            r#"
            INSERT INTO libraries (id, name, library_type, paths, scan_interval_minutes, enabled)
            VALUES ($1, $2, $3, $4, 60, true)
            "#,
            library_id,
            unique_name,
            match library_type {
                LibraryType::Movies => "movies",
                LibraryType::TvShows => "tvshows",
            },
&vec!["/test/path".to_string()]
        )
        .execute(db.pool())
        .await
        .expect("Failed to create test library");
        
        library_id
    }
    
    pub async fn cleanup_library(db: &PostgresDatabase, library_id: Uuid) {
        sqlx::query!("DELETE FROM libraries WHERE id = $1", library_id)
            .execute(db.pool())
            .await
            .expect("Failed to cleanup library");
    }

    pub async fn create_test_movie(
        db: &PostgresDatabase,
        library_id: Uuid,
        title: &str,
        tmdb_id: i64,
        release_date: Option<chrono::NaiveDate>,
        vote_average: Option<f32>,
        runtime: Option<i32>,
        genres: Vec<String>,
    ) -> (Uuid, Uuid) {
        let file_id = Uuid::new_v4();
        let movie_id = Uuid::new_v4();
        
        // Create media file with unique path
        sqlx::query!(
            r#"
            INSERT INTO media_files (id, library_id, file_path, filename, file_size)
            VALUES ($1, $2, $3, $4, 1000000)
            "#,
            file_id,
            library_id,
            format!("/test/movies/{}_{}.mp4", title, file_id),
            format!("{}.mp4", title)
        )
        .execute(db.pool())
        .await
        .expect("Failed to create test media file");
        
        // Create movie reference
        sqlx::query!(
            r#"
            INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            movie_id,
            library_id,
            file_id,
            tmdb_id,
            title
        )
        .execute(db.pool())
        .await
        .expect("Failed to create test movie reference");
        
        // Create movie metadata with extracted fields
        let tmdb_details = serde_json::json!({
            "id": tmdb_id,
            "title": title,
            "release_date": release_date.map(|d| d.to_string()),
            "vote_average": vote_average,
            "runtime": runtime,
            "genres": genres.iter().map(|g| {
                serde_json::json!({
                    "id": 1,
                    "name": g
                })
            }).collect::<Vec<_>>()
        });
        
        sqlx::query!(
            r#"
            INSERT INTO movie_metadata (
                movie_id, tmdb_details, release_date, vote_average, 
                runtime, genre_names, release_year
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            movie_id,
            tmdb_details,
            release_date,
            vote_average.map(|v| BigDecimal::from_str(&v.to_string()).ok()).flatten(),
            runtime,
            &genres,
            release_date.map(|d| d.year() as i32)
        )
        .execute(db.pool())
        .await
        .expect("Failed to create test movie metadata");
        
        (movie_id, file_id)
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use helpers::*;

    #[tokio::test]
    async fn test_query_movies_by_genre() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies", LibraryType::Movies).await;
        
        // Create test movies with different genres
        create_test_movie(
            &db, library_id, "Action Movie", 1001,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(7.5), Some(120), vec!["Action".to_string(), "Adventure".to_string()]
        ).await;
        
        create_test_movie(
            &db, library_id, "Comedy Movie", 1002,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 2, 1).unwrap()),
            Some(8.0), Some(90), vec!["Comedy".to_string()]
        ).await;
        
        create_test_movie(
            &db, library_id, "Drama Movie", 1003,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()),
            Some(9.0), Some(150), vec!["Drama".to_string()]
        ).await;
        
        // Query for Action movies
        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec!["Action".to_string()],
                library_ids: vec![library_id],
                ..Default::default()
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 1);
        
        if let MediaReference::Movie(movie) = &results[0].media {
            assert_eq!(movie.title.as_str(), "Action Movie");
        } else {
            panic!("Expected movie reference");
        }
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_query_by_year_range() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies 2", LibraryType::Movies).await;
        
        // Create movies from different years
        create_test_movie(
            &db, library_id, "Old Movie", 2001,
            Some(chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()),
            Some(6.5), Some(100), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Recent Movie", 2002,
            Some(chrono::NaiveDate::from_ymd_opt(2023, 1, 1).unwrap()),
            Some(7.5), Some(110), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "New Movie", 2003,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(8.5), Some(120), vec![]
        ).await;
        
        // Query for movies from 2023-2024
        let query = MediaQuery {
            filters: MediaFilters {
                year_range: Some((2023, 2024)),
                library_ids: vec![library_id],
                ..Default::default()
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_query_by_rating_range() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies 3", LibraryType::Movies).await;
        
        // Create movies with different ratings
        create_test_movie(
            &db, library_id, "Low Rated", 3001,
            None, Some(5.0), Some(90), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Mid Rated", 3002,
            None, Some(7.0), Some(95), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "High Rated", 3003,
            None, Some(9.0), Some(100), vec![]
        ).await;
        
        // Query for highly rated movies (8.0+)
        let query = MediaQuery {
            filters: MediaFilters {
                rating_range: Some((8.0, 10.0)),
                library_ids: vec![library_id],
                ..Default::default()
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 1);
        
        if let MediaReference::Movie(movie) = &results[0].media {
            assert_eq!(movie.title.as_str(), "High Rated");
        } else {
            panic!("Expected movie reference");
        }
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_complex_query_with_multiple_filters() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies Complex", LibraryType::Movies).await;
        
        // Create diverse movies
        create_test_movie(
            &db, library_id, "Action 2023 High", 7001,
            Some(chrono::NaiveDate::from_ymd_opt(2023, 6, 1).unwrap()),
            Some(8.5), Some(140), vec!["Action".to_string()]
        ).await;
        
        create_test_movie(
            &db, library_id, "Action 2023 Low", 7002,
            Some(chrono::NaiveDate::from_ymd_opt(2023, 7, 1).unwrap()),
            Some(6.0), Some(100), vec!["Action".to_string()]
        ).await;
        
        create_test_movie(
            &db, library_id, "Drama 2023 High", 7003,
            Some(chrono::NaiveDate::from_ymd_opt(2023, 8, 1).unwrap()),
            Some(9.0), Some(180), vec!["Drama".to_string()]
        ).await;
        
        create_test_movie(
            &db, library_id, "Action 2022 High", 7004,
            Some(chrono::NaiveDate::from_ymd_opt(2022, 1, 1).unwrap()),
            Some(8.0), Some(120), vec!["Action".to_string()]
        ).await;
        
        // Complex query: Action movies from 2023 with rating >= 7.0
        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec!["Action".to_string()],
                year_range: Some((2023, 2023)),
                rating_range: Some((7.0, 10.0)),
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::Rating,
                order: SortOrder::Descending,
                secondary: None,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 1);
        
        if let MediaReference::Movie(movie) = &results[0].media {
            assert_eq!(movie.title.as_str(), "Action 2023 High");
        } else {
            panic!("Expected movie reference");
        }
        
        cleanup_library(&db, library_id).await;
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;
    use helpers::*;

    #[tokio::test]
    async fn test_sort_by_release_date() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies 4", LibraryType::Movies).await;
        
        // Create movies with different release dates
        create_test_movie(
            &db, library_id, "Movie C", 4001,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()),
            None, None, vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Movie A", 4002,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            None, None, vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Movie B", 4003,
            Some(chrono::NaiveDate::from_ymd_opt(2024, 2, 1).unwrap()),
            None, None, vec![]
        ).await;
        
        // Query with release date sorting (ascending)
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::ReleaseDate,
                order: SortOrder::Ascending,
                secondary: None,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 3);
        
        // Check order
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert_eq!(titles, vec!["Movie A", "Movie B", "Movie C"]);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_sort_by_rating() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies 5", LibraryType::Movies).await;
        
        // Create movies with different ratings
        create_test_movie(
            &db, library_id, "Low Movie", 5001,
            None, Some(6.0), None, vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "High Movie", 5002,
            None, Some(9.0), None, vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Mid Movie", 5003,
            None, Some(7.5), None, vec![]
        ).await;
        
        // Query with rating sorting (descending)
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::Rating,
                order: SortOrder::Descending,
                secondary: None,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 3);
        
        // Check order
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert_eq!(titles, vec!["High Movie", "Mid Movie", "Low Movie"]);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_sort_by_runtime() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Runtime Sort", LibraryType::Movies).await;
        
        // Create movies with different runtimes
        create_test_movie(
            &db, library_id, "Short Movie", 8001,
            None, None, Some(90), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Long Movie", 8002,
            None, None, Some(180), vec![]
        ).await;
        
        create_test_movie(
            &db, library_id, "Medium Movie", 8003,
            None, None, Some(120), vec![]
        ).await;
        
        // Query with runtime sorting (ascending)
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::Runtime,
                order: SortOrder::Ascending,
                secondary: None,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 3);
        
        // Check order
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert_eq!(titles, vec!["Short Movie", "Medium Movie", "Long Movie"]);
        
        cleanup_library(&db, library_id).await;
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    use helpers::*;

    #[tokio::test]
    async fn test_basic_pagination() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Movies 6", LibraryType::Movies).await;
        
        // Create 5 movies
        for i in 1..=5 {
            create_test_movie(
                &db, library_id, &format!("Movie {}", i), 6000 + i,
                None, None, None, vec![]
            ).await;
        }
        
        // Query with pagination - page 1
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            pagination: Pagination {
                offset: 0,
                limit: 2,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        // Query with pagination - page 2
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            pagination: Pagination {
                offset: 2,
                limit: 2,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        // Query with pagination - page 3
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            pagination: Pagination {
                offset: 4,
                limit: 2,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 1);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_pagination_with_filters() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Pagination Filters", LibraryType::Movies).await;
        
        // Create action movies
        for i in 1..=10 {
            create_test_movie(
                &db, library_id, &format!("Action Movie {}", i), 9000 + i,
                None, Some(7.0 + (i as f32) * 0.1), None, vec!["Action".to_string()]
            ).await;
        }
        
        // Query action movies with pagination
        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec!["Action".to_string()],
                library_ids: vec![library_id],
                ..Default::default()
            },
            pagination: Pagination {
                offset: 3,
                limit: 3,
            },
            sort: SortCriteria {
                primary: SortField::Rating,
                order: SortOrder::Descending,
                secondary: None,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 3);
        
        cleanup_library(&db, library_id).await;
    }
}