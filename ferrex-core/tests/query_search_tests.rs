use ferrex_core::{
    database::{postgres::PostgresDatabase, traits::MediaDatabaseTrait},
    query::*,
    media::*,
    LibraryType,
};
use uuid::Uuid;
use sqlx::PgPool;

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

    pub async fn create_test_movie_with_metadata(
        db: &PostgresDatabase,
        library_id: Uuid,
        title: &str,
        tmdb_id: i64,
        overview: &str,
        cast: Vec<String>,
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
        
        // Create movie metadata with overview and cast
        let cast_crew = serde_json::json!({
            "cast": cast.iter().map(|name| {
                serde_json::json!({
                    "id": 1,
                    "name": name,
                    "character": "Character"
                })
            }).collect::<Vec<_>>()
        });
        
        let tmdb_details = serde_json::json!({
            "id": tmdb_id,
            "title": title,
            "overview": overview
        });
        
        sqlx::query!(
            r#"
            INSERT INTO movie_metadata (movie_id, tmdb_details, overview, cast_crew, cast_names)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            movie_id,
            tmdb_details,
            overview,
            cast_crew,
            &cast
        )
        .execute(db.pool())
        .await
        .expect("Failed to create test movie metadata");
        
        (movie_id, file_id)
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use helpers::*;

    #[tokio::test]
    async fn test_search_by_title_exact() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Search Title", LibraryType::Movies).await;
        
        // Create test movies
        create_test_movie_with_metadata(
            &db, library_id, "The Matrix", 10001,
            "A computer hacker learns about the true nature of reality.",
            vec!["Keanu Reeves".to_string(), "Laurence Fishburne".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "The Matrix Reloaded", 10002,
            "Neo and the rebels continue their fight against the machines.",
            vec!["Keanu Reeves".to_string(), "Hugo Weaving".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "Inception", 10003,
            "A thief who enters people's dreams takes on a dangerous mission.",
            vec!["Leonardo DiCaprio".to_string(), "Tom Hardy".to_string()]
        ).await;
        
        // Search for "Matrix" in title only
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "Matrix".to_string(),
                fields: vec![SearchField::Title],
                fuzzy: false,
            }),
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        // Verify both Matrix movies are returned
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert!(titles.contains(&"The Matrix".to_string()));
        assert!(titles.contains(&"The Matrix Reloaded".to_string()));
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_search_by_overview() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Search Overview", LibraryType::Movies).await;
        
        // Create test movies
        create_test_movie_with_metadata(
            &db, library_id, "The Matrix", 11001,
            "A computer hacker learns about the true nature of reality.",
            vec![]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "Hackers", 11002,
            "Young hackers discover a criminal conspiracy.",
            vec![]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "The Social Network", 11003,
            "The story of Facebook's founding.",
            vec![]
        ).await;
        
        // Search for "hacker" in overview
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "hacker".to_string(),
                fields: vec![SearchField::Overview],
                fuzzy: false,
            }),
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert!(titles.contains(&"The Matrix".to_string()));
        assert!(titles.contains(&"Hackers".to_string()));
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_search_by_cast() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Search Cast", LibraryType::Movies).await;
        
        // Create test movies with cast
        create_test_movie_with_metadata(
            &db, library_id, "The Matrix", 12001,
            "A sci-fi action film.",
            vec!["Keanu Reeves".to_string(), "Laurence Fishburne".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "John Wick", 12002,
            "An ex-hitman seeks vengeance.",
            vec!["Keanu Reeves".to_string(), "Willem Dafoe".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "The Avengers", 12003,
            "Earth's mightiest heroes assemble.",
            vec!["Robert Downey Jr.".to_string(), "Chris Evans".to_string()]
        ).await;
        
        // Search for "Keanu Reeves" in cast
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "Keanu Reeves".to_string(),
                fields: vec![SearchField::Cast],
                fuzzy: false,
            }),
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 2);
        
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert!(titles.contains(&"The Matrix".to_string()));
        assert!(titles.contains(&"John Wick".to_string()));
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_fuzzy_search() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Fuzzy Search", LibraryType::Movies).await;
        
        // Create test movies
        create_test_movie_with_metadata(
            &db, library_id, "Interstellar", 13001,
            "A journey through space and time.",
            vec![]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "Inception", 13002,
            "Dreams within dreams.",
            vec![]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "The Prestige", 13003,
            "A story about rival magicians.",
            vec![]
        ).await;
        
        // Fuzzy search for misspelled "intersteller"
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "intersteller".to_string(),
                fields: vec![SearchField::Title],
                fuzzy: true,
            }),
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert!(results.len() >= 1);
        
        // Should find "Interstellar" despite misspelling
        let found = results.iter().any(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str() == "Interstellar"
            } else {
                false
            }
        });
        assert!(found);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_search_all_fields() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test All Fields", LibraryType::Movies).await;
        
        // Create a movie where "Batman" appears in different fields
        create_test_movie_with_metadata(
            &db, library_id, "The Dark Knight", 14001,
            "Batman protects Gotham from the Joker.",
            vec!["Christian Bale".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "Batman Begins", 14002,
            "The origin story of the caped crusader.",
            vec!["Christian Bale".to_string()]
        ).await;
        
        create_test_movie_with_metadata(
            &db, library_id, "Joker", 14003,
            "The origin of Batman's greatest enemy.",
            vec!["Joaquin Phoenix".to_string()]
        ).await;
        
        // Search for "Batman" in all fields
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "Batman".to_string(),
                fields: vec![SearchField::All],
                fuzzy: false,
            }),
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 3); // All three movies mention Batman
        
        let titles: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        assert!(titles.contains(&"The Dark Knight".to_string()));
        assert!(titles.contains(&"Batman Begins".to_string()));
        assert!(titles.contains(&"Joker".to_string()));
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_search_with_pagination() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Test Search Pagination", LibraryType::Movies).await;
        
        // Create 10 movies with "Action" in the title
        for i in 1..=10 {
            create_test_movie_with_metadata(
                &db, library_id, &format!("Action Movie {}", i), 15000 + i,
                "An action-packed adventure.",
                vec![]
            ).await;
        }
        
        // Search for "Action" with pagination
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "Action".to_string(),
                fields: vec![SearchField::Title],
                fuzzy: false,
            }),
            pagination: Pagination {
                offset: 0,
                limit: 5,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        assert_eq!(results.len(), 5);
        
        // Get next page
        let query_page2 = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "Action".to_string(),
                fields: vec![SearchField::Title],
                fuzzy: false,
            }),
            pagination: Pagination {
                offset: 5,
                limit: 5,
            },
            ..Default::default()
        };
        
        let results_page2 = db.query_media(&query_page2).await.expect("Query failed");
        assert_eq!(results_page2.len(), 5);
        
        // Ensure no overlap between pages
        let titles_page1: Vec<String> = results.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        let titles_page2: Vec<String> = results_page2.iter().map(|r| {
            if let MediaReference::Movie(movie) = &r.media {
                movie.title.as_str().to_string()
            } else {
                panic!("Expected movie reference");
            }
        }).collect();
        
        for title in &titles_page1 {
            assert!(!titles_page2.contains(title));
        }
        
        cleanup_library(&db, library_id).await;
    }
}