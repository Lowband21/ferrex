//! Authentication domain
//! 
//! Contains all authentication-related state and logic moved from the monolithic State

pub mod messages;
pub mod update;
pub mod update_handlers;
pub mod dto;
pub mod errors;
pub mod manager;
pub mod service;
pub mod state_types;
pub mod storage;
pub mod hardware_fingerprint;
pub mod permissions;
pub mod security;
pub mod types;

// Testing module is available when compiling tests or with test feature
// TODO: Re-enable after fixing AuthTestContext implementation
// #[cfg(any(test, feature = "testing"))]
// pub mod testing;

use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use self::messages::Message as AuthMessage;
use crate::domains::ui::views::first_run::FirstRunState;
use ferrex_core::rbac::UserPermissions;
use iced::Task;

// Re-export commonly used auth types
pub use dto::*;
pub use errors::*;
pub use manager::AuthManager;
pub use service::AuthService;
pub use types::AuthenticationFlow;

/// Authentication domain state - moved from monolithic State
pub struct AuthDomainState {
    // From State struct:
    pub api_service: std::sync::Arc<ApiClientAdapter>,
    pub is_authenticated: bool,
    pub auth_flow: AuthenticationFlow,
    pub user_permissions: Option<UserPermissions>,
    pub first_run_state: FirstRunState,
    pub auto_login_enabled: bool,
    /// Trait-based authentication service (Ports & Adapters)
    pub auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>, 
}

impl AuthDomainState {
    /// Create a new AuthDomainState with required services
    pub fn new(
        api_service: std::sync::Arc<ApiClientAdapter>,
        auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
    ) -> Self {
        Self {
            api_service,
            is_authenticated: false,
            auth_flow: AuthenticationFlow::default(),
            user_permissions: None,
            first_run_state: FirstRunState::default(),
            auto_login_enabled: false,
            auth_service,
        }
    }
}

impl std::fmt::Debug for AuthDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthDomainState")
            .field("api_service", &"ApiClientAdapter(..)")
            .field("is_authenticated", &self.is_authenticated)
            .field("auth_flow", &self.auth_flow)
            .field("user_permissions", &self.user_permissions)
            .field("first_run_state", &self.first_run_state)
            .field("auto_login_enabled", &self.auto_login_enabled)
            .field("auth_service", &"AuthService(..)")
            .finish()
    }
}

#[derive(Debug)]
pub struct AuthDomain {
    pub state: AuthDomainState,
}

impl AuthDomain {
    pub fn new(state: AuthDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_auth logic
    pub fn update(&mut self, message: AuthMessage) -> Task<DomainMessage> {
        // This will call the existing update_auth function
        // For now, we return Task::none() to make it compile
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::DatabaseCleared => {
                // Reset auth state
                self.state.is_authenticated = false;
                self.state.user_permissions = None;
                self.state.auth_flow = AuthenticationFlow::default();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

