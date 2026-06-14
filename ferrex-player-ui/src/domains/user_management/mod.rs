//! User management domain integration for the desktop player.
//!
//! User-admin state and messages live in `ferrex-player-user-admin`; this
//! module keeps desktop app task and cross-domain routing glue.

pub mod messages;
pub mod update;

pub use ferrex_player_user_admin::{
    UserManagementDomainState, UserManagementMessage,
};

use crate::common::messages::{CrossDomainEvent, DomainMessage};
use iced::Task;

pub struct UserManagementDomain {
    pub state: UserManagementDomainState,
}

impl UserManagementDomain {
    pub fn new(state: UserManagementDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to app-level update_user_management logic.
    pub fn update(
        &mut self,
        _message: UserManagementMessage,
    ) -> Task<DomainMessage> {
        Task::none()
    }

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, permissions) => {
                self.state.set_user_permissions(permissions.clone());
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

impl std::fmt::Debug for UserManagementDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserManagementDomain")
            .field("state", &self.state)
            .finish()
    }
}
