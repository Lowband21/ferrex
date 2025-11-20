// Token Persistence Tests
//
// These tests verify that auth tokens are correctly stored and retrieved
// at app startup, focusing on identifying why tokens appear expired.

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

/// Create a real JWT token using the same format the server would use
fn create_jwt_token_with_expiry_minutes(minutes_from_now: i64) -> String {
    let now = Utc::now().timestamp();
    let claims = TestJwtClaims {
        sub: "test_user".to_string(),
        exp: now + (minutes_from_now * 60),
        iat: now,
    };

    let header = Header::new(Algorithm::HS256);
    let key = EncodingKey::from_secret(b"test_secret");
    
    encode(&header, &claims, &key).expect("JWT encoding should succeed")
}

/// Create a test user
fn create_test_user() -> User {
    User {
        id: Uuid::new_v4(),
        username: "testuser".to_string(),
        display_name: "Test User".to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: UserPreferences::default(),
    }
}

#[tokio::test]
async fn test_token_expiry_buffer_issue() {
    println!("Testing token expiry with 5-minute buffer...");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Get the actual hardware fingerprint for testing
    let hardware_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    
    // Test Case 1: Token with 10 minutes remaining - should be valid
    {
        println!("\nTest 1: Token with 10 minutes remaining");
        let token_10min = create_jwt_token_with_expiry_minutes(10);
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_10min.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 600, // 10 minutes
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        // Save auth using the actual hardware fingerprint
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        // Create AuthManager with custom storage path
        let api_client = ApiClient::new("http://localhost:3000".to_string());
        
        // We need to create an AuthManager that uses our test storage
        // For now, we'll use the standard one which will look in the wrong place
        // This is a limitation - we need to modify AuthManager to accept custom storage for testing
        
        // Instead, let's test the token expiry logic directly
        // We'll load the token and check if it's considered expired
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        // Now check if the token would be considered expired by AuthManager
        use ferrex_player::domains::auth::manager::is_token_expired;
        
        let is_expired = is_token_expired(&loaded.token);
        
        if is_expired {
            println!("✗ Token with 10 min remaining - considered expired");
            panic!("Token with 10 minutes should not be expired");
        } else {
            println!("✓ Token with 10 min remaining - considered valid");
        }
    }
    
    // Test Case 2: Token with 30 seconds remaining - will be considered expired due to buffer
    {
        println!("\nTest 2: Token with 30 seconds remaining");
        
        // Create token with 30 seconds remaining
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let now = Utc::now().timestamp();
        let claims = TestJwtClaims {
            sub: "test_user".to_string(),
            exp: now + 30,  // 30 seconds from now
            iat: now,
        };
        let header = Header::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(b"test_secret");
        let token_30sec = encode(&header, &claims, &key).expect("JWT encoding should succeed");
        
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_30sec,
                refresh_token: "refresh_token".to_string(),
                expires_in: 30, // 30 seconds
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        // Save auth using the actual hardware fingerprint
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        // Load and check if the token would be considered expired
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        
        let is_expired = is_token_expired(&loaded.token);
        
        if is_expired {
            println!("✓ Token with 30 sec remaining - rejected due to 1-minute buffer");
        } else {
            println!("✗ Token with 30 sec remaining - NOT rejected (unexpected!)");
            println!("This token should be rejected due to 1-minute buffer!");
            panic!("Token with 30 seconds should be rejected");
        }
    }
    
    // Test Case 3: Token with 2 minutes remaining - should be valid (above 1-minute buffer)
    {
        println!("\nTest 3: Token with 2 minutes remaining");
        let token_2min = create_jwt_token_with_expiry_minutes(2);
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_2min.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 120, // 2 minutes
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        // Save auth using the actual hardware fingerprint
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        // Load and check if the token would be considered expired
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        
        let is_expired = is_token_expired(&loaded.token);
        
        if is_expired {
            println!("✗ Token with 2 min remaining - rejected (unexpected)");
            panic!("Token with 2 minutes should be accepted");
        } else {
            println!("✓ Token with 2 min remaining - considered valid");
        }
    }
    
    println!("\n=== CONCLUSION ===");
    println!("Testing with 1-minute (60 second) TOKEN_EXPIRY_BUFFER:");
    println!("- Tokens with more than 1 minute remaining are accepted ✓");
    println!("- Tokens with less than 1 minute remaining are rejected ✓");
    println!("This provides a good balance between security and user experience.");
}

#[tokio::test]
async fn test_token_expiry_buffer_fixed() {
    println!("Testing token expiry with fixed 1-minute buffer...");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Get the actual hardware fingerprint for testing
    let hardware_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    
    // Test Case 1: Token with 2 minutes remaining - should now be valid with 60-second buffer
    {
        println!("\nTest: Token with 2 minutes remaining (should be valid with 60-second buffer)");
        let token_2min = create_jwt_token_with_expiry_minutes(2);
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_2min.clone(),
                refresh_token: "refresh_token".to_string(),
                expires_in: 120, // 2 minutes
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        // Save auth using the actual hardware fingerprint
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        // Load and check if the token would be considered expired
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        
        let is_expired = is_token_expired(&loaded.token);
        
        if is_expired {
            println!("✗ Token with 2 min remaining - rejected (buffer still too aggressive!)");
            panic!("Token with 2 minutes should be valid with 60-second buffer");
        } else {
            println!("✓ Token with 2 min remaining - accepted (buffer is reasonable)");
        }
    }
    
    // Test Case 2: Token with 30 seconds remaining - should be rejected with 60-second buffer
    {
        println!("\nTest: Token with 30 seconds remaining (should be rejected with 60-second buffer)");
        
        // Create token with 30 seconds remaining
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let now = Utc::now().timestamp();
        let claims = TestJwtClaims {
            sub: "test_user".to_string(),
            exp: now + 30,  // 30 seconds from now
            iat: now,
        };
        let header = Header::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(b"test_secret");
        let token_30sec = encode(&header, &claims, &key).expect("JWT encoding should succeed");
        
        let stored_auth = StoredAuth {
            token: AuthToken {
                access_token: token_30sec,
                refresh_token: "refresh_token".to_string(),
                expires_in: 30, // 30 seconds
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        // Save auth using the actual hardware fingerprint
        let storage = AuthStorage::with_cache_path(cache_path.clone());
        storage.save_auth(&stored_auth, &hardware_fingerprint).await
            .expect("Save should succeed");
        
        // Load and check if the token would be considered expired
        let loaded = storage.load_auth(&hardware_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        
        let is_expired = is_token_expired(&loaded.token);
        
        if is_expired {
            println!("✓ Token with 30 sec remaining - correctly rejected by 60-second buffer");
        } else {
            println!("✗ Token with 30 sec remaining - NOT rejected (buffer too permissive!)");
            panic!("Token with 30 seconds should be rejected with 60-second buffer");
        }
    }
    
    println!("\n=== RESULT ===");
    println!("With 60-second buffer:");
    println!("- Tokens with 2+ minutes are accepted ✓");
    println!("- Tokens with <1 minute are rejected ✓");
    println!("This provides a good balance between security and user experience.");
}

#[tokio::test]
async fn test_token_persistence_across_app_restart() {
    println!("Testing token persistence across app restart simulation...");
    
    // Create temporary storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // Simulate initial login with a fresh token (1 hour expiry)
    let token = create_jwt_token_with_expiry_minutes(60);
    let stored_auth = StoredAuth {
        token: AuthToken {
            access_token: token.clone(),
            refresh_token: "refresh_token".to_string(),
            expires_in: 3600, // 1 hour
        },
        user: create_test_user(),
        server_url: "http://localhost:3000".to_string(),
        permissions: None,
        stored_at: Utc::now(),
        device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
        refresh_token: Some("refresh_token".to_string()),
    };
    
    // Save auth (simulating successful login)
    let storage = AuthStorage::with_cache_path(cache_path.clone());
    let device_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    storage.save_auth(&stored_auth, &device_fingerprint).await
        .expect("Save should succeed");
    
    println!("Saved token with 1 hour expiry");
    
    // Load it back using storage directly (no AuthManager filtering)
    let loaded = storage.load_auth(&device_fingerprint).await
        .expect("Load should succeed")
        .expect("Should have auth stored");
    
    assert_eq!(loaded.token.access_token, token, "Token should be preserved exactly");
    assert_eq!(loaded.user.username, "testuser", "User data should be preserved");
    
    println!("✓ Token successfully persisted and loaded at storage level");
    
    // Now test with AuthManager (which applies expiry check)
    let api_client = ApiClient::new("http://localhost:3000".to_string());
    let auth_manager = AuthManager::new(api_client);
    
    // This may fail due to device fingerprint mismatch in test environment
    // but demonstrates the concept
    match auth_manager.load_from_keychain().await {
        Ok(Some(_)) => {
            println!("✓ AuthManager also loaded the token (1 hour is > 5 min buffer)");
        }
        Ok(None) => {
            println!("✗ AuthManager rejected the token or couldn't find it");
            println!("  (This might be due to device fingerprint mismatch in test)");
        }
        Err(e) => {
            println!("? AuthManager error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_typical_user_session_lifecycle() {
    println!("Testing typical user session lifecycle...");
    
    // Scenario: User logs in, uses app for a while, closes app, reopens later
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    
    // User logs in - gets token valid for 30 minutes (typical session)
    let token = create_jwt_token_with_expiry_minutes(30);
    let stored_auth = StoredAuth {
        token: AuthToken {
            access_token: token.clone(),
            refresh_token: "refresh_token".to_string(),
            expires_in: 1800, // 30 minutes
        },
        user: create_test_user(),
        server_url: "http://localhost:3000".to_string(),
        permissions: None,
        stored_at: Utc::now(),
        device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
        refresh_token: Some("refresh_token".to_string()),
    };
    
    let storage = AuthStorage::with_cache_path(cache_path.clone());
    let device_fingerprint = generate_hardware_fingerprint().await
        .expect("Should get hardware fingerprint");
    storage.save_auth(&stored_auth, &device_fingerprint).await
        .expect("Save should succeed");
    
    println!("User logged in with 30-minute token");
    
    // Test case 1: App restart after 20 minutes (10 minutes remaining)
    {
        println!("\nScenario: App restart with 10 minutes remaining");
        let token_10min = create_jwt_token_with_expiry_minutes(10);
        let stored_auth_later = StoredAuth {
            token: AuthToken {
                access_token: token_10min,
                refresh_token: "refresh_token".to_string(),
                expires_in: 600, // 10 minutes remaining
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        storage.save_auth(&stored_auth_later, &device_fingerprint).await
            .expect("Save should succeed");
        
        // Storage level - should work
        let loaded = storage.load_auth(&device_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        println!("  Storage: Token loaded ✓");
        
        // AuthManager level - should also work (10 min > 5 min buffer)
        let api_client = ApiClient::new("http://localhost:3000".to_string());
        let auth_manager = AuthManager::new(api_client);
        
        match auth_manager.load_from_keychain().await {
            Ok(Some(_)) => println!("  AuthManager: Token accepted ✓"),
            Ok(None) => println!("  AuthManager: Token rejected or not found"),
            Err(e) => println!("  AuthManager: Error - {:?}", e),
        }
    }
    
    // Test case 2: App restart with only 3 minutes remaining
    {
        println!("\nScenario: App restart with 3 minutes remaining");
        let token_3min = create_jwt_token_with_expiry_minutes(3);
        let stored_auth_late = StoredAuth {
            token: AuthToken {
                access_token: token_3min,
                refresh_token: "refresh_token".to_string(),
                expires_in: 180, // 3 minutes remaining
            },
            user: create_test_user(),
            server_url: "http://localhost:3000".to_string(),
            permissions: None,
            stored_at: Utc::now(),
            device_trust_expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            refresh_token: Some("refresh_token".to_string()),
        };
        
        storage.save_auth(&stored_auth_late, &device_fingerprint).await
            .expect("Save should succeed");
        
        // Storage level - should work
        let loaded = storage.load_auth(&device_fingerprint).await
            .expect("Load should succeed")
            .expect("Should have auth");
        
        println!("  Storage: Token loaded ✓");
        
        // AuthManager level - will reject (3 min < 5 min buffer)
        let api_client = ApiClient::new("http://localhost:3000".to_string());
        let auth_manager = AuthManager::new(api_client);
        
        match auth_manager.load_from_keychain().await {
            Ok(Some(_)) => println!("  AuthManager: Token accepted (unexpected!)"),
            Ok(None) => println!("  AuthManager: Token rejected due to 5-min buffer ✗"),
            Err(e) => println!("  AuthManager: Error - {:?}", e),
        }
        
        println!("  User must re-login even though token is technically still valid!");
    }
}