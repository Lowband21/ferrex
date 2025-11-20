//! Authentication domain
//!
//! Contains all authentication-related state and logic moved from the monolithic State

pub mod dto;
pub mod errors;
pub mod hardware_fingerprint;
pub mod manager;
pub mod messages;
pub mod permissions;
pub mod security;
pub mod service;
pub mod state_types;
pub mod storage;
pub mod types;
pub mod update;
pub mod update_handlers;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use crate::infrastructure::services::api::ApiService;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;

// Re-export commonly used auth types
pub use dto::*;
pub use errors::*;
pub use manager::AuthManager;
pub use service::AuthService;
pub use types::AuthenticationFlow;

pub struct AuthDomainState {
    pub api_service: std::sync::Arc<dyn ApiService>,
    pub is_authenticated: bool,
    pub auth_flow: AuthenticationFlow,
    pub user_permissions: Option<UserPermissions>,
    pub auto_login_enabled: bool,
    pub auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl AuthDomainState {
    pub fn new(
        api_service: std::sync::Arc<dyn ApiService>,
        auth_service: std::sync::Arc<dyn crate::infrastructure::services::auth::AuthService>,
    ) -> Self {
        Self {
            api_service,
            is_authenticated: false,
            auth_flow: AuthenticationFlow::default(),
            user_permissions: None,
            auto_login_enabled: false,
            auth_service,
        }
    }
}

impl std::fmt::Debug for AuthDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthDomainState")
            .field("api_service", &"ApiService(..)")
            .field("is_authenticated", &self.is_authenticated)
            .field("auth_flow", &self.auth_flow)
            .field("user_permissions", &self.user_permissions)
            .field("auto_login_enabled", &self.auto_login_enabled)
            .field("auth_service", &"AuthService(..)")
            .finish()
    }
}

#[derive(Debug)]
pub struct AuthDomain {
    pub state: AuthDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl AuthDomain {
    pub fn new(state: AuthDomainState) -> Self {
        Self { state }
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
