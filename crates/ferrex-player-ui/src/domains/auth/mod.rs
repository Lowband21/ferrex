//! Authentication domain integration for the desktop player.
//!
//! Core authentication state, messages, storage, manager, service adapters, PIN
//! policy, security helpers, and testkit live in `ferrex-player-auth`. This
//! module keeps the desktop app's domain wrapper and update routing glue while
//! preserving the historical `ferrex_player::domains::auth::*` imports used by
//! UI code and tests.

pub mod permissions;
pub mod update;
pub mod update_handlers;

pub use ferrex_player_auth::{
    AuthDomainState, AuthManager, AuthManagerAdapter, AuthService,
    MockAuthService, adapter, dto, errors, hardware_fingerprint, manager,
    messages, pin_policy, security, service, state_types, storage, testkit,
    types,
};
pub use ferrex_player_auth::{
    AuthError, AuthResult, AuthenticationFlow, AutoLoginScope,
    DeviceAuthStatus, DeviceError, DeviceTrustPolicyResponse, NetworkError,
    PinPolicyResponse, PlayerAuthResult, StorageError, TokenError,
};
pub use ferrex_player_auth::{DeviceIdentity, UserListItemDto};

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use iced::Task;

#[derive(Debug)]
pub struct AuthDomain {
    pub state: AuthDomainState,
}

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
                self.state.reset_runtime_auth_state();
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
