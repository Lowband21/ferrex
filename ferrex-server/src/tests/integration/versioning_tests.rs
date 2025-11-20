use crate::tests::test_utils::TestContext;
use axum::http::{header, StatusCode};

#[tokio::test]
async fn test_v1_api_routes() {
    let ctx = TestContext::new().await;
    
    // Test that v1 routes are accessible
    let response = ctx.client
        .get(&format!("{}/api/v1/setup/status", ctx.server_url))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Check that API version header is present
    let version_header = response.headers().get("X-API-Version");
    assert!(version_header.is_some());
    assert_eq!(version_header.unwrap().to_str().unwrap(), "v1");
}

#[tokio::test]
async fn test_version_negotiation_via_accept_header() {
    let ctx = TestContext::new().await;
    
    // Test with vendor-specific accept header
    let response = ctx.client
        .get(&format!("{}/api/v1/setup/status", ctx.server_url))
        .header(header::ACCEPT, "application/vnd.ferrex.v1+json")
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Check that API version header matches requested version
    let version_header = response.headers().get("X-API-Version");
    assert!(version_header.is_some());
    assert_eq!(version_header.unwrap().to_str().unwrap(), "v1");
}

#[tokio::test]
async fn test_backward_compatibility() {
    let ctx = TestContext::new().await;
    
    // Test that old routes still work (through compatibility layer)
    let response = ctx.client
        .get(&format!("{}/api/setup/status", ctx.server_url))
        .send()
        .await
        .unwrap();
    
    // Old routes should still work
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_authenticated_v1_routes() {
    let ctx = TestContext::new().await;
    let (user, token) = ctx.create_test_user("test_user", "password123").await;
    
    // Test authenticated route through v1 API
    let response = ctx.client
        .get(&format!("{}/api/v1/users/me", ctx.server_url))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Verify user data
    let user_data: serde_json::Value = response.json().await.unwrap();
    assert_eq!(user_data["data"]["username"], "test_user");
}

#[tokio::test]
async fn test_v1_media_endpoints() {
    let ctx = TestContext::new().await;
    let (_user, token) = ctx.create_test_user("test_user", "password123").await;
    
    // Test batch media endpoint
    let response = ctx.client
        .post(&format!("{}/api/v1/media/batch", ctx.server_url))
        .bearer_auth(&token.access_token)
        .json(&serde_json::json!({
            "ids": []
        }))
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v1_watch_status_endpoints() {
    let ctx = TestContext::new().await;
    let (_user, token) = ctx.create_test_user("test_user", "password123").await;
    
    // Test watch state endpoint
    let response = ctx.client
        .get(&format!("{}/api/v1/watch/state", ctx.server_url))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
}