#[cfg(test)]
mod auth_tests {
    use ferrex_core::user::*;
    use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    const JWT_SECRET: &str = "test_secret_key_for_testing_only";

    #[test]
    fn test_jwt_token_generation() {
        let user_id = Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let claims = Claims {
            sub: user_id,
            exp: now + 900, // 15 minutes
            iat: now,
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_ref()),
        )
        .expect("Failed to encode JWT");

        assert!(!token.is_empty());
        assert!(token.split('.').count() == 3); // JWT has 3 parts
    }

    #[test]
    fn test_jwt_token_validation() {
        let user_id = Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let claims = Claims {
            sub: user_id,
            exp: now + 900,
            iat: now,
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_ref()),
        )
        .unwrap();

        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(JWT_SECRET.as_ref()),
            &Validation::new(Algorithm::HS256),
        )
        .expect("Failed to decode JWT");

        assert_eq!(decoded.claims.sub, user_id);
        assert_eq!(decoded.claims.exp, claims.exp);
        assert_eq!(decoded.claims.iat, claims.iat);
        assert_eq!(decoded.claims.jti, claims.jti);
    }

    #[test]
    fn test_jwt_token_expiry() {
        let user_id = Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Create an expired token
        let claims = Claims {
            sub: user_id,
            exp: now - 100, // Expired 100 seconds ago
            iat: now - 1000,
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_ref()),
        )
        .unwrap();

        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(JWT_SECRET.as_ref()),
            &Validation::new(Algorithm::HS256),
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("ExpiredSignature"));
    }

    #[test]
    fn test_jwt_invalid_signature() {
        let user_id = Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let claims = Claims {
            sub: user_id,
            exp: now + 900,
            iat: now,
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(JWT_SECRET.as_ref()),
        )
        .unwrap();

        // Try to decode with wrong secret
        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(b"wrong_secret"),
            &Validation::new(Algorithm::HS256),
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("InvalidSignature"));
    }

    #[test]
    fn test_password_hashing() {
        use argon2::{
            password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
            Argon2,
        };

        let password = "super_secure_password123";
        let salt = SaltString::generate(&mut OsRng);

        // Hash password
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .expect("Failed to hash password")
            .to_string();

        // Verify it's a valid Argon2 hash
        assert!(password_hash.starts_with("$argon2"));
        assert!(password_hash.len() > 50);

        // Verify password
        let parsed_hash = PasswordHash::new(&password_hash).expect("Failed to parse hash");
        assert!(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok());

        // Wrong password should fail
        assert!(argon2.verify_password(b"wrong_password", &parsed_hash).is_err());
    }

    #[test]
    fn test_register_request_validation() {
        // Valid request
        let valid_request = RegisterRequest {
            username: "testuser".to_string(),
            password: "password123".to_string(),
            display_name: "Test User".to_string(),
        };
        assert!(valid_request.validate().is_ok());

        // Username too short
        let short_username = RegisterRequest {
            username: "ab".to_string(),
            password: "password123".to_string(),
            display_name: "Test User".to_string(),
        };
        assert!(matches!(
            short_username.validate(),
            Err(ValidationError::InvalidUsername)
        ));

        // Username too long
        let long_username = RegisterRequest {
            username: "a".repeat(31),
            password: "password123".to_string(),
            display_name: "Test User".to_string(),
        };
        assert!(matches!(
            long_username.validate(),
            Err(ValidationError::InvalidUsername)
        ));

        // Invalid username characters
        let invalid_chars = RegisterRequest {
            username: "test@user".to_string(),
            password: "password123".to_string(),
            display_name: "Test User".to_string(),
        };
        assert!(matches!(
            invalid_chars.validate(),
            Err(ValidationError::InvalidUsername)
        ));

        // Password too short
        let short_password = RegisterRequest {
            username: "testuser".to_string(),
            password: "pass".to_string(),
            display_name: "Test User".to_string(),
        };
        assert!(matches!(
            short_password.validate(),
            Err(ValidationError::PasswordTooShort)
        ));

        // Empty display name
        let empty_display = RegisterRequest {
            username: "testuser".to_string(),
            password: "password123".to_string(),
            display_name: "".to_string(),
        };
        assert!(matches!(
            empty_display.validate(),
            Err(ValidationError::InvalidDisplayName)
        ));

        // Display name too long
        let long_display = RegisterRequest {
            username: "testuser".to_string(),
            password: "password123".to_string(),
            display_name: "a".repeat(101),
        };
        assert!(matches!(
            long_display.validate(),
            Err(ValidationError::InvalidDisplayName)
        ));
    }

    #[test]
    fn test_auth_token_structure() {
        let auth_token = AuthToken {
            access_token: "<REDACTED>".to_string(),
            refresh_token: Uuid::new_v4().to_string(),
            expires_in: 900, // 15 minutes
        };

        assert!(!auth_token.access_token.is_empty());
        assert!(!auth_token.refresh_token.is_empty());
        assert_eq!(auth_token.expires_in, 900);
    }

    #[test]
    fn test_user_session_creation() {
        let session = UserSession {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            device_name: Some("Test Device".to_string()),
            ip_address: Some("192.168.1.1".to_string()),
            user_agent: Some("Mozilla/5.0...".to_string()),
            last_active: chrono::Utc::now().timestamp(),
            created_at: chrono::Utc::now().timestamp(),
        };

        assert!(session.device_name.is_some());
        assert!(session.ip_address.is_some());
        assert!(session.user_agent.is_some());
        assert!(session.last_active >= session.created_at);
    }

    #[test]
    fn test_auth_error_messages() {
        let errors = vec![
            (AuthError::InvalidCredentials, "Invalid credentials"),
            (AuthError::UsernameTaken, "Username already taken"),
            (AuthError::TokenExpired, "Token expired"),
            (AuthError::TokenInvalid, "Invalid token"),
            (AuthError::RateLimitExceeded, "Rate limit exceeded"),
            (AuthError::InternalError, "Internal error"),
        ];

        for (error, expected_msg) in errors {
            assert_eq!(error.to_string(), expected_msg);
        }
    }
}