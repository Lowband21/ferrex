#[cfg(test)]
mod tests {
    use ferrex_core::{
        ApiResponse, FetchMediaRequest, LibraryMediaResponse, ManualMatchRequest,
        MediaId, MediaReference, MovieID, SeriesID, MediaDetailsOption,
        MovieReference, SeriesReference, MovieTitle, SeriesTitle, MovieURL, SeriesURL,
        LibraryType, Library, MediaFile, FileType,
    };
    use ferrex_server::{create_app, AppState};
    use axum_test::TestServer;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use uuid::Uuid;
    
    async fn setup_test_db() -> Arc<ferrex_core::MediaDatabase> {
        // Use test database
        let database_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgresql://ferrex:ferrex@localhost:5432/ferrex_test".to_string());
        
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to test database");
            
        // Run migrations
        sqlx::migrate!("../ferrex-core/migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");
            
        Arc::new(ferrex_core::MediaDatabase::postgres(pool))
    }
    
    async fn create_test_app() -> TestServer {
        let db = setup_test_db().await;
        let metadata_service = Arc::new(ferrex_server::metadata_service::MetadataService::new(db.clone()));
        let thumbnail_service = Arc::new(ferrex_server::thumbnail_service::ThumbnailService::new());
        let tmdb_provider = Arc::new(ferrex_core::providers::TmdbApiProvider::new());
        let scan_manager = Arc::new(ferrex_server::scan_manager::ScanManager::new(
            db.clone(),
            metadata_service.clone(),
            thumbnail_service.clone(),
            tmdb_provider.clone(),
        ));
        
        let state = AppState {
            db,
            metadata_service,
            thumbnail_service,
            scan_manager,
        };
        
        let app = create_app(state);
        TestServer::new(app).unwrap()
    }
    
    async fn create_test_library(db: &ferrex_core::MediaDatabase) -> Uuid {
        let library = Library {
            id: Uuid::new_v4(),
            name: "Test Movies".to_string(),
            library_type: LibraryType::Movies,
            paths: vec![std::path::PathBuf::from("/test/movies")],
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        
        db.backend().store_library(library.clone()).await.unwrap();
        library.id
    }
    
    async fn create_test_movie_reference(db: &ferrex_core::MediaDatabase, library_id: Uuid) -> MovieReference {
        let file = MediaFile {
            id: Uuid::new_v4(),
            path: std::path::PathBuf::from("/test/movies/test_movie.mp4"),
            name: "test_movie.mp4".to_string(),
            size: 1_000_000,
            media_type: Some(FileType::Movie),
            parent_directory: Some(std::path::PathBuf::from("/test/movies")),
            library_id: Some(library_id),
            parent_media_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_scanned: Some(chrono::Utc::now()),
            metadata: None,
        };
        
        let movie = MovieReference {
            id: MovieID::new(Uuid::new_v4().to_string()).unwrap(),
            tmdb_id: 550, // Fight Club
            title: MovieTitle::new("Test Movie").unwrap(),
            details: MediaDetailsOption::Endpoint(
                url::Url::parse(&format!("/api/movie/{}", Uuid::new_v4())).unwrap()
            ),
            endpoint: MovieURL::new(
                url::Url::parse(&format!("/api/stream/{}", file.id)).unwrap()
            ),
            file,
        };
        
        db.backend().store_movie_reference(&movie).await.unwrap();
        movie
    }
    
    #[tokio::test]
    async fn test_get_library_media_empty() {
        let server = create_test_app().await;
        let db = setup_test_db().await;
        let library_id = create_test_library(&db).await;
        
        let response = server
            .get(&format!("/libraries/{}/media", library_id))
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<LibraryMediaResponse> = response.json();
        assert!(body.success);
        assert_eq!(body.data.unwrap().media.len(), 0);
    }
    
    #[tokio::test]
    async fn test_get_library_media_with_movies() {
        let server = create_test_app().await;
        let db = setup_test_db().await;
        let library_id = create_test_library(&db).await;
        
        // Add test movie
        let movie = create_test_movie_reference(&db, library_id).await;
        
        let response = server
            .get(&format!("/libraries/{}/media", library_id))
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<LibraryMediaResponse> = response.json();
        assert!(body.success);
        let data = body.data.unwrap();
        assert_eq!(data.media.len(), 1);
        
        match &data.media[0] {
            MediaReference::Movie(m) => {
                assert_eq!(m.id, movie.id);
                assert_eq!(m.tmdb_id, movie.tmdb_id);
                assert_eq!(m.title, movie.title);
            }
            _ => panic!("Expected movie reference"),
        }
    }
    
    #[tokio::test]
    async fn test_fetch_movie_with_tmdb_details() {
        let server = create_test_app().await;
        let db = setup_test_db().await;
        let library_id = create_test_library(&db).await;
        let movie = create_test_movie_reference(&db, library_id).await;
        
        let request = FetchMediaRequest {
            library_id,
            media_id: MediaId::Movie(movie.id.clone()),
        };
        
        let response = server
            .post("/media")
            .json(&request)
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<MediaReference> = response.json();
        assert!(body.success);
        
        match body.data.unwrap() {
            MediaReference::Movie(m) => {
                assert_eq!(m.id, movie.id);
                // With a valid TMDB API key, this would fetch details
                // For now, it should at least return the endpoint
                assert!(matches!(m.details, MediaDetailsOption::Endpoint(_)));
            }
            _ => panic!("Expected movie reference"),
        }
    }
    
    #[tokio::test]
    async fn test_manual_match_movie() {
        let server = create_test_app().await;
        let db = setup_test_db().await;
        let library_id = create_test_library(&db).await;
        let movie = create_test_movie_reference(&db, library_id).await;
        
        let request = ManualMatchRequest {
            media_id: MediaId::Movie(movie.id.clone()),
            tmdb_id: 680, // Pulp Fiction
        };
        
        let response = server
            .post("/media/match")
            .json(&request)
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<String> = response.json();
        assert!(body.success);
        assert_eq!(body.data.unwrap(), "Movie TMDB ID updated");
        
        // Verify the update
        let updated = db.backend().get_movie_reference(&movie.id).await.unwrap();
        assert_eq!(updated.tmdb_id, 680);
    }
    
    #[tokio::test]
    async fn test_get_library_media_invalid_library() {
        let server = create_test_app().await;
        let invalid_id = Uuid::new_v4();
        
        let response = server
            .get(&format!("/libraries/{}/media", invalid_id))
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<LibraryMediaResponse> = response.json();
        assert!(!body.success);
        assert!(body.error.is_some());
    }
    
    #[tokio::test]
    async fn test_fetch_movie_not_found() {
        let server = create_test_app().await;
        let library_id = Uuid::new_v4();
        let movie_id = MovieID::new(Uuid::new_v4().to_string()).unwrap();
        
        let request = FetchMediaRequest {
            library_id,
            media_id: MediaId::Movie(movie_id),
        };
        
        let response = server
            .post("/media")
            .json(&request)
            .await;
            
        response.assert_status(200);
        
        let body: ApiResponse<MediaReference> = response.json();
        assert!(!body.success);
        assert!(body.error.is_some());
    }
    
    // TODO: Add tests for series, seasons, and episodes once they're fully implemented
}