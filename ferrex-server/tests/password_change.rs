use anyhow::Result;
use axum::Router;
use axum::http::StatusCode;
use axum_test::TestServer;
use ferrex_core::api::routes::utils as route_utils;
use ferrex_core::api::routes::v1;
use serde_json::json;
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

#[path = "support/mod.rs"]
mod support;
use support::build_test_app;

fn bearer(token: &str) -> String {
    format!("Bearer {}", token)
}

fn extract_token_field<'a>(body: &'a serde_json::Value, key: &str) -> &'a str {
    body["data"][key]
        .as_str()
        .unwrap_or_else(|| panic!("{} missing", key))
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn user_password_change_revokes_tokens(pool: PgPool) -> Result<()> {
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir;
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let username = "user_change";
    let initial_password = "Password#123";
    let response = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": "Change Tester",
            "password": initial_password
        }))
        .await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let access_token = extract_token_field(&body, "access_token").to_string();
    let refresh_token = extract_token_field(&body, "refresh_token").to_string();
    let _user_id = body["data"]["user_id"]
        .as_str()
        .expect("user_id present")
        .to_string();
    let new_password = "NewPassword#456";
    let update = server
        .put(v1::users::CHANGE_PASSWORD)
        .add_header("Authorization", bearer(&access_token))
        .json(&json!({
            "current_password": initial_password,
            "new_password": new_password
        }))
        .await;
    update.assert_status(StatusCode::NO_CONTENT);

    let reuse_me = server
        .get(v1::users::CURRENT)
        .add_header("Authorization", bearer(&access_token))
        .await;
    reuse_me.assert_status(StatusCode::UNAUTHORIZED);

    let login = server
        .post(v1::auth::LOGIN)
        .json(&json!({
            "username": username,
            "password": new_password
        }))
        .await;
    login.assert_status_ok();
    let login_body: serde_json::Value = login.json();
    let new_access = extract_token_field(&login_body, "access_token");
    let new_refresh = extract_token_field(&login_body, "refresh_token");
    assert_ne!(new_access, access_token);
    assert_ne!(new_refresh, refresh_token);

    let refresh_fail = server
        .post(v1::auth::REFRESH)
        .json(&json!({ "refresh_token": refresh_token }))
        .await;
    refresh_fail.assert_status(StatusCode::UNAUTHORIZED);

    let refresh_success = server
        .post(v1::auth::REFRESH)
        .json(&json!({ "refresh_token": new_refresh }))
        .await;
    refresh_success.assert_status_ok();

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn admin_password_reset_revokes_user_tokens(pool: PgPool) -> Result<()> {
    let app = build_test_app(pool).await?;
    let (router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir;
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let admin_username = "admin_reset";
    let admin_password = "AdminPass#123";
    let admin_register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": admin_username,
            "display_name": "Admin",
            "password": admin_password
        }))
        .await;
    admin_register.assert_status_ok();
    let admin_body: serde_json::Value = admin_register.json();
    let admin_access =
        extract_token_field(&admin_body, "access_token").to_string();
    let admin_user_id =
        admin_body["data"]["user_id"].as_str().unwrap().to_string();
    let admin_uuid = Uuid::parse_str(&admin_user_id)?;
    let admin_role_id =
        Uuid::parse_str("00000000-0000-0000-0000-000000000001")?;
    state
        .unit_of_work()
        .rbac
        .assign_user_role(admin_uuid, admin_role_id, admin_uuid)
        .await?;

    let member_username = "member_reset";
    let member_password = "MemberPass#123";
    let member_register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": member_username,
            "display_name": "Member",
            "password": member_password
        }))
        .await;
    member_register.assert_status_ok();
    let member_body: serde_json::Value = member_register.json();
    let member_access =
        extract_token_field(&member_body, "access_token").to_string();
    let member_refresh =
        extract_token_field(&member_body, "refresh_token").to_string();
    let member_user_id =
        member_body["data"]["user_id"].as_str().unwrap().to_string();

    let reset_path =
        route_utils::replace_param(v1::users::ITEM, "{id}", &member_user_id);
    let reset_password = "ResetMember#456";
    let reset = server
        .put(&reset_path)
        .add_header("Authorization", bearer(&admin_access))
        .json(&json!({
            "new_password": reset_password
        }))
        .await;
    reset.assert_status_ok();

    let member_old_me = server
        .get(v1::users::CURRENT)
        .add_header("Authorization", bearer(&member_access))
        .await;
    member_old_me.assert_status(StatusCode::UNAUTHORIZED);

    let old_login = server
        .post(v1::auth::LOGIN)
        .json(&json!({
            "username": member_username,
            "password": member_password
        }))
        .await;
    old_login.assert_status(StatusCode::UNAUTHORIZED);

    let new_login = server
        .post(v1::auth::LOGIN)
        .json(&json!({
            "username": member_username,
            "password": reset_password
        }))
        .await;
    new_login.assert_status_ok();
    let new_login_body: serde_json::Value = new_login.json();
    let new_refresh = extract_token_field(&new_login_body, "refresh_token");

    let refresh_old = server
        .post(v1::auth::REFRESH)
        .json(&json!({"refresh_token": member_refresh}))
        .await;
    refresh_old.assert_status(StatusCode::UNAUTHORIZED);

    let refresh_new = server
        .post(v1::auth::REFRESH)
        .json(&json!({"refresh_token": new_refresh}))
        .await;
    refresh_new.assert_status_ok();

    Ok(())
}
