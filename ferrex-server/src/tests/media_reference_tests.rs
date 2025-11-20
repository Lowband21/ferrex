#[cfg(test)]
mod tests {
    use super::super::test_utils::*;
    use crate::create_app;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use ferrex_core::media::*;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_get_movie_reference() {
        // Setup test state and database
        let state = setup_test_state().await;
        
        // Create and store a test movie
        let movie_ref = create_test_movie_reference();
        let movie_id = movie_ref.id.as_str().to_string();
        
        state
            .db
            .backend()
            .store_movie_reference(&movie_ref)
            .await
            .expect("Failed to store test movie");

        // Create app and make request
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/{}", movie_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "success");
        assert!(json["media"].is_object());
        
        // Verify it's a movie
        let media = &json["media"];
        assert!(media["Movie"].is_object());
        
        let movie = &media["Movie"];
        assert_eq!(movie["title"]["0"], "Test Movie");
        assert_eq!(movie["tmdb_id"], 12345);
        
        // Verify full metadata is present
        assert!(movie["details"]["Details"].is_object());
        let details = &movie["details"]["Details"]["Movie"];
        assert_eq!(details["title"], "Test Movie");
        assert_eq!(details["genres"], json!(["Action", "Adventure"]));
        assert!(!details["images"]["posters"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_series_reference() {
        // Setup test state and database
        let state = setup_test_state().await;
        
        // Create and store a test series
        let series_ref = create_test_series_reference();
        let series_id = series_ref.id.as_str().to_string();
        
        state
            .db
            .backend()
            .store_series_reference(&series_ref)
            .await
            .expect("Failed to store test series");

        // Create app and make request
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/{}", series_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "success");
        
        // Verify it's a series
        let media = &json["media"];
        assert!(media["Series"].is_object());
        
        let series = &media["Series"];
        assert_eq!(series["title"]["0"], "Test Series");
        assert_eq!(series["tmdb_id"], 54321);
    }

    #[tokio::test]
    async fn test_get_season_reference() {
        // Setup test state and database
        let state = setup_test_state().await;
        
        // Create series first
        let series_ref = create_test_series_reference();
        let series_id = series_ref.id.clone();
        
        state
            .db
            .backend()
            .store_series_reference(&series_ref)
            .await
            .expect("Failed to store test series");
        
        // Create and store a test season
        let season_ref = create_test_season_reference(series_id);
        let season_id = season_ref.id.as_str().to_string();
        
        state
            .db
            .backend()
            .store_season_reference(&season_ref)
            .await
            .expect("Failed to store test season");

        // Create app and make request
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/{}", season_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "success");
        
        // Verify it's a season
        let media = &json["media"];
        assert!(media["Season"].is_object());
        
        let season = &media["Season"];
        assert_eq!(season["season_number"]["0"], 1);
    }

    #[tokio::test]
    async fn test_get_episode_reference() {
        // Setup test state and database
        let state = setup_test_state().await;
        
        // Create series and season first
        let series_ref = create_test_series_reference();
        let series_id = series_ref.id.clone();
        
        state
            .db
            .backend()
            .store_series_reference(&series_ref)
            .await
            .expect("Failed to store test series");
        
        let season_ref = create_test_season_reference(series_id.clone());
        let season_id = season_ref.id.clone();
        
        state
            .db
            .backend()
            .store_season_reference(&season_ref)
            .await
            .expect("Failed to store test season");
        
        // Create and store a test episode
        let episode_ref = create_test_episode_reference(season_id, series_id);
        let episode_id = episode_ref.id.as_str().to_string();
        
        state
            .db
            .backend()
            .store_episode_reference(&episode_ref)
            .await
            .expect("Failed to store test episode");

        // Create app and make request
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/media/{}", episode_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "success");
        
        // Verify it's an episode
        let media = &json["media"];
        assert!(media["Episode"].is_object());
        
        let episode = &media["Episode"];
        assert_eq!(episode["episode_number"]["0"], 1);
        assert_eq!(episode["season_number"]["0"], 1);
    }

    #[tokio::test]
    async fn test_get_nonexistent_media() {
        // Setup test state
        let state = setup_test_state().await;

        // Create app and make request with non-existent ID
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/media/nonexistent-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_image_endpoint() {
        // Setup test state
        let state = setup_test_state().await;

        // Create a dummy image file in cache
        let cache_dir = state.config.cache_dir.join("images");
        std::fs::create_dir_all(&cache_dir).unwrap();
        
        let test_image_path = cache_dir.join("movie_12345_poster_0_test.png");
        std::fs::write(&test_image_path, b"fake image data").unwrap();

        // Create app and make request
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/images/movie/12345/poster/0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify response
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "image/png"
        );
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "public, max-age=31536000"
        );

        // Clean up
        let _ = std::fs::remove_file(test_image_path);
    }

    #[tokio::test]
    async fn test_image_endpoint_not_found() {
        // Setup test state
        let state = setup_test_state().await;

        // Create app and make request for non-existent image
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/images/movie/99999/poster/0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_image_endpoint_invalid_type() {
        // Setup test state
        let state = setup_test_state().await;

        // Create app and make request with invalid media type
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/images/invalid/12345/poster/0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 400
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_image_endpoint_invalid_category() {
        // Setup test state
        let state = setup_test_state().await;

        // Create app and make request with invalid category
        let app = create_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/images/movie/12345/invalid/0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 400
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}