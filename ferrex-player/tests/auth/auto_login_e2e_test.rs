// End-to-end test for auto-login functionality

use chrono::Utc;
use ferrex_core::user::{AuthToken, User, UserPreferences};
use ferrex_player::domains::auth::{
    hardware_fingerprint::generate_hardware_fingerprint,
    manager::AuthManager,
    storage::{AuthStorage, StoredAuth},
};
use ferrex_player::infrastructure::api_client::ApiClient;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct TestJwtClaims {
    sub: String,
    exp: i64,
    iat: i64,
}

/// Create a test JWT token with specific expiry
fn create_test_jwt_with_expiry(seconds_from_now: i64) -> String {
    let now = Utc::now().timestamp();
    let claims = TestJwtClaims {
        sub: "test_user".to_string(),
        exp: now + seconds_from_now,
        iat: now,
    };
    
    let header = Header::new(Algorithm::HS256);
    let key = EncodingKey::from_secret(b"test_secret");
    
    encode(&header, &claims, &key).expect("JWT encoding should succeed")
}

/// Create a test user
fn create_test_user() -> User {
    User {
        id: Uuid::now_v7(),
        username: "testuser".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: UserPreferences {
            auto_login_enabled: true, // Important: auto-login is enabled
            ..UserPreferences::default()
        },
    }
}

#[tokio::test]
async fn test_auto_login_with_valid_token() {
    println!("=== Test: Auto-login with valid token ===");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Get the actual hardware fingerprint for testing
    let hardware_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    
    // Simulate saving a token with 2 hours expiry (plenty of time)
    {
        println!("\n1. Saving auth with 2-hour token expiry...");
        let token_2hr = create_test_jwt_with_expiry(7200); // 2 hours
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_2hr.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 7200, // 2 hours
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        println!("  ✓ Token saved with 2-hour expiry");
    }
    
    // Simulate app restart and auto-login attempt
    {
        println!("\n2. Simulating app restart and auto-login...");
        
        // Load the token back
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        println!("  ✓ Token loaded successfully");
        println!("  - Username: {}", loaded.user.username);
        println!("  - Token expires_in field: {} seconds", loaded.token.expires_in);
        
        // Check if token would be considered expired
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&loaded.token);
        
        if expired {
            println!("  ✗ Token considered expired (unexpected!)");
            panic!("Token with 2 hours remaining should not be expired");
        } else {
            println!("  ✓ Token considered valid - auto-login would succeed");
        }
        
        // Check auto-login preference
        assert!(loaded.user.preferences.auto_login_enabled, 
                "Auto-login should be enabled in user preferences");
        println!("  ✓ Auto-login enabled in user preferences");
    }
    
    println!("\n=== Result: Auto-login should work with valid tokens ===");
}

#[tokio::test]
async fn test_auto_login_with_nearly_expired_token() {
    println!("=== Test: Auto-login with nearly expired token ===");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Get the actual hardware fingerprint for testing
    let hardware_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    
    // Simulate saving a token with only 90 seconds expiry (just above 60-second buffer)
    {
        println!("\n1. Saving auth with 90-second token expiry...");
        let token_90sec = create_test_jwt_with_expiry(90); // 90 seconds
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_90sec.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 90,
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        println!("  ✓ Token saved with 90-second expiry");
    }
    
    // Simulate app restart and auto-login attempt
    {
        println!("\n2. Simulating app restart and auto-login...");
        
        // Load the token back
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        println!("  ✓ Token loaded successfully");
        
        // Check if token would be considered expired
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&loaded.token);
        
        if expired {
            println!("  ✗ Token considered expired (unexpected with 90 seconds remaining)");
            panic!("Token with 90 seconds remaining should not be expired (buffer is 60 seconds)");
        } else {
            println!("  ✓ Token still valid with 90 seconds remaining");
        }
    }
    
    println!("\n=== Result: Tokens with >60 seconds work for auto-login ===");
}

#[tokio::test]
async fn test_auto_login_with_expired_token() {
    println!("=== Test: Auto-login with expired token ===");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Get the actual hardware fingerprint for testing
    let hardware_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    
    // Simulate saving a token with only 30 seconds expiry (below 60-second buffer)
    {
        println!("\n1. Saving auth with 30-second token expiry...");
        let token_30sec = create_test_jwt_with_expiry(30); // 30 seconds
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_30sec.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 30,
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        println!("  ✓ Token saved with 30-second expiry");
    }
    
    // Simulate app restart and auto-login attempt
    {
        println!("\n2. Simulating app restart and auto-login...");
        
        // Load the token back
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        println!("  ✓ Token loaded from storage");
        
        // Check if token would be considered expired
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&loaded.token);
        
        if expired {
            println!("  ✓ Token correctly rejected (30 seconds < 60-second buffer)");
            println!("  → User would need to re-authenticate");
        } else {
            println!("  ✗ Token accepted (unexpected!)");
            panic!("Token with only 30 seconds remaining should be expired");
        }
    }
    
    println!("\n=== Result: Expired tokens are correctly rejected ===");
}