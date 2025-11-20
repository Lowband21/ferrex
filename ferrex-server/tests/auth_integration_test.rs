use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use ferrex_core::{api_types::ApiResponse, user::*};
use serde_json::json;
use tower::ServiceExt;

// Re-use the test utilities from the main crate
use ferrex_server::tests::test_utils::{setup_test_state, test_request_json, parse_json_response};

/// Test the complete authentication flow:
/// 1. Create test user via registration
/// 2. Authenticate with password
/// 3. Verify session is created and tokens work
#[tokio::test]
async fn test_complete_auth_flow() {
    // Setup test environment
    let state = setup_test_state().await;
    
    // Create the actual app with real handlers
    let app = ferrex_server::create_app(state.clone());
    
    // Step 1: Register a new user
    let register_request = RegisterRequest {
        username: "integrationtest".to_string(),
        password: "testpassword123".to_string(),
        display_name: "Integration Test User".to_string(),
    };
    
    let register_req = test_request_json(
        "POST",
        "/api/v1/auth/register",
        None,
        &register_request,
    );
    
    let register_response = app.clone().oneshot(register_req).await.unwrap();
    assert_eq!(register_response.status(), StatusCode::OK);
    
    let auth_token: ApiResponse<AuthToken> = parse_json_response(register_response).await;
    assert!(auth_token.success);
    assert!(!auth_token.data.access_token.is_empty());
    assert!(!auth_token.data.refresh_token.is_empty());
    assert_eq!(auth_token.data.expires_in, 900);
    
    // Step 2: Login with the same credentials
    let login_request = LoginRequest {
        username: "integrationtest".to_string(),
        password: "testpassword123".to_string(),
        device_name: Some("Test Device".to_string()),
    };
    
    let login_req = test_request_json(
        "POST",
        "/api/v1/auth/login",
        None,
        &login_request,
    );
    
    let login_response = app.clone().oneshot(login_req).await.unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);
    
    let login_token: ApiResponse<AuthToken> = parse_json_response(login_response).await;
    assert!(login_token.success);
    assert!(!login_token.data.access_token.is_empty());
    assert!(!login_token.data.refresh_token.is_empty());
    
    // Step 3: Verify session was created by using the access token to get current user
    let current_user_req = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header("authorization", format!("Bearer {}", login_token.data.access_token))
        .body(Body::empty())
        .unwrap();
    
    let user_response = app.oneshot(current_user_req).await.unwrap();
    assert_eq!(user_response.status(), StatusCode::OK);
    
    let user_data: ApiResponse<User> = parse_json_response(user_response).await;
    assert!(user_data.success);
    assert_eq!(user_data.data.username, "integrationtest");
    assert_eq!(user_data.data.display_name, "Integration Test User");
}

/// Test authentication failure with invalid credentials
#[tokio::test]
async fn test_auth_invalid_credentials() {
    let state = setup_test_state().await;
    let app = ferrex_server::create_app(state);
    
    // Try to login with non-existent user
    let login_request = LoginRequest {
        username: "nonexistent".to_string(),
        password: "wrongpassword".to_string(),
        device_name: Some("Test Device".to_string()),
    };
    
    let login_req = test_request_json(
        "POST",
        "/api/v1/auth/login",
        None,
        &login_request,
    );
    
    let response = app.oneshot(login_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Test token refresh functionality
#[tokio::test]
async fn test_token_refresh() {
    let state = setup_test_state().await;
    let app = ferrex_server::create_app(state);
    
    // First register a user
    let register_request = RegisterRequest {
        username: "refreshtest".to_string(),
        password: "testpassword123".to_string(),
        display_name: "Refresh Test User".to_string(),
    };
    
    let register_req = test_request_json(
        "POST",
        "/api/v1/auth/register",
        None,
        &register_request,
    );
    
    let register_response = app.clone().oneshot(register_req).await.unwrap();
    let auth_token: ApiResponse<AuthToken> = parse_json_response(register_response).await;
    
    // Use refresh token to get new access token
    let refresh_request = json!({
        "refresh_token": auth_token.data.refresh_token
    });
    
    let refresh_req = test_request_json(
        "POST",
        "/api/v1/auth/refresh",
        None,
        &refresh_request,
    );
    
    let refresh_response = app.oneshot(refresh_req).await.unwrap();
    assert_eq!(refresh_response.status(), StatusCode::OK);
    
    let new_token: ApiResponse<AuthToken> = parse_json_response(refresh_response).await;
    assert!(new_token.success);
    assert!(!new_token.data.access_token.is_empty());
    assert!(!new_token.data.refresh_token.is_empty());
    
    // New tokens should be different from original ones
    assert_ne!(new_token.data.access_token, auth_token.data.access_token);
    assert_ne!(new_token.data.refresh_token, auth_token.data.refresh_token);
}

/// Test registration validation
#[tokio::test]
async fn test_registration_validation() {
    let state = setup_test_state().await;
    let app = ferrex_server::create_app(state);
    
    // Test with invalid username (too short)
    let invalid_request = RegisterRequest {
        username: "ab".to_string(), // Too short
        password: "validpassword123".to_string(),
        display_name: "Test User".to_string(),
    };
    
    let register_req = test_request_json(
        "POST",
        "/api/v1/auth/register",
        None,
        &invalid_request,
    );
    
    let response = app.oneshot(register_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Test duplicate username registration
#[tokio::test]
async fn test_duplicate_username() {
    let state = setup_test_state().await;
    let app = ferrex_server::create_app(state);
    
    let register_request = RegisterRequest {
        username: "duplicatetest".to_string(),
        password: "testpassword123".to_string(),
        display_name: "First User".to_string(),
    };
    
    // Register first user
    let first_req = test_request_json(
        "POST",
        "/api/v1/auth/register",
        None,
        &register_request,
    );
    
    let first_response = app.clone().oneshot(first_req).await.unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    
    // Try to register second user with same username
    let duplicate_request = RegisterRequest {
        username: "duplicatetest".to_string(), // Same username
        password: "differentpassword".to_string(),
        display_name: "Second User".to_string(),
    };
    
    let duplicate_req = test_request_json(
        "POST",
        "/api/v1/auth/register",
        None,
        &duplicate_request,
    );
    
    let duplicate_response = app.oneshot(duplicate_req).await.unwrap();
    assert_eq!(duplicate_response.status(), StatusCode::CONFLICT);
}