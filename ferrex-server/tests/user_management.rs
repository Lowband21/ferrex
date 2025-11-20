use std::net::SocketAddr;

use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use ferrex_core::api_routes::{self, utils as route_utils};
use ferrex_server::infra::app_state::AppState;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[path = "support/mod.rs"]
mod support;

use support::build_test_app;

fn bearer(token: &str) -> String {
    format!("Bearer {}", token)
}

fn extract_token_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("missing token field: {key}"))
}

async fn register_user(
    server: &TestServer,
    username: &str,
    password: &str,
) -> Result<(String, String, String)> {
    let response = server
        .post(api_routes::v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": format!("{} display", username),
            "password": password
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();

    let user_id = body["data"]["user_id"]
        .as_str()
        .expect("user_id provided")
        .to_string();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let refresh_token = extract_token_field(&body, "refresh_token").to_string();

    Ok((user_id, access_token, refresh_token))
}

async fn promote_to_admin(state: &AppState, user_id: Uuid) -> Result<()> {
    let roles = state.unit_of_work.rbac.get_all_roles().await?;
    let admin_role = roles
        .into_iter()
        .find(|role| role.name == "admin")
        .expect("admin role seeded");

    state
        .unit_of_work
        .rbac
        .assign_user_role(user_id, admin_role.id, user_id)
        .await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn user_management_requires_permissions(pool: PgPool) -> Result<()> {
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir;

    let router: Router<()> = router.with_state(state.clone());
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let (user_id, access_token, _) =
        register_user(&server, "standard_user", "Password#123").await?;

    // List users should be forbidden without the admin users:read permission.
    let list = server
        .get(api_routes::v1::users::COLLECTION)
        .add_header("Authorization", bearer(&access_token))
        .await;
    list.assert_status(StatusCode::FORBIDDEN);

    // Creating a new user should also be forbidden.
    let create = server
        .post(api_routes::v1::users::COLLECTION)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "username": "another_user",
            "display_name": "Another",
            "password": "Another#123"
        }))
        .await;
    create.assert_status(StatusCode::FORBIDDEN);

    // Updating an existing user without permissions should fail.
    let update_path = route_utils::replace_param(api_routes::v1::users::ITEM, "{id}", &user_id);
    let update = server
        .put(&update_path)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "display_name": "Updated",
        }))
        .await;
    update.assert_status(StatusCode::FORBIDDEN);

    // Deleting a user without permissions should fail before self-deletion guard.
    let delete = server
        .delete(&update_path)
        .add_header("Authorization", bearer(&access_token))
        .await;
    delete.assert_status(StatusCode::FORBIDDEN);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn admin_user_crud_flow_enforces_audit_expectations(pool: PgPool) -> Result<()> {
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir;

    let router: Router<()> = router.with_state(state.clone());
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    // Seed an admin user and promote them to the admin role so that they have the required permissions.
    let (admin_id_raw, admin_access, _admin_refresh) =
        register_user(&server, "admin_user", "Admin#Pass123").await?;
    let admin_id = Uuid::parse_str(&admin_id_raw)?;
    promote_to_admin(&state, admin_id).await?;

    // Track baseline admin_actions entries; user CRUD should eventually populate this log.
    let admin_actions_before: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM admin_actions")
        .fetch_one(state.postgres.pool())
        .await?
        .unwrap_or(0);

    // Create a managed user through the admin API.
    let create_response = server
        .post(api_routes::v1::users::COLLECTION)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "username": "managed_user",
            "display_name": "Managed User",
            "password": "Managed#Pass123",
            "email": "managed@example.com"
        }))
        .await;
    create_response.assert_status_ok();
    let created_body: Value = create_response.json();
    let managed_id_raw = created_body["data"]["id"]
        .as_str()
        .expect("id present")
        .to_string();
    let managed_id = Uuid::parse_str(&managed_id_raw)?;

    // Verify the user exists in the repository with the expected fields.
    let stored_user = state
        .unit_of_work
        .users
        .get_user_by_id(managed_id)
        .await?
        .expect("user persisted");
    assert_eq!(stored_user.username, "managed_user");
    assert_eq!(stored_user.display_name, "Managed User");
    assert!(stored_user.is_active);

    // Admin listing should include the newly created user.
    let list_response = server
        .get(api_routes::v1::users::COLLECTION)
        .add_header("Authorization", bearer(&admin_access))
        .await;
    list_response.assert_status_ok();
    let list_body: Value = list_response.json();
    let users = list_body["data"]["users"].as_array().expect("users array");
    let managed_entry = users
        .iter()
        .find(|entry| entry["username"].as_str() == Some("managed_user"))
        .expect("managed_user present in listing");
    assert_eq!(managed_entry["session_count"].as_u64(), Some(0));

    // Update the managed user profile.
    let update_path =
        route_utils::replace_param(api_routes::v1::users::ITEM, "{id}", &managed_id_raw);
    let update_response = server
        .put(&update_path)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "display_name": "Updated Managed",
            "email": "updated@example.com",
        }))
        .await;
    update_response.assert_status_ok();
    let updated_user = state
        .unit_of_work
        .users
        .get_user_by_id(managed_id)
        .await?
        .expect("user updated");
    assert_eq!(updated_user.display_name, "Updated Managed");
    assert_eq!(updated_user.email.as_deref(), Some("updated@example.com"));

    // Log in as the managed user to create active sessions and refresh tokens.
    let login_response = server
        .post(api_routes::v1::auth::LOGIN)
        .json(&json!({
            "username": "managed_user",
            "password": "Managed#Pass123"
        }))
        .await;
    login_response.assert_status_ok();
    let login_body: Value = login_response.json();
    let managed_access = extract_token_field(&login_body, "access_token").to_string();
    let managed_refresh = extract_token_field(&login_body, "refresh_token").to_string();
    assert!(!managed_access.is_empty());
    assert!(!managed_refresh.is_empty());

    // The listing should now show one active session for the managed user.
    let list_after_login = server
        .get(api_routes::v1::users::COLLECTION)
        .add_header("Authorization", bearer(&admin_access))
        .await;
    list_after_login.assert_status_ok();
    let list_after_login_body: Value = list_after_login.json();
    let users_after_login = list_after_login_body["data"]["users"].as_array().unwrap();
    let managed_entry_after_login = users_after_login
        .iter()
        .find(|entry| entry["username"].as_str() == Some("managed_user"))
        .expect("managed_user present after login");
    assert_eq!(managed_entry_after_login["session_count"].as_u64(), Some(1));

    // Confirm the database reflects the active session and refresh token before deletion.
    let sessions_before: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM auth_sessions WHERE user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(
        sessions_before, 1,
        "expected one active session before deletion"
    );

    let refresh_before: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM auth_refresh_tokens WHERE user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(
        refresh_before, 1,
        "expected one refresh token before deletion"
    );

    // Delete the managed user and ensure the response code is 204 NO CONTENT.
    let delete_response = server
        .delete(&update_path)
        .add_header("Authorization", bearer(&admin_access))
        .await;
    delete_response.assert_status(StatusCode::NO_CONTENT);

    // The user should now be absent from the repository and all sessions should be gone.
    let deleted_user = state.unit_of_work.users.get_user_by_id(managed_id).await?;
    assert!(deleted_user.is_none(), "user record should be removed");

    let sessions_after: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM auth_sessions WHERE user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(
        sessions_after, 0,
        "all sessions must be revoked after deletion"
    );

    let refresh_after: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM auth_refresh_tokens WHERE user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(
        refresh_after, 0,
        "all refresh tokens must be removed after deletion"
    );

    // Admin actions are not yet persisted for user management flows; track the gap explicitly.
    let admin_actions_after: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM admin_actions")
        .fetch_one(state.postgres.pool())
        .await?
        .unwrap_or(0);
    assert_eq!(
        admin_actions_after - admin_actions_before,
        0,
        "admin action audit logging not implemented for user CRUD"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn admin_endpoints_record_audit_logs(pool: PgPool) -> Result<()> {
    use ferrex_core::api_routes::v1;

    let app = build_test_app(pool).await?;
    let (router, state, _tempdir) = app.into_parts();

    let router: Router<()> = router.with_state(state.clone());
    let make_service = router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    // Create admin and promote
    let (admin_id_raw, admin_access, _admin_refresh) =
        register_user(&server, "admin_crud", "Admin#Pass123").await?;
    let admin_id = Uuid::parse_str(&admin_id_raw)?;
    promote_to_admin(&state, admin_id).await?;

    // Create a managed user through the admin API
    let create_response = server
        .post(v1::admin::USERS)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "username": "managed_admin_user",
            "display_name": "Managed Admin User",
            "password": "Managed#Pass123",
            "email": "managed_admin@example.com"
        }))
        .await;
    create_response.assert_status_ok();
    let created_body: Value = create_response.json();
    let managed_id_raw = created_body["data"]["id"]
        .as_str()
        .expect("id present")
        .to_string();
    let managed_id = Uuid::parse_str(&managed_id_raw)?;

    // Verify admin_actions and security_audit_log entries for creation
    let admin_actions_created: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM admin_actions WHERE action_type = 'user.create' AND target_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(admin_actions_created, 1, "one admin action for create");

    let security_created: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM security_audit_log WHERE event_type = 'user_created' AND user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(security_created, 1, "one security audit for create");

    // Update via admin API
    let update_path = route_utils::replace_param(v1::admin::USER_ITEM, "{id}", &managed_id_raw);
    let update_response = server
        .put(&update_path)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "display_name": "Managed Admin User Updated",
            "email": "managed_admin_updated@example.com"
        }))
        .await;
    update_response.assert_status_ok();

    let admin_actions_updated: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM admin_actions WHERE action_type = 'user.update' AND target_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(admin_actions_updated, 1, "one admin action for update");

    let security_updated: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM security_audit_log WHERE event_type = 'user_updated' AND user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(security_updated, 1, "one security audit for update");

    // Delete via admin API
    let delete_response = server
        .delete(&update_path)
        .add_header("Authorization", bearer(&admin_access))
        .await;
    delete_response.assert_status(StatusCode::NO_CONTENT);

    let admin_actions_deleted: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM admin_actions WHERE action_type = 'user.delete' AND target_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(admin_actions_deleted, 1, "one admin action for delete");

    let security_deleted: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM security_audit_log WHERE event_type = 'user_deleted' AND user_id = $1",
        managed_id
    )
    .fetch_one(state.postgres.pool())
    .await?
    .unwrap_or(0);
    assert_eq!(security_deleted, 1, "one security audit for delete");

    Ok(())
}
