#[cfg(test)]
mod auth_integration_tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use tower::ServiceExt;
    use serde_json::json;
    use ferrex_core::user::*;
    use crate::tests::fixtures::user_fixtures::*;

    // Mock auth handler for testing
    async fn mock_login_handler(
        axum::Json(request): axum::Json<LoginRequest>,
    ) -> Result<axum::Json<AuthToken>, StatusCode> {
        // Simple mock validation
        if request.username == "testuser" && request.password == TEST_PASSWORD {
            Ok(axum::Json(AuthTokenBuilder::new().build()))
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    }

    async fn mock_register_handler(
        axum::Json(request): axum::Json<RegisterRequest>,
    ) -> Result<axum::Json<User>, StatusCode> {
        // Validate request
        request.validate().map_err(|_| StatusCode::BAD_REQUEST)?;

        // Mock user creation
        let user = UserBuilder::new()
            .with_username(&request.username)
            .with_display_name(&request.display_name)
            .build();

        Ok(axum::Json(user))
    }

    fn create_test_app() -> Router {
        Router::new()
            .route("/auth/login", axum::routing::post(mock_login_handler))
            .route("/auth/register", axum::routing::post(mock_register_handler))
    }

    #[tokio::test]
    async fn test_login_success() {
        let app = create_test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "username": "testuser",
                    "password": TEST_PASSWORD,
                    "device_name": "Test Device"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let auth_token: AuthToken = serde_json::from_slice(&body_bytes).unwrap();

        assert!(!auth_token.access_token.is_empty());
        assert!(!auth_token.refresh_token.is_empty());
        assert_eq!(auth_token.expires_in, 900);
    }

    #[tokio::test]
    async fn test_login_invalid_credentials() {
        let app = create_test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "username": "testuser",
                    "password": "wrong_password"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_register_success() {
        let app = create_test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "username": "newuser",
                    "password": "password123",
                    "display_name": "New User"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let user: User = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(user.username, "newuser");
        assert_eq!(user.display_name, "New User");
    }

    #[tokio::test]
    async fn test_register_validation_failure() {
        let app = create_test_app();

        // Test with invalid username
        let request = Request::builder()
            .method("POST")
            .uri("/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "username": "ab", // Too short
                    "password": "password123",
                    "display_name": "Test User"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_concurrent_login_attempts() {
        use futures::future::join_all;

        let app = create_test_app();

        // Simulate 10 concurrent login attempts
        let mut handles = vec![];

        for i in 0..10 {
            let app_clone = app.clone();
            let handle = tokio::spawn(async move {
                let request = Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&json!({
                            "username": "testuser",
                            "password": TEST_PASSWORD,
                            "device_name": format!("Device {}", i)
                        }))
                        .unwrap(),
                    ))
                    .unwrap();

                app_clone.oneshot(request).await.unwrap()
            });
            handles.push(handle);
        }

        let results = join_all(handles).await;

        // All should succeed
        for result in results {
            let response = result.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}