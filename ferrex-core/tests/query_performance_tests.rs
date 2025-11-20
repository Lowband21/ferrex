use ferrex_core::{
    database::{postgres::PostgresDatabase, traits::MediaDatabaseTrait},
    query::*,
    media::*,
    LibraryType,
};
use uuid::Uuid;
use sqlx::PgPool;
use std::time::Instant;

#[cfg(test)]
mod helpers {
    use super::*;
    
    pub async fn setup_test_db() -> PostgresDatabase {
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

    pub async fn bulk_insert_movies(
        db: &PostgresDatabase,
        library_id: Uuid,
        count: usize,
    ) {
        // Prepare bulk data
        let mut file_ids = Vec::new();
        let mut movie_ids = Vec::new();
        let mut file_values = Vec::new();
        let mut movie_values = Vec::new();
        let mut metadata_values = Vec::new();
        
        for i in 0..count {
            let file_id = Uuid::new_v4();
            let movie_id = Uuid::new_v4();
            file_ids.push(file_id);
            movie_ids.push(movie_id);
            
            // Vary the data to test different filters
            let year = 2015 + (i % 10);
            let month = (i % 12) + 1;
            let rating = 5.0 + (i % 50) as f32 / 10.0;
            let runtime = 90 + (i % 90);
            let genre = match i % 5 {
                0 => "Action",
                1 => "Drama",
                2 => "Comedy",
                3 => "Thriller",
                _ => "Sci-Fi",
            };
            
            file_values.push(format!(
                "('{}'::uuid, '{}'::uuid, '/test/movie_{}_{}.mp4', 'movie_{}.mp4', 1000000)",
                file_id, library_id, i, file_id, i
            ));
            
            movie_values.push(format!(
                "('{}'::uuid, '{}'::uuid, '{}'::uuid, {}, 'Test Movie {}')",
                movie_id, library_id, file_id, 100000 + i, i
            ));
            
            let tmdb_details = serde_json::json!({
                "id": 100000 + i,
                "title": format!("Test Movie {}", i),
                "release_date": format!("{}-{:02}-01", year, month),
                "vote_average": rating,
                "runtime": runtime,
                "popularity": 50.0 + (i % 100) as f32,
                "genres": [{"id": 1, "name": genre}],
                "overview": format!("This is test movie {} about {} adventures", i, genre.to_lowercase())
            });
            
            let cast_crew = serde_json::json!({
                "cast": [
                    {"id": 1, "name": format!("Actor {}", i % 20), "character": "Lead"},
                    {"id": 2, "name": format!("Actor {}", (i + 1) % 20), "character": "Support"}
                ]
            });
            
            metadata_values.push((
                movie_id,
                tmdb_details,
                format!("{}-{:02}-01", year, month),
                rating,
                runtime,
                vec![genre.to_string()],
                vec![format!("Actor {}", i % 20), format!("Actor {}", (i + 1) % 20)],
                year
            ));
        }
        
        // Bulk insert media files
        let file_query = format!(
            "INSERT INTO media_files (id, library_id, file_path, filename, file_size) VALUES {}",
            file_values.join(", ")
        );
        sqlx::query(&file_query)
            .execute(db.pool())
            .await
            .expect("Failed to bulk insert media files");
        
        // Bulk insert movie references
        let movie_query = format!(
            "INSERT INTO movie_references (id, library_id, file_id, tmdb_id, title) VALUES {}",
            movie_values.join(", ")
        );
        sqlx::query(&movie_query)
            .execute(db.pool())
            .await
            .expect("Failed to bulk insert movie references");
        
        // Insert metadata (can't easily bulk insert JSONB, so do in batches)
        for chunk in metadata_values.chunks(100) {
            for (movie_id, tmdb_details, release_date, vote_average, runtime, genres, cast, year) in chunk {
                sqlx::query!(
                    r#"
                    INSERT INTO movie_metadata (
                        movie_id, tmdb_details, release_date, vote_average, 
                        runtime, genre_names, cast_names, release_year, 
                        overview, popularity
                    )
                    VALUES ($1, $2, $3::date, $4, $5, $6, $7, $8, $9, $10)
                    "#,
                    movie_id,
                    tmdb_details,
                    chrono::NaiveDate::parse_from_str(release_date, "%Y-%m-%d").unwrap(),
                    {
                        use std::str::FromStr;
                        sqlx::types::BigDecimal::from_str(&vote_average.to_string()).unwrap()
                    },
                    *runtime as i32,
                    genres,
                    cast,
                    *year as i32,
                    tmdb_details["overview"].as_str(),
                    tmdb_details["popularity"].as_f64().map(|p| {
                        use std::str::FromStr;
                        sqlx::types::BigDecimal::from_str(&p.to_string()).unwrap()
                    })
                )
                .execute(db.pool())
                .await
                .expect("Failed to insert movie metadata");
            }
        }
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use helpers::*;

    #[tokio::test]
    async fn test_large_dataset_query_performance() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Performance Test", LibraryType::Movies).await;
        
        // Insert 1000 movies
        println!("Inserting 1000 test movies...");
        let insert_start = Instant::now();
        bulk_insert_movies(&db, library_id, 1000).await;
        println!("Insert completed in {:?}", insert_start.elapsed());
        
        // Refresh materialized view
        sqlx::query!("SELECT refresh_media_query_view()")
            .execute(db.pool())
            .await
            .expect("Failed to refresh materialized view");
        
        // Test 1: Simple query with pagination
        let start = Instant::now();
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            pagination: Pagination {
                offset: 0,
                limit: 20,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        let duration = start.elapsed();
        
        assert_eq!(results.len(), 20);
        assert!(duration.as_millis() < 50, "Simple query took {:?}, expected < 50ms", duration);
        println!("Simple query completed in {:?}", duration);
        
        // Test 2: Complex filtered query
        let start = Instant::now();
        let query = MediaQuery {
            filters: MediaFilters {
                genres: vec!["Action".to_string()],
                year_range: Some((2018, 2022)),
                rating_range: Some((7.0, 10.0)),
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::Rating,
                order: SortOrder::Descending,
                secondary: None,
            },
            pagination: Pagination {
                offset: 0,
                limit: 10,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        let duration = start.elapsed();
        
        assert!(!results.is_empty());
        assert!(duration.as_millis() < 100, "Complex query took {:?}, expected < 100ms", duration);
        println!("Complex filtered query completed in {:?}", duration);
        
        // Test 3: Search query
        let start = Instant::now();
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "adventure".to_string(),
                fields: vec![SearchField::Overview],
                fuzzy: false,
            }),
            pagination: Pagination {
                offset: 0,
                limit: 20,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        let duration = start.elapsed();
        
        assert!(!results.is_empty());
        assert!(duration.as_millis() < 150, "Search query took {:?}, expected < 150ms", duration);
        println!("Search query completed in {:?}", duration);
        
        // Test 4: Fuzzy search
        let start = Instant::now();
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            search: Some(SearchQuery {
                text: "advanture".to_string(), // Misspelled
                fields: vec![SearchField::Overview],
                fuzzy: true,
            }),
            pagination: Pagination {
                offset: 0,
                limit: 10,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        let duration = start.elapsed();
        
        assert!(!results.is_empty(), "Fuzzy search should find results for misspelled word");
        assert!(duration.as_millis() < 200, "Fuzzy search took {:?}, expected < 200ms", duration);
        println!("Fuzzy search completed in {:?}", duration);
        
        // Test 5: Deep pagination
        let start = Instant::now();
        let query = MediaQuery {
            filters: MediaFilters {
                library_ids: vec![library_id],
                ..Default::default()
            },
            sort: SortCriteria {
                primary: SortField::Title,
                order: SortOrder::Ascending,
                secondary: None,
            },
            pagination: Pagination {
                offset: 900,
                limit: 50,
            },
            ..Default::default()
        };
        
        let results = db.query_media(&query).await.expect("Query failed");
        let duration = start.elapsed();
        
        assert_eq!(results.len(), 50);
        assert!(duration.as_millis() < 100, "Deep pagination query took {:?}, expected < 100ms", duration);
        println!("Deep pagination query completed in {:?}", duration);
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_sort_performance() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Sort Performance Test", LibraryType::Movies).await;
        
        // Insert 500 movies
        bulk_insert_movies(&db, library_id, 500).await;
        
        // Test each sort field
        let sort_fields = vec![
            (SortField::Title, "Title"),
            (SortField::ReleaseDate, "Release Date"),
            (SortField::Rating, "Rating"),
            (SortField::Runtime, "Runtime"),
            (SortField::DateAdded, "Date Added"),
        ];
        
        for (field, name) in sort_fields {
            let start = Instant::now();
            let query = MediaQuery {
                filters: MediaFilters {
                    library_ids: vec![library_id],
                    ..Default::default()
                },
                sort: SortCriteria {
                    primary: field,
                    order: SortOrder::Descending,
                    secondary: None,
                },
                pagination: Pagination {
                    offset: 0,
                    limit: 20,
                },
                ..Default::default()
            };
            
            let results = db.query_media(&query).await.expect("Query failed");
            let duration = start.elapsed();
            
            assert_eq!(results.len(), 20);
            assert!(duration.as_millis() < 50, "Sort by {} took {:?}, expected < 50ms", name, duration);
            println!("Sort by {} completed in {:?}", name, duration);
        }
        
        cleanup_library(&db, library_id).await;
    }

    #[tokio::test]
    async fn test_concurrent_queries() {
        let db = setup_test_db().await;
        let library_id = create_test_library(&db, "Concurrent Test", LibraryType::Movies).await;
        
        // Insert 200 movies
        bulk_insert_movies(&db, library_id, 200).await;
        
        // Run 10 concurrent queries
        let start = Instant::now();
        let mut handles = vec![];
        
        for i in 0..10 {
            let db_clone = db.clone();
            let handle = tokio::spawn(async move {
                let query = MediaQuery {
                    filters: MediaFilters {
                        library_ids: vec![library_id],
                        ..Default::default()
                    },
                    pagination: Pagination {
                        offset: i * 10,
                        limit: 10,
                    },
                    ..Default::default()
                };
                
                db_clone.query_media(&query).await
            });
            handles.push(handle);
        }
        
        // Wait for all queries to complete
        let mut total_results = 0;
        for handle in handles {
            let results = handle.await.unwrap().expect("Query failed");
            total_results += results.len();
        }
        
        let duration = start.elapsed();
        
        assert_eq!(total_results, 100);
        assert!(duration.as_millis() < 500, "Concurrent queries took {:?}, expected < 500ms", duration);
        println!("10 concurrent queries completed in {:?}", duration);
        
        cleanup_library(&db, library_id).await;
    }
}