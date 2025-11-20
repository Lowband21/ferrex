use chrono::Utc;
use ferrex_core::user::AuthToken;
use ferrex_player::domains::auth::manager::is_token_expired;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TestClaims {
    sub: String,
    exp: i64,
    iat: i64,
}

fn jwt_with_expiry(seconds_from_now: i64) -> String {
    let now = Utc::now().timestamp();
    let claims = TestClaims {
        sub: "user".into(),
        exp: now + seconds_from_now,
        iat: now,
    };
    let header = Header::new(Algorithm::HS256);
    encode(&header, &claims, &EncodingKey::from_secret(b"secret"))
        .expect("encode jwt")
}

#[test]
fn opaque_token_uses_expires_in_field() {
    let token = AuthToken {
        access_token: "<REDACTED>".into(),
        refresh_token: String::new(),
        expires_in: 120,
        session_id: None,
        device_session_id: None,
        user_id: None,
    };
    assert!(!is_token_expired(&token));

    let short_lived = AuthToken {
        access_token: "<REDACTED>".into(),
        refresh_token: String::new(),
        expires_in: 30,
        session_id: None,
        device_session_id: None,
        user_id: None,
    };
    assert!(is_token_expired(&short_lived));
}

#[test]
fn jwt_token_with_comfortable_margin_is_valid() {
    let token = AuthToken {
        access_token: jwt_with_expiry(300),
        refresh_token: String::new(),
        expires_in: 300,
        session_id: None,
        device_session_id: None,
        user_id: None,
    };
    assert!(!is_token_expired(&token));
}

#[test]
fn jwt_token_inside_refresh_buffer_is_treated_as_expired() {
    let token = AuthToken {
        access_token: jwt_with_expiry(30),
        refresh_token: String::new(),
        expires_in: 30,
        session_id: None,
        device_session_id: None,
        user_id: None,
    };
    assert!(is_token_expired(&token));
}
