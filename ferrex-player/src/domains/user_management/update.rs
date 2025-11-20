use crate::infra::api_types::AdminUserInfo;
use ferrex_core::player_prelude::UserPermissions;
use iced::Task;
use log::{debug, error, info};

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::user_management::messages::UserManagementMessage,
    state::State,
};

/// Handle user management domain messages
pub fn update_user_management(
    state: &mut State,
    message: UserManagementMessage,
) -> DomainUpdateResult {
    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::USER_MGMT_UPDATE);

    debug!("User management update: {}", message.name());

    match message {
        // User CRUD operations
        UserManagementMessage::LoadUsers => {
            info!("Loading users from server");
            // Prefer trait-based service
            let Some(service) = state
                .domains
                .user_management
                .state
                .user_admin_service
                .clone()
            else {
                error!("No UserAdminService available");
                return DomainUpdateResult::task(Task::none());
            };
            DomainUpdateResult::task(Task::perform(
                async move { service.list_users().await.map_err(|e| e.to_string()) },
                |result: Result<Vec<AdminUserInfo>, String>| {
                    DomainMessage::from(UserManagementMessage::UsersLoaded(result))
                },
            ))
        }

        UserManagementMessage::UsersLoaded(result) => match result {
            Ok(users) => {
                info!("Successfully loaded {} users (admin view)", users.len());
                state.domains.user_management.state.users = users;
                DomainUpdateResult::task(Task::none())
            }
            Err(error) => {
                error!("Failed to load users: {}", error);
                // Keep previous list; optionally surface an error in UI later
                DomainUpdateResult::task(Task::none())
            }
        },

        // User selection
        UserManagementMessage::SelectUser(user_id) => {
            info!("Selecting user: {}", user_id);
            // TODO: Implement user selection logic
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UserSelected(user) => {
            info!("User selected: {} ({})", user.display_name, user.username);
            // TODO: Update state with selected user
            // Emit cross-domain event
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::UserAuthenticated(
                    user.clone(),
                    UserPermissions {
                        user_id: user.id,
                        roles: Vec::new(),
                        permissions: std::collections::HashMap::new(),
                        permission_details: None,
                    }, // TODO: Load actual permissions
                )],
            )
        }

        // User creation
        UserManagementMessage::CreateUser => {
            info!("Starting user creation flow");
            // TODO: Initialize user creation form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormUpdateUsername(username) => {
            debug!("Updating create user form username: {}", username);
            // TODO: Update form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormUpdateDisplayName(display_name) => {
            debug!("Updating create user form display name: {}", display_name);
            // TODO: Update form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormUpdatePassword(_password) => {
            debug!("Updating create user form password");
            // TODO: Update form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormUpdateConfirmPassword(_confirm_password) => {
            debug!("Updating create user form confirm password");
            // TODO: Update form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormTogglePasswordVisibility => {
            debug!("Toggling create user form password visibility");
            // TODO: Toggle password visibility state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserFormSubmit => {
            info!("Submitting create user form");
            // TODO: Validate form and submit to API
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserSuccess(user) => {
            info!(
                "User created successfully: {} ({})",
                user.display_name, user.username
            );
            // TODO: Update state and emit event
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::LibraryUpdated],
            )
            // TODO: Create proper UserCreated event
        }

        UserManagementMessage::CreateUserError(error) => {
            error!("Failed to create user: {}", error);
            // TODO: Handle error state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::CreateUserCancel => {
            info!("User creation cancelled");
            // TODO: Reset form state
            DomainUpdateResult::task(Task::none())
        }

        // User updates
        UserManagementMessage::UpdateUser(user_id) => {
            info!("Starting user update flow for: {}", user_id);
            // TODO: Load user data and initialize update form
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormUpdateUsername(username) => {
            debug!("Updating user update form username: {}", username);
            // TODO: Update form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormUpdateDisplayName(display_name) => {
            debug!("Updating user update form display name: {}", display_name);
            // TODO: Update form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormUpdatePassword(_password) => {
            debug!("Updating user update form password");
            // TODO: Update form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormUpdateConfirmPassword(_confirm_password) => {
            debug!("Updating user update form confirm password");
            // TODO: Update form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormTogglePasswordVisibility => {
            debug!("Toggling user update form password visibility");
            // TODO: Toggle password visibility state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserFormSubmit => {
            info!("Submitting user update form");
            // TODO: Validate form and submit to API
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserSuccess(user) => {
            info!(
                "User updated successfully: {} ({})",
                user.display_name, user.username
            );
            // TODO: Update state and emit event
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::LibraryUpdated],
            )
            // TODO: Create proper UserUpdated event
        }

        UserManagementMessage::UpdateUserError(error) => {
            error!("Failed to update user: {}", error);
            // TODO: Handle error state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::UpdateUserCancel => {
            info!("User update cancelled");
            // TODO: Reset form state
            DomainUpdateResult::task(Task::none())
        }

        // User deletion
        UserManagementMessage::DeleteUser(user_id) => {
            info!("Requesting user deletion confirmation for: {}", user_id);
            // TODO: Show confirmation dialog
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::DeleteUserConfirm(user_id) => {
            info!("Deleting user: {}", user_id);
            let Some(service) = state
                .domains
                .user_management
                .state
                .user_admin_service
                .clone()
            else {
                error!("No UserAdminService available for deletion");
                return DomainUpdateResult::task(Task::none());
            };
            DomainUpdateResult::task(Task::perform(
                async move {
                    service
                        .delete_user(user_id)
                        .await
                        .map(|_| user_id)
                        .map_err(|e| e.to_string())
                },
                |result| match result {
                    Ok(id) => {
                        DomainMessage::from(UserManagementMessage::DeleteUserSuccess(id))
                    }
                    Err(err) => {
                        DomainMessage::from(UserManagementMessage::DeleteUserError(err))
                    }
                },
            ))
        }

        UserManagementMessage::DeleteUserSuccess(user_id) => {
            info!("User deleted successfully: {}", user_id);
            // Remove from cached admin users list for immediate UI feedback
            state
                .domains
                .user_management
                .state
                .users
                .retain(|u| u.id != user_id);
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::LibraryUpdated],
            )
            // TODO: Create proper UserDeleted event
        }

        UserManagementMessage::DeleteUserError(error) => {
            error!("Failed to delete user: {}", error);
            // TODO: Handle error state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::DeleteUserCancel => {
            info!("User deletion cancelled");
            // TODO: Close confirmation dialog
            DomainUpdateResult::task(Task::none())
        }

        // First-run user creation (moved from auth)
        UserManagementMessage::FirstRunCreateUser => {
            info!("Starting first-run user creation");
            // TODO: Initialize first-run user creation state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunUpdateUsername(username) => {
            debug!("Updating first-run username: {}", username);
            // TODO: Update first-run form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunUpdateDisplayName(display_name) => {
            debug!("Updating first-run display name: {}", display_name);
            // TODO: Update first-run form state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunUpdatePassword(_password) => {
            debug!("Updating first-run password");
            // TODO: Update first-run form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunUpdateConfirmPassword(_confirm_password) => {
            debug!("Updating first-run confirm password");
            // TODO: Update first-run form state (password will be SecureCredential)
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunTogglePasswordVisibility => {
            debug!("Toggling first-run password visibility");
            // TODO: Toggle password visibility state
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunSubmit => {
            info!("Submitting first-run user creation");
            // TODO: Validate and submit first-run user creation
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::FirstRunSuccess(user) => {
            info!(
                "First-run user creation successful: {} ({})",
                user.display_name, user.username
            );
            // TODO: Complete first-run setup
            DomainUpdateResult::with_events(
                Task::none(),
                vec![CrossDomainEvent::AuthenticationComplete],
            )
        }

        UserManagementMessage::FirstRunError(error) => {
            error!("First-run user creation failed: {}", error);
            // TODO: Handle error state
            DomainUpdateResult::task(Task::none())
        }

        // Navigation
        UserManagementMessage::ShowUserList => {
            info!("Showing user list");
            // TODO: Navigate to user list view
            DomainUpdateResult::task(Task::none())
        }

        UserManagementMessage::BackToUserList => {
            info!("Navigating back to user list");
            // TODO: Navigate back to user list
            DomainUpdateResult::task(Task::none())
        }
    }
}
