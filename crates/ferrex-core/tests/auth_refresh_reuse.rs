//! Guards refresh-token reuse semantics (family revocation on reuse detection).

use std::sync::Arc;

use anyhow::Result;
use ferrex_core::domain::users::auth::{
    AuthCrypto,
    domain::services::{
        AuthenticationError, AuthenticationService,
        create_authentication_service,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

const TEST_USERNAME: &str = "testuser";
const TEST_PASSWORD: &str = "CorrectHorseBattery1!";

fn build_service(
    pool: PgPool,
) -> Result<(AuthenticationService, Arc<AuthCrypto>)> {
    let crypto = Arc::new(AuthCrypto::new("test-pepper", "test-token-key")?);
    let service = create_authentication_service(pool, crypto.clone());
    Ok((service, crypto))
}

async fn seed_user(pool: &PgPool, crypto: &AuthCrypto) -> Result<Uuid> {
    let user_id = Uuid::new_v4();
    let password_hash = crypto.hash_password(TEST_PASSWORD)?;

    sqlx::query!(
        r#"
        INSERT INTO users (id, username, display_name)
        VALUES ($1, $2, $3)
        "#,
        user_id,
        TEST_USERNAME,
        "Test User"
    )
    .execute(pool)
    .await?;

    sqlx::query!(
        r#"
        INSERT INTO user_credentials (user_id, password_hash)
        VALUES ($1, $2)
        "#,
        user_id,
        password_hash
    )
    .execute(pool)
    .await?;

    Ok(user_id)
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn refresh_reuse_revokes_family(pool: PgPool) -> Result<()> {
    let (service, crypto) = build_service(pool.clone())?;
    seed_user(&pool, &crypto).await?;

    let initial_bundle = service
        .authenticate_with_password(TEST_USERNAME, TEST_PASSWORD)
        .await?;

    let family_id = initial_bundle.refresh_token.family_id();
    let initial_token = initial_bundle.refresh_token.clone();

    let rotated_bundle =
        service.refresh_session(initial_token.as_str()).await?;

    let reused = service.refresh_session(initial_token.as_str()).await;
    assert!(matches!(reused, Err(AuthenticationError::SessionExpired)));

    let rotated_hash = crypto.hash_token(rotated_bundle.refresh_token.as_str());
    let rotated_record = sqlx::query!(
        r#"
        SELECT revoked, revoked_reason
        FROM auth_refresh_tokens
        WHERE token_hash = $1
        "#,
        rotated_hash
    )
    .fetch_one(&pool)
    .await?;

    assert!(
        rotated_record.revoked,
        "rotated token should be revoked after reuse"
    );
    assert_eq!(
        rotated_record.revoked_reason.as_deref(),
        Some("reuse_detected"),
        "family revocation should annotate reuse"
    );

    let remaining_active: i64 = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)
        FROM auth_refresh_tokens
        WHERE family_id = $1 AND revoked = FALSE
        "#,
        family_id
    )
    .fetch_one(&pool)
    .await?
    .unwrap_or(0);

    assert_eq!(remaining_active, 0, "reuse should revoke entire family");

    Ok(())
}
