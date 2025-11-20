use chrono::Utc;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::{
    auth::domain::value_objects::SessionScope,
    user::{AuthToken, User, UserPreferences},
};
use ferrex_player::domains::auth::storage::{AuthStorage, StoredAuth};
use tempfile::TempDir;
use uuid::Uuid;

fn sample_user() -> User {
    User {
        id: Uuid::now_v7(),
        username: "persisted".into(),
        display_name: "Persisted User".into(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: UserPreferences::default(),
    }
}

fn sample_permissions(user_id: Uuid) -> UserPermissions {
    UserPermissions {
        user_id,
        roles: Vec::new(),
        permissions: std::collections::HashMap::new(),
        permission_details: None,
    }
}

fn sample_auth(expires_in: u32) -> StoredAuth {
    let user = sample_user();
    let permissions = sample_permissions(user.id);
    StoredAuth {
        token: AuthToken {
            access_token: "<REDACTED>".into(),
            refresh_token: "refresh-token".into(),
            expires_in,
        session_id: None,
        device_session_id: None,
        user_id: None,
        scope: SessionScope::Full,
    },
        user,
        server_url: "https://localhost:3000".into(),
        permissions: Some(permissions),
        stored_at: Utc::now(),
        device_trust_expires_at: None,
        refresh_token: Some("refresh-token".into()),
    }
}

#[tokio::test]
async fn persisted_auth_roundtrips_with_matching_fingerprint() {
    let temp_dir = TempDir::new().expect("temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    let storage = AuthStorage::with_cache_path(cache_path);
    let fingerprint = "fp-test";

    let auth = sample_auth(3600);
    let before_save = Utc::now();
    storage
        .save_auth(&auth, fingerprint)
        .await
        .expect("save succeeds");

    let loaded = storage
        .load_auth(fingerprint)
        .await
        .expect("load ok")
        .expect("auth content");
    let after_load = Utc::now();

    assert_eq!(loaded.user.id, auth.user.id);
    assert_eq!(loaded.server_url, auth.server_url);
    assert_eq!(loaded.token.access_token, auth.token.access_token);
    assert!(
        loaded.stored_at >= before_save && loaded.stored_at <= after_load,
        "stored_at should be refreshed within test window"
    );
}

#[tokio::test]
async fn loading_with_wrong_fingerprint_fails() {
    let temp_dir = TempDir::new().expect("temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    let storage = AuthStorage::with_cache_path(cache_path);

    storage
        .save_auth(&sample_auth(3600), "fingerprint-a")
        .await
        .expect("save succeeds");

    let result = storage.load_auth("fingerprint-b").await;
    assert!(
        result.is_err(),
        "decrypting with mismatched fingerprint should fail"
    );
}

#[tokio::test]
async fn persisted_auth_respects_refresh_token_flag() {
    let temp_dir = TempDir::new().expect("temp dir");
    let cache_path = temp_dir.path().join("auth_cache.enc");
    let storage = AuthStorage::with_cache_path(cache_path);
    let fingerprint = "fp-refresh";

    let auth = sample_auth(7200);
    storage
        .save_auth(&auth, fingerprint)
        .await
        .expect("save succeeds");

    let loaded = storage
        .load_auth(fingerprint)
        .await
        .expect("load ok")
        .expect("auth content");

    assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-token"));
    assert_eq!(loaded.permissions.unwrap().user_id, auth.user.id);
}
