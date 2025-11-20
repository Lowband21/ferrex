//! User management domain
//!
//! Contains all user management-related state and logic

pub mod messages;
pub mod update;

use self::messages::Message as UserManagementMessage;
use crate::common::messages::{CrossDomainEvent, DomainMessage};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use ferrex_core::rbac::UserPermissions;
use iced::Task;

/// User management domain state
pub struct UserManagementDomainState {
    // References needed by user management domain
    pub api_service: Option<std::sync::Arc<ApiClientAdapter>>,
    pub user_permissions: Option<UserPermissions>,
    pub user_admin_service: Option<
        std::sync::Arc<dyn crate::infrastructure::services::user_management::UserAdminService>,
    >,
}

impl Default for UserManagementDomainState {
    fn default() -> Self {
        Self {
            api_service: None,
            user_permissions: None,
            user_admin_service: None,
        }
    }
}

impl std::fmt::Debug for UserManagementDomainState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserManagementDomainState")
            .field("has_api_service", &self.api_service.as_ref().map(|_| true))
            .field("user_permissions", &self.user_permissions)
            .field(
                "has_user_admin_service",
                &self.user_admin_service.as_ref().map(|_| true),
            )
            .finish()
    }
}

impl std::fmt::Debug for UserManagementDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserManagementDomain")
            .field("state", &self.state)
            .finish()
    }
}

pub struct UserManagementDomain {
    pub state: UserManagementDomainState,
}

impl UserManagementDomain {
    pub fn new(state: UserManagementDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_user_management logic
    pub fn update(&mut self, message: UserManagementMessage) -> Task<DomainMessage> {
        // This will call the existing update_user_management function
        Task::none()
    }

    pub fn handle_event(&mut self, event: &CrossDomainEvent) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::UserAuthenticated(_user, permissions) => {
                self.state.user_permissions = Some(permissions.clone());
                Task::none()
            }
            _ => Task::none(),
        }
    }
}
