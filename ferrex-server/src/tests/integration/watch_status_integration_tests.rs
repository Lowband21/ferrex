#[cfg(test)]
mod tests {
    use crate::tests::test_utils::*;
    use axum::http::StatusCode;
    use ferrex_core::{
        api_types::MediaID,
        media::{EpisodeID, MovieID},
        watch_status::{InProgressItem, UpdateProgressRequest},
        MediaFile,
    };
    use tower::ServiceExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_update_watch_progress() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Create a test media file
        let media_id = Uuid::new_v4();
        let media_file = create_test_media_file(media_id, "Test Movie.mp4", 7200.0);
        state.db.backend().store_media(media_file).await.unwrap();

        // Update progress
        let request = UpdateProgressRequest {
            media_id: MediaID::Movie(MovieID::new(media_id.to_string()).unwrap()),
            position: 1800.0,
            duration: 7200.0,
        };

        let response = app
            .clone()
            .oneshot(test_request_json(
                "POST",
                "/api/watch/progress",
                Some(&auth_token.access_token),
                &request,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify progress was saved
        let watch_state = state
            .db
            .backend()
            .get_user_watch_state(user.id)
            .await
            .unwrap();
        assert_eq!(watch_state.in_progress.len(), 1);
        assert_eq!(watch_state.in_progress[0].position, 1800.0);
    }

    #[tokio::test]
    async fn test_get_watch_state() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Add some test progress
        let media_id1 = MediaID::Movie(MovieID::new(Uuid::new_v4().to_string()).unwrap());
        let media_id2 = MediaID::Episode(EpisodeID::new(Uuid::new_v4().to_string()).unwrap());

        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id1.clone(),
                    position: 1000.0,
                    duration: 2000.0,
                },
            )
            .await
            .unwrap();

        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id2.clone(),
                    position: 1900.0,
                    duration: 2000.0,
                },
            )
            .await
            .unwrap();

        // Get watch state
        let response = app
            .clone()
            .oneshot(test_request(
                "GET",
                "/api/watch/state",
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body: serde_json::Value = parse_json_response(response).await;
        assert_eq!(body["in_progress"].as_array().unwrap().len(), 1);
        assert_eq!(body["completed_count"], 1);
    }

    #[tokio::test]
    async fn test_get_continue_watching() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Add multiple items with delays
        for i in 0..5 {
            let media_id = MediaID::Movie(MovieID::new(Uuid::new_v4().to_string()).unwrap());
            state
                .db
                .backend()
                .update_watch_progress(
                    user.id,
                    &UpdateProgressRequest {
                        media_id,
                        position: 300.0 + (i as f32 * 100.0),
                        duration: 7200.0,
                    },
                )
                .await
                .unwrap();

            // Small delay to ensure different timestamps
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Get continue watching with limit
        let response = app
            .clone()
            .oneshot(test_request(
                "GET",
                "/api/watch/continue?limit=3",
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let items: Vec<InProgressItem> = parse_json_response(response).await;
        assert_eq!(items.len(), 3);
        // Verify most recent first
        assert!(items[0].last_watched > items[1].last_watched);
        assert!(items[1].last_watched > items[2].last_watched);
    }

    #[tokio::test]
    async fn test_clear_watch_progress() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Add progress
        let media_id = MediaID::Movie(MovieID::new(Uuid::new_v4().to_string()).unwrap());
        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id.clone(),
                    position: 1000.0,
                    duration: 2000.0,
                },
            )
            .await
            .unwrap();

        // Clear progress
        let response = app
            .clone()
            .oneshot(test_request(
                "DELETE",
                &format!("/api/watch/progress/{}", media_id),
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify cleared
        let watch_state = state
            .db
            .backend()
            .get_user_watch_state(user.id)
            .await
            .unwrap();
        assert!(watch_state.in_progress.is_empty());
    }

    #[tokio::test]
    async fn test_get_media_progress() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Create media and add progress
        let media_id = Uuid::new_v4();
        let media_file = create_test_media_file(media_id, "Test Movie.mp4", 7200.0);
        state.db.backend().store_media(media_file).await.unwrap();

        let media_id_str = MediaID::Movie(MovieID::new(media_id.to_string()).unwrap());
        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id_str.clone(),
                    position: 3600.0,
                    duration: 7200.0,
                },
            )
            .await
            .unwrap();

        // Get progress
        let response = app
            .clone()
            .oneshot(test_request(
                "GET",
                &format!("/api/media/{}/progress", media_id),
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body: serde_json::Value = parse_json_response(response).await;
        assert_eq!(body["percentage"], 50.0);
        assert_eq!(body["position"], 3600.0);
        assert_eq!(body["duration"], 7200.0);
        assert_eq!(body["is_completed"], false);
    }

    #[tokio::test]
    async fn test_mark_media_completed() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Create media
        let media_id = Uuid::new_v4();
        let media_file = create_test_media_file(media_id, "Test Movie.mp4", 7200.0);
        state.db.backend().store_media(media_file).await.unwrap();

        // Mark as completed
        let response = app
            .clone()
            .oneshot(test_request(
                "POST",
                &format!("/api/media/{}/complete", media_id),
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Verify completion
        let media_id_str = MediaID::Movie(MovieID::new(media_id.to_string()).unwrap());
        let is_completed = state
            .db
            .backend()
            .is_media_completed(user.id, &media_id_str)
            .await
            .unwrap();
        assert!(is_completed);
    }

    #[tokio::test]
    async fn test_is_media_completed() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Create media and mark as completed
        let media_id = Uuid::new_v4();
        let media_file = create_test_media_file(media_id, "Test Movie.mp4", 7200.0);
        state.db.backend().store_media(media_file).await.unwrap();

        let media_id_str = MediaID::Movie(MovieID::new(media_id.to_string()).unwrap());
        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id_str.clone(),
                    position: 7000.0,
                    duration: 7200.0,
                },
            )
            .await
            .unwrap();

        // Check completion status
        let response = app
            .clone()
            .oneshot(test_request(
                "GET",
                &format!("/api/media/{}/is-completed", media_id),
                Some(&auth_token.access_token),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body: serde_json::Value = parse_json_response(response).await;
        assert_eq!(body["is_completed"], true);
    }

    #[tokio::test]
    async fn test_invalid_progress_values() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        // Test negative position
        let request = UpdateProgressRequest {
            media_id: MediaID::Movie(Uuid::new_v4().to_string()),
            position: -100.0,
            duration: 7200.0,
        };

        let response = app
            .clone()
            .oneshot(test_request_json(
                "POST",
                "/api/watch/progress",
                Some(&auth_token.access_token),
                &request,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Test position > duration
        let request = UpdateProgressRequest {
            media_id: MediaID::Movie(Uuid::new_v4().to_string()),
            position: 8000.0,
            duration: 7200.0,
        };

        let response = app
            .clone()
            .oneshot(test_request_json(
                "POST",
                "/api/watch/progress",
                Some(&auth_token.access_token),
                &request,
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_progress_completion_threshold() {
        let (app, state, user, auth_token) = setup_authenticated_app().await;

        let media_id = MediaID::Movie(MovieID::new(Uuid::new_v4().to_string()).unwrap());

        // Update to 94% - should still be in progress
        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id.clone(),
                    position: 6768.0, // 94% of 7200
                    duration: 7200.0,
                },
            )
            .await
            .unwrap();

        let watch_state = state
            .db
            .backend()
            .get_user_watch_state(user.id)
            .await
            .unwrap();
        assert_eq!(watch_state.in_progress.len(), 1);
        assert!(!watch_state.is_completed(&media_id));

        // Update to 96% - should be completed
        state
            .db
            .backend()
            .update_watch_progress(
                user.id,
                &UpdateProgressRequest {
                    media_id: media_id.clone(),
                    position: 6912.0, // 96% of 7200
                    duration: 7200.0,
                },
            )
            .await
            .unwrap();

        let watch_state = state
            .db
            .backend()
            .get_user_watch_state(user.id)
            .await
            .unwrap();
        assert_eq!(watch_state.in_progress.len(), 0);
        assert!(watch_state.is_completed(&media_id));
    }

    #[tokio::test]
    async fn test_unauthorized_access() {
        let (app, _, _, _) = setup_authenticated_app().await;

        // Try to access without auth token
        let response = app
            .clone()
            .oneshot(test_request("GET", "/api/watch/state", None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // Helper function to create test media file
    fn create_test_media_file(id: Uuid, filename: &str, duration: f64) -> MediaFile {
        use std::path::PathBuf;

        MediaFile {
            id,
            filename: filename.to_string(),
            path: PathBuf::from(format!("/test/{}", filename)),
            size: 1000000,
            created_at: chrono::Utc::now(),
            library_id: Uuid::new_v4(),
            media_file_metadata: Some(ferrex_core::MediaFileMetadata {
                duration: Some(duration),
                file_size: 1000000,
                width: None,
                height: None,
                video_codec: None,
                audio_codec: None,
                bitrate: None,
                framerate: None,
                color_primaries: None,
                color_transfer: None,
                color_space: None,
                bit_depth: None,
                parsed_info: None,
            }),
        }
    }
}
