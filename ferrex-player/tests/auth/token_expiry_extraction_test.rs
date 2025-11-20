// Test token expiry extraction from JWT tokens

use chrono::Utc;
use ferrex_core::{
    auth::domain::value_objects::SessionScope,
    user::AuthToken,
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

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

#[tokio::test]
async fn test_token_expiry_extraction() {
    // Test 1: Token expiring in 1 hour
    {
        let token_str = create_test_jwt_with_expiry(3600);
        let token = AuthToken {
            access_token: token_str.clone(),
            refresh_token: String::new(),
            expires_in: 0, // This should be updated based on JWT,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    };
        
        // The AuthManager should extract ~3600 seconds from this token
        println!("Created token expiring in ~3600 seconds");
        
        // When loaded, this token should NOT be considered expired
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&token);
        
        assert!(!expired, "Token with 1 hour remaining should not be expired");
    }
    
    // Test 2: Token expiring in 5 minutes
    {
        let token_str = create_test_jwt_with_expiry(300);
        let token = AuthToken {
            access_token: token_str.clone(),
            refresh_token: String::new(),
            expires_in: 0,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    };
        
        println!("Created token expiring in ~300 seconds");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&token);
        
        assert!(!expired, "Token with 5 minutes remaining should not be expired (buffer is 60 seconds)");
    }
    
    // Test 3: Token expiring in 30 seconds (less than buffer)
    {
        let token_str = create_test_jwt_with_expiry(30);
        let token = AuthToken {
            access_token: token_str.clone(),
            refresh_token: String::new(),
            expires_in: 0,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    };
        
        println!("Created token expiring in ~30 seconds");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&token);
        
        assert!(expired, "Token with 30 seconds remaining should be expired (buffer is 60 seconds)");
    }
    
    // Test 4: Already expired token
    {
        let token_str = create_test_jwt_with_expiry(-60); // Expired 1 minute ago
        let token = AuthToken {
            access_token: token_str.clone(),
            refresh_token: String::new(),
            expires_in: 0,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    };
        
        println!("Created already expired token");
        
        use ferrex_player::domains::auth::manager::is_token_expired;
        let expired = is_token_expired(&token);
        
        assert!(expired, "Already expired token should be expired");
    }
    
    println!("\n✓ All token expiry extraction tests passed");
}
