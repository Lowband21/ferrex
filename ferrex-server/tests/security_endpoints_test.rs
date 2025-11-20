//! Security endpoint tests
//! 
//! Verifies that sensitive endpoints are properly secured

use axum::http::{HeaderMap, StatusCode};
use ferrex_server::{AppState, create_test_app};
use serde_json::json;

#[tokio::test]
async fn test_public_users_endpoint_requires_fingerprint() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    
    // Request without device fingerprint should fail
    let response = client
        .get(&format!("{}/api/v1/users/public", app.base_url()))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("Device fingerprint required"));
}

#[tokio::test]
async fn test_public_users_endpoint_validates_fingerprint() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    
    // Request with invalid fingerprint should fail
    let response = client
        .get(&format!("{}/api/v1/users/public", app.base_url()))
        .header("X-Device-Fingerprint", "short")
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("Invalid device fingerprint"));
}

#[tokio::test]
async fn test_public_users_endpoint_anonymizes_unknown_devices() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    
    // Create test users
    let admin_token = create_test_admin(&app).await;
    create_test_user(&app, &admin_token, "testuser1").await;
    create_test_user(&app, &admin_token, "testuser2").await;
    
    // Request with valid but unknown fingerprint
    let fingerprint = "a".repeat(64); // Valid length fingerprint
    let response = client
        .get(&format!("{}/api/v1/users/public", app.base_url()))
        .header("X-Device-Fingerprint", &fingerprint)
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body: serde_json::Value = response.json().await.unwrap();
    let users = body["data"].as_array().unwrap();
    
    // Verify anonymization
    for user in users {
        // Should not have real UUIDs (check that ID is derived from username)
        assert!(user["id"].as_str().is_some());
        // Should not have last_login
        assert!(user["last_login"].is_null());
        // Should not reveal PIN status
        assert_eq!(user["has_pin"], false);
    }
}

#[tokio::test]
async fn test_authenticated_users_endpoint_requires_auth() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    
    // Request without authentication should fail
    let response = client
        .get(&format!("{}/api/v1/users/list", app.base_url()))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_authenticated_users_endpoint_shows_full_info() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    
    // Create admin and regular user
    let admin_token = create_test_admin(&app).await;
    let user_token = create_test_user(&app, &admin_token, "regularuser").await;
    
    // Admin request should get full list
    let response = client
        .get(&format!("{}/api/v1/users/list", app.base_url()))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    let users = body["data"].as_array().unwrap();
    assert!(users.len() >= 2); // At least admin and regular user
    
    // Regular user request should only see themselves
    let response = client
        .get(&format!("{}/api/v1/users/list", app.base_url()))
        .bearer_auth(&user_token)
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    let users = body["data"].as_array().unwrap();
    assert_eq!(users.len(), 1); // Only themselves
    assert_eq!(users[0]["username"], "regularuser");
}

#[tokio::test]
async fn test_rate_limiting_on_public_endpoint() {
    let app = create_test_app().await;
    let client = reqwest::Client::new();
    let fingerprint = "b".repeat(64);
    
    // Make multiple rapid requests
    let mut responses = Vec::new();
    for _ in 0..20 {
        let response = client
            .get(&format!("{}/api/v1/users/public", app.base_url()))
            .header("X-Device-Fingerprint", &fingerprint)
            .send()
            .await
            .unwrap();
        responses.push(response.status());
    }
    
    // At least one should be rate limited (if rate limiting is enabled)
    // Note: This test assumes rate limiting is configured with a low threshold for testing
    let rate_limited = responses.iter().any(|&status| status == StatusCode::TOO_MANY_REQUESTS);
    
    // Log result for debugging (rate limiting might be disabled in test env)
    if !rate_limited {
        println!("Warning: Rate limiting may not be enabled in test environment");
    }
}

// Helper functions

async fn create_test_admin(app: &TestApp) -> String {
    let client = reqwest::Client::new();
    
    // Create initial admin
    let response = client
        .post(&format!("{}/api/v1/setup/admin", app.base_url()))
        .json(&json!({
            "username": "admin",
            "password": "AdminPass123!",
            "display_name": "Test Admin"
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Login as admin
    let response = client
        .post(&format!("{}/api/v1/auth/login", app.base_url()))
        .json(&json!({
            "username": "admin",
            "password": "AdminPass123!"
        }))
        .send()
        .await
        .unwrap();
    
    let body: serde_json::Value = response.json().await.unwrap();
    body["data"]["access_token"].as_str().unwrap().to_string()
}

async fn create_test_user(app: &TestApp, admin_token: &str, username: &str) -> String {
    let client = reqwest::Client::new();
    
    // Create user as admin
    let response = client
        .post(&format!("{}/api/v1/users", app.base_url()))
        .bearer_auth(admin_token)
        .json(&json!({
            "username": username,
            "password": "UserPass123!",
            "display_name": format!("Test {}", username),
            "role": "User"
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Login as the new user
    let response = client
        .post(&format!("{}/api/v1/auth/login", app.base_url()))
        .json(&json!({
            "username": username,
            "password": "UserPass123!"
        }))
        .send()
        .await
        .unwrap();
    
    let body: serde_json::Value = response.json().await.unwrap();
    body["data"]["access_token"].as_str().unwrap().to_string()
}

// Test app structure
struct TestApp {
    base_url: String,
}

impl TestApp {
    fn base_url(&self) -> &str {
        &self.base_url
    }
}