// First-run experience tests
//
// Requirements from USER_MANAGEMENT_REQUIREMENTS.md:
// - First user becomes admin automatically
// - Subsequent users are standard users
// - System exits first-run mode after admin creation

use ferrex_player::domains::auth::service::AuthService;

#[tokio::test]
async fn first_user_becomes_admin_automatically() {
    // RED: This test should fail until we implement the logic
    let auth = AuthService::new();

    // Verify initial state - system should be in first run
    assert!(
        auth.is_first_run().await,
        "System should be in first-run state"
    );

    // Act - create the first user
    let user_id = auth
        .create_user("admin".to_string(), "password123".to_string())
        .await
        .expect("First user creation should succeed");

    // Assert - first user should automatically have admin permissions
    let permissions = auth
        .get_user_permissions(user_id)
        .await
        .expect("Should be able to get permissions for first user");

    // Verify admin role was assigned
    assert!(
        !permissions.roles.is_empty(),
        "First user should have roles assigned"
    );
    assert!(
        permissions.roles.iter().any(|role| role.name == "admin"),
        "First user should have admin role, but got roles: {:?}",
        permissions
            .roles
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Verify admin permissions
    assert_eq!(
        permissions.permissions.get("system:admin"),
        Some(&true),
        "First user should have system admin permission"
    );

    // System should no longer be in first-run mode
    assert!(
        !auth.is_first_run().await,
        "System should exit first-run after admin creation"
    );
}

#[tokio::test]
async fn second_user_does_not_get_admin_automatically() {
    let auth = AuthService::new();

    // Create first user (becomes admin)
    let _first_user = auth
        .create_user("admin".to_string(), "password123".to_string())
        .await
        .expect("First user creation should succeed");

    // Act - create second user
    let second_user_id = auth
        .create_user("regular_user".to_string(), "password456".to_string())
        .await
        .expect("Second user creation should succeed");

    // Assert - second user should NOT have admin permissions
    let permissions = auth
        .get_user_permissions(second_user_id)
        .await
        .expect("Should be able to get permissions for second user");

    // Verify no admin role
    assert!(
        !permissions.roles.iter().any(|role| role.name == "admin"),
        "Second user should NOT have admin role, but got roles: {:?}",
        permissions
            .roles
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Verify no admin permissions
    assert_ne!(
        permissions.permissions.get("system:admin"),
        Some(&true),
        "Second user should NOT have system admin permission"
    );

    // But should have basic user permissions
    assert_eq!(
        permissions.permissions.get("media:stream"),
        Some(&true),
        "Regular user should have media streaming permission"
    );
}

#[tokio::test]
async fn cannot_create_user_with_duplicate_username() {
    let auth = AuthService::new();

    // Create first user
    let _first_user = auth
        .create_user("testuser".to_string(), "password1".to_string())
        .await
        .expect("First user creation should succeed");

    // Act & Assert - creating user with same username should fail
    let result = auth
        .create_user("testuser".to_string(), "password2".to_string())
        .await;

    assert!(
        result.is_err(),
        "Should not be able to create duplicate username"
    );

    match result.unwrap_err() {
        ferrex_player::domains::auth::AuthError::UserAlreadyExists(username) => {
            assert_eq!(username, "testuser");
        }
        other => panic!("Expected UserAlreadyExists error, got: {:?}", other),
    }
}
