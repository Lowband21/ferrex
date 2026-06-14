//! UI-agnostic user-administration reducers.
//!
//! App shells run the service calls requested by [`UserManagementEffect`] and
//! route completion messages back through this reducer.

use ferrex_core::player_prelude::{User, UserPermissions};
use uuid::Uuid;

use crate::{UserManagementDomainState, messages::UserManagementMessage};

/// Side effects requested by the user-administration reducer.
#[derive(Clone, Debug)]
pub enum UserManagementEffect {
    /// Fetch the admin user list.
    LoadUsers,
    /// Delete a user after the app shell confirms the action.
    DeleteUser(Uuid),
    /// Broadcast that library/admin-visible data changed.
    LibraryUpdated,
    /// Broadcast that first-run authentication completed.
    AuthenticationComplete,
    /// Broadcast an authenticated user selection.
    UserAuthenticated(User, UserPermissions),
}

/// Reducer result containing effects for the app shell.
#[derive(Clone, Debug, Default)]
pub struct UserManagementUpdate {
    pub effects: Vec<UserManagementEffect>,
}

impl UserManagementUpdate {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn effect(effect: UserManagementEffect) -> Self {
        Self {
            effects: vec![effect],
        }
    }
}

/// Handle user-management domain messages.
pub fn update_user_management(
    state: &mut UserManagementDomainState,
    message: UserManagementMessage,
) -> UserManagementUpdate {
    match message {
        UserManagementMessage::LoadUsers => {
            UserManagementUpdate::effect(UserManagementEffect::LoadUsers)
        }
        UserManagementMessage::UsersLoaded(result) => {
            if let Ok(users) = result {
                state.set_users(users);
            }
            UserManagementUpdate::none()
        }
        UserManagementMessage::SelectUser(_) => UserManagementUpdate::none(),
        UserManagementMessage::UserSelected(user) => {
            let permissions = UserPermissions {
                user_id: user.id,
                roles: Vec::new(),
                permissions: std::collections::HashMap::new(),
                permission_details: None,
            };
            UserManagementUpdate::effect(
                UserManagementEffect::UserAuthenticated(user, permissions),
            )
        }
        UserManagementMessage::CreateUser
        | UserManagementMessage::CreateUserFormUpdateUsername(_)
        | UserManagementMessage::CreateUserFormUpdateDisplayName(_)
        | UserManagementMessage::CreateUserFormUpdatePassword(_)
        | UserManagementMessage::CreateUserFormUpdateConfirmPassword(_)
        | UserManagementMessage::CreateUserFormTogglePasswordVisibility
        | UserManagementMessage::CreateUserFormSubmit
        | UserManagementMessage::CreateUserError(_)
        | UserManagementMessage::CreateUserCancel
        | UserManagementMessage::UpdateUser(_)
        | UserManagementMessage::UpdateUserFormUpdateUsername(_)
        | UserManagementMessage::UpdateUserFormUpdateDisplayName(_)
        | UserManagementMessage::UpdateUserFormUpdatePassword(_)
        | UserManagementMessage::UpdateUserFormUpdateConfirmPassword(_)
        | UserManagementMessage::UpdateUserFormTogglePasswordVisibility
        | UserManagementMessage::UpdateUserFormSubmit
        | UserManagementMessage::UpdateUserError(_)
        | UserManagementMessage::UpdateUserCancel
        | UserManagementMessage::DeleteUser(_)
        | UserManagementMessage::DeleteUserError(_)
        | UserManagementMessage::DeleteUserCancel
        | UserManagementMessage::FirstRunCreateUser
        | UserManagementMessage::FirstRunUpdateUsername(_)
        | UserManagementMessage::FirstRunUpdateDisplayName(_)
        | UserManagementMessage::FirstRunUpdatePassword(_)
        | UserManagementMessage::FirstRunUpdateConfirmPassword(_)
        | UserManagementMessage::FirstRunTogglePasswordVisibility
        | UserManagementMessage::FirstRunSubmit
        | UserManagementMessage::FirstRunError(_)
        | UserManagementMessage::ShowUserList
        | UserManagementMessage::BackToUserList => UserManagementUpdate::none(),
        UserManagementMessage::CreateUserSuccess(_)
        | UserManagementMessage::UpdateUserSuccess(_) => {
            UserManagementUpdate::effect(UserManagementEffect::LibraryUpdated)
        }
        UserManagementMessage::DeleteUserConfirm(user_id) => {
            UserManagementUpdate::effect(UserManagementEffect::DeleteUser(
                user_id,
            ))
        }
        UserManagementMessage::DeleteUserSuccess(user_id) => {
            state.remove_user(user_id);
            UserManagementUpdate::effect(UserManagementEffect::LibraryUpdated)
        }
        UserManagementMessage::FirstRunSuccess(_) => {
            UserManagementUpdate::effect(
                UserManagementEffect::AuthenticationComplete,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_player_api::api_types::AdminUserInfo;

    #[test]
    fn users_loaded_replaces_cached_list() {
        let mut state = UserManagementDomainState::default();
        let user = AdminUserInfo {
            id: Uuid::now_v7(),
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            roles: Vec::new(),
            created_at: 0,
            session_count: 0,
        };

        update_user_management(
            &mut state,
            UserManagementMessage::UsersLoaded(Ok(vec![user])),
        );

        assert_eq!(state.users.len(), 1);
    }

    #[test]
    fn delete_success_removes_cached_user() {
        let user_id = Uuid::now_v7();
        let mut state = UserManagementDomainState::default();
        state.users.push(AdminUserInfo {
            id: user_id,
            username: "alice".to_string(),
            display_name: "Alice".to_string(),
            roles: Vec::new(),
            created_at: 0,
            session_count: 0,
        });

        let update = update_user_management(
            &mut state,
            UserManagementMessage::DeleteUserSuccess(user_id),
        );

        assert!(state.users.is_empty());
        assert!(matches!(
            update.effects.as_slice(),
            [UserManagementEffect::LibraryUpdated]
        ));
    }
}
