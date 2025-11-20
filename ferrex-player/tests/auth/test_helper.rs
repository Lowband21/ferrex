//! Minimal test helper for TDD tests
//! 
//! No heavy mocks - uses real AuthService with test configuration

use ferrex_player::domains::auth::service::AuthService;

/// Minimal test helper - no business logic, just service setup
pub struct TestHelper {
    pub auth_service: AuthService,
}

impl TestHelper {
    pub fn new() -> Self {
        Self {
            auth_service: AuthService::new(),
        }
    }
    
    // TODO: Add test database support when needed
    // pub async fn new_with_test_db() -> Self {
    //     Self {
    //         auth_service: AuthService::new_with_test_db().await,
    //     }
    // }
}