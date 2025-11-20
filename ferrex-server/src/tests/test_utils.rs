#[cfg(test)]
use crate::{config::Config, AppState};
use axum::{body::Body, http::{self, Request, Response}, Router};
use ferrex_core::{
    media::*,
    user::{AuthToken, User},
    MediaDatabase, MediaFile,
};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

/// Create a test AppState with in-memory database
pub async fn setup_test_state() -> AppState {
    // Use test database URL from env or default
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://ferrex:ferrex@localhost:5432/ferrex_test".to_string());
    
    let config = Arc::new(Config {
        database_url: Some(database_url),
        redis_url: None,
        media_root: Some(std::path::PathBuf::from("/tmp/test_media")),
        cache_dir: std::path::PathBuf::from("/tmp/test_cache"),
        server_host: "127.0.0.1".to_string(),
        server_port: 3001,
        ffmpeg_path: "ffmpeg".to_string(),
        ffprobe_path: "ffprobe".to_string(),
        transcode_cache_dir: std::path::PathBuf::from("/tmp/test_transcode"),
        thumbnail_cache_dir: std::path::PathBuf::from("/tmp/test_thumbnails"),
        cors_allowed_origins: vec!["http://localhost:3000".to_string()],
        dev_mode: true,
    });

    // Create cache directories
    let _ = std::fs::create_dir_all(&config.cache_dir);
    let _ = std::fs::create_dir_all(config.cache_dir.join("images"));

    let db = Arc::new(
        MediaDatabase::new_postgres(&config.database_url.as_ref().unwrap(), false)
            .await
            .expect("Failed to create test database"),
    );

    // Initialize schema
    db.backend()
        .initialize_schema()
        .await
        .expect("Failed to initialize test schema");

    let metadata_service = Arc::new(crate::metadata_service::MetadataService::new(
        None,
        config.cache_dir.clone(),
    ));

    let thumbnail_service = Arc::new(
        crate::thumbnail_service::ThumbnailService::new(config.cache_dir.clone(), db.clone())
            .expect("Failed to initialize thumbnail service"),
    );

    let scan_manager = Arc::new(crate::scan_manager::ScanManager::new(
        db.clone(),
        metadata_service.clone(),
        thumbnail_service.clone(),
    ));

    let transcoding_service = Arc::new(
        crate::transcoding::TranscodingService::new(
            crate::transcoding::config::TranscodingConfig::default(),
            db.clone(),
        )
        .await
        .expect("Failed to initialize transcoding service"),
    );

    let image_service = Arc::new(ferrex_core::ImageService::new(
        db.clone(),
        config.cache_dir.clone(),
    ));

    let websocket_manager = Arc::new(crate::websocket::ConnectionManager::new());

    AppState {
        db: db.clone(),
        database: db,
        config,
        metadata_service,
        thumbnail_service,
        scan_manager,
        transcoding_service,
        image_service,
        websocket_manager,
    }
}

/// Create a test movie reference with full metadata
pub fn create_test_movie_reference() -> MovieReference {
    let movie_id = MovieID::new(Uuid::new_v4().to_string()).unwrap();
    let media_file = create_test_media_file();

    MovieReference {
        id: movie_id,
        tmdb_id: 12345,
        title: MovieTitle::new("Test Movie".to_string()).unwrap(),
        details: MediaDetailsOption::Details(TmdbDetails::Movie(EnhancedMovieDetails {
            id: 12345,
            title: "Test Movie".to_string(),
            overview: Some("This is a test movie".to_string()),
            release_date: Some("2024-01-01".to_string()),
            runtime: Some(120),
            vote_average: Some(8.5),
            vote_count: Some(1000),
            popularity: Some(50.0),
            genres: vec!["Action".to_string(), "Adventure".to_string()],
            production_companies: vec!["Test Studios".to_string()],
            poster_path: Some("/images/movie/12345/poster/0".to_string()),
            backdrop_path: Some("/images/movie/12345/backdrop/0".to_string()),
            logo_path: None,
            images: MediaImages {
                posters: vec![
                    ImageWithMetadata {
                        endpoint: "/images/movie/12345/poster/0".to_string(),
                        metadata: ImageMetadata {
                            file_path: "/test_poster.jpg".to_string(),
                            width: 500,
                            height: 750,
                            aspect_ratio: 0.667,
                            iso_639_1: Some("en".to_string()),
                            vote_average: 5.5,
                            vote_count: 10,
                        },
                    },
                ],
                backdrops: vec![
                    ImageWithMetadata {
                        endpoint: "/images/movie/12345/backdrop/0".to_string(),
                        metadata: ImageMetadata {
                            file_path: "/test_backdrop.jpg".to_string(),
                            width: 1920,
                            height: 1080,
                            aspect_ratio: 1.778,
                            iso_639_1: Some("en".to_string()),
                            vote_average: 6.0,
                            vote_count: 5,
                        },
                    },
                ],
                logos: vec![],
                stills: vec![],
            },
            cast: vec![
                CastMember {
                    id: 1,
                    name: "Test Actor".to_string(),
                    character: "Main Character".to_string(),
                    profile_path: Some("/test_profile.jpg".to_string()),
                    order: 0,
                },
            ],
            crew: vec![
                CrewMember {
                    id: 2,
                    name: "Test Director".to_string(),
                    job: "Director".to_string(),
                    department: "Directing".to_string(),
                    profile_path: None,
                },
            ],
            videos: vec![],
            keywords: vec!["test".to_string(), "movie".to_string()],
            external_ids: ExternalIds {
                imdb_id: Some("tt1234567".to_string()),
                ..Default::default()
            },
        })),
        endpoint: MovieURL::from_string("/api/stream/test-movie".to_string()),
        file: media_file,
        theme_color: None,
    }
}

/// Create a test series reference with full metadata
pub fn create_test_series_reference() -> SeriesReference {
    let series_id = SeriesID::new(Uuid::new_v4().to_string()).unwrap();

    SeriesReference {
        id: series_id,
        tmdb_id: 54321,
        title: SeriesTitle::new("Test Series".to_string()).unwrap(),
        details: MediaDetailsOption::Details(TmdbDetails::Series(EnhancedSeriesDetails {
            id: 54321,
            name: "Test Series".to_string(),
            overview: Some("This is a test TV series".to_string()),
            first_air_date: Some("2023-01-01".to_string()),
            last_air_date: Some("2024-01-01".to_string()),
            number_of_seasons: Some(2),
            number_of_episodes: Some(20),
            vote_average: Some(8.0),
            vote_count: Some(500),
            popularity: Some(40.0),
            genres: vec!["Drama".to_string(), "Mystery".to_string()],
            networks: vec!["Test Network".to_string()],
            poster_path: Some("/images/series/54321/poster/0".to_string()),
            backdrop_path: Some("/images/series/54321/backdrop/0".to_string()),
            logo_path: None,
            images: MediaImages {
                posters: vec![
                    ImageWithMetadata {
                        endpoint: "/images/series/54321/poster/0".to_string(),
                        metadata: ImageMetadata {
                            file_path: "/test_series_poster.jpg".to_string(),
                            width: 500,
                            height: 750,
                            aspect_ratio: 0.667,
                            iso_639_1: Some("en".to_string()),
                            vote_average: 7.0,
                            vote_count: 15,
                        },
                    },
                ],
                backdrops: vec![],
                logos: vec![],
                stills: vec![],
            },
            cast: vec![],
            crew: vec![],
            videos: vec![],
            keywords: vec![],
            external_ids: ExternalIds::default(),
        })),
        endpoint: SeriesURL::from_string("/api/series/test-series".to_string()),
        library_id: Uuid::new_v4(),
        theme_color: None,
    }
}

/// Create a test season reference
pub fn create_test_season_reference(series_id: SeriesID) -> SeasonReference {
    let season_id = SeasonID::new(Uuid::new_v4().to_string()).unwrap();

    SeasonReference {
        id: season_id,
        season_number: SeasonNumber::new(1),
        series_id,
        tmdb_series_id: 54321,
        details: MediaDetailsOption::Details(TmdbDetails::Season(SeasonDetails {
            id: 11111,
            season_number: 1,
            name: "Season 1".to_string(),
            overview: Some("The first season".to_string()),
            air_date: Some("2023-01-01".to_string()),
            episode_count: 10,
            poster_path: Some("/test_season_poster.jpg".to_string()),
        })),
        endpoint: SeasonURL::from_string("/api/season/test-season-1".to_string()),
        theme_color: None,
    }
}

/// Create a test episode reference
pub fn create_test_episode_reference(season_id: SeasonID, series_id: SeriesID) -> EpisodeReference {
    let episode_id = EpisodeID::new(Uuid::new_v4().to_string()).unwrap();
    let media_file = create_test_media_file();

    EpisodeReference {
        id: episode_id,
        episode_number: EpisodeNumber::new(1),
        season_number: SeasonNumber::new(1),
        season_id,
        series_id,
        tmdb_series_id: 54321,
        details: MediaDetailsOption::Details(TmdbDetails::Episode(EpisodeDetails {
            id: 22222,
            episode_number: 1,
            season_number: 1,
            name: "Pilot".to_string(),
            overview: Some("The first episode".to_string()),
            air_date: Some("2023-01-01".to_string()),
            runtime: Some(45),
            still_path: Some("/test_episode_still.jpg".to_string()),
            vote_average: Some(8.2),
        })),
        endpoint: EpisodeURL::from_string("/api/stream/test-episode".to_string()),
        file: media_file,
    }
}

/// Create a test media file
fn create_test_media_file() -> MediaFile {
    MediaFile {
        id: Uuid::new_v4(),
        path: std::path::PathBuf::from("/tmp/test_movie.mp4"),
        filename: "test_movie.mp4".to_string(),
        size: 1000000,
        created_at: chrono::Utc::now(),
        media_file_metadata: None,
        library_id: Uuid::new_v4(),
    }
}

/// Clean up test database
pub async fn cleanup_test_db(state: &AppState) {
    // TODO: Implement database cleanup for tests
    // Currently there's no clear_all_data method in the trait
    let _ = state; // Suppress unused variable warning
}

/// Setup an authenticated app for integration testing
pub async fn setup_authenticated_app() -> (Router, AppState, User, AuthToken) {
    let state = setup_test_state().await;
    
    // Create a test user with password hash
    let user_id = Uuid::new_v4();
    let password_hash = {
        use argon2::{
            password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
            Argon2,
        };
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(b"testpassword", &salt)
            .unwrap()
            .to_string()
    };
    
    let user = User {
        id: user_id,
        username: "testuser".to_string(),
        display_name: "Test User".to_string(),
        password_hash,
        created_at: chrono::Utc::now().timestamp(),
    };
    
    // Store the user in database
    state.db.backend().create_user(&user).await.expect("Failed to create test user");
    
    // Generate real JWT tokens
    let access_token = crate::auth::jwt::generate_access_token(user.id)
        .expect("Failed to generate access token");
    let refresh_token = crate::auth::jwt::generate_refresh_token();
    
    // Create auth token
    let auth_token = AuthToken {
        access_token,
        refresh_token,
        expires_in: 900,
    };
    
    // Create the app router
    let app = crate::create_app(state.clone());
    
    (app, state, user, auth_token)
}

/// Create a test request with JSON body
pub fn test_request_json<T: serde::Serialize>(
    method: &str,
    uri: &str,
    auth_token: Option<&str>,
    body: &T,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    
    if let Some(token) = auth_token {
        req = req.header("authorization", format!("Bearer {}", token));
    }
    
    req.body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

/// Create a test request without body
pub fn test_request(
    method: &str,
    uri: &str,
    auth_token: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri);
    
    if let Some(token) = auth_token {
        req = req.header("authorization", format!("Bearer {}", token));
    }
    
    req.body(Body::empty())
        .unwrap()
}

/// Parse JSON response from HTTP response
pub async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: Response<Body>,
) -> T {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read response body");
    serde_json::from_slice(&body_bytes).expect("Failed to parse JSON response")
}