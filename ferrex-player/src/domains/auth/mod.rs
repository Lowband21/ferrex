//! Authentication domain
//!
//! Contains auth-related UI state and testing helpers. The runtime app uses the
//! `infra::services::auth::AuthService` trait (implemented by `AuthManager`) to
//! talk to the server, which is the authority for authentication.

pub mod dto;
pub mod errors;
pub mod hardware_fingerprint;
pub mod manager;
pub mod messages;
pub mod permissions;
pub mod pin_policy;
pub mod security;
pub mod state_types;
pub mod storage;
pub mod testkit;
pub mod types;
pub mod update;
pub mod update_handlers;

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infra::services::api::ApiService;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;

// Re-export commonly used auth types
pub use dto::*;
pub use errors::*;
pub use manager::{AuthManager, DeviceTrustPolicyResponse, PinPolicyResponse};
pub use testkit::MockAuthService;
pub use types::AuthenticationFlow;

pub struct AuthDomainState {
    pub api_service: std::sync::Arc<dyn ApiService>,
    pub is_authenticated: bool,
    pub auth_flow: AuthenticationFlow,
    pub user_permissions: Option<UserPermissions>,
    pub auto_login_enabled: bool,
    /// Tracks whether the active remember-device checkbox was changed by the user.
    pub remember_device_explicit_override: bool,
    pub pin_policy: PinPolicyResponse,
    pub device_trust_policy: DeviceTrustPolicyResponse,
    pub auth_service:
        std::sync::Arc<dyn crate::infra::services::auth::AuthService>,
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
        auth_service: std::sync::Arc<
            dyn crate::infra::services::auth::AuthService,
        >,
    ) -> Self {
        Self {
            api_service,
            is_authenticated: false,
            auth_flow: AuthenticationFlow::default(),
            user_permissions: None,
            auto_login_enabled: false,
            remember_device_explicit_override: false,
            pin_policy: PinPolicyResponse::default(),
            device_trust_policy: DeviceTrustPolicyResponse::default(),
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
            .field(
                "remember_device_explicit_override",
                &self.remember_device_explicit_override,
            )
            .field("pin_policy", &self.pin_policy)
            .field("device_trust_policy", &self.device_trust_policy)
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

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
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
