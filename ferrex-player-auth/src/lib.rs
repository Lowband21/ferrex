//! Authentication domain surfaces for Ferrex player clients.
//!
//! This crate owns the dependency-light authentication state, messages,
//! service contract, concrete `AuthManager` adapter, encrypted local auth
//! storage, hardware fingerprinting, PIN policy helpers, security helpers, and
//! auth-specific testkit. UI crates are expected to provide view rendering and
//! app-level message routing around these primitives.

pub mod adapter;
pub mod dto;
pub mod errors;
pub mod hardware_fingerprint;
pub mod manager;
pub mod messages;
pub mod pin_policy;
pub mod security;
pub mod service;
pub mod state_types;
pub mod storage;
pub mod testkit;
pub mod types;
pub mod update_handlers;

use ferrex_core::player_prelude::UserPermissions;
use ferrex_player_api::services::api::ApiService;
use std::sync::Arc;

pub use adapter::AuthManagerAdapter;
pub use dto::*;
pub use errors::*;
pub use ferrex_player_api::auth::{
    AutoLoginScope, DeviceAuthStatus, DeviceTrustPolicyResponse,
    PinPolicyResponse, PlayerAuthResult,
};
pub use manager::{AuthManager, DeviceIdentity};
pub use service::AuthService;
pub use testkit::MockAuthService;
pub use types::AuthenticationFlow;

/// Runtime authentication state shared by player frontends.
pub struct AuthDomainState {
    pub api_service: Arc<dyn ApiService>,
    pub is_authenticated: bool,
    pub auth_flow: AuthenticationFlow,
    pub user_permissions: Option<UserPermissions>,
    pub auto_login_enabled: bool,
    /// Tracks whether the active remember-device checkbox was changed by the user.
    pub remember_device_explicit_override: bool,
    pub pin_policy: PinPolicyResponse,
    pub device_trust_policy: DeviceTrustPolicyResponse,
    pub auth_service: Arc<dyn AuthService>,
}

impl AuthDomainState {
    /// Build authentication domain state from API and auth service adapters.
    pub fn new(
        api_service: Arc<dyn ApiService>,
        auth_service: Arc<dyn AuthService>,
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

    /// Reset in-memory authentication state after destructive local data cleanup.
    pub fn reset_runtime_auth_state(&mut self) {
        self.is_authenticated = false;
        self.user_permissions = None;
        self.auth_flow = AuthenticationFlow::default();
        self.auto_login_enabled = false;
        self.remember_device_explicit_override = false;
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
