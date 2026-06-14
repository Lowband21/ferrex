//! Desktop user-management update adapter.
//!
//! `ferrex-player-user-admin` owns the UI-agnostic reducers. This module maps
//! reducer effects to desktop tasks and cross-domain events.

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage, DomainUpdateResult},
    domains::user_management::messages::UserManagementMessage,
    state::State,
};
use ferrex_player_api::api_types::AdminUserInfo;
use ferrex_player_user_admin::{UserManagementEffect, UserManagementUpdate};
use iced::Task;

/// Handle user management domain messages.
pub fn update_user_management(
    state: &mut State,
    message: UserManagementMessage,
) -> DomainUpdateResult {
    let update = ferrex_player_user_admin::update_user_management(
        &mut state.domains.user_management.state,
        message,
    );
    apply_user_management_update(state, update)
}

fn apply_user_management_update(
    state: &mut State,
    update: UserManagementUpdate,
) -> DomainUpdateResult {
    let mut tasks = Vec::new();
    let mut events = Vec::new();

    for effect in update.effects {
        match effect {
            UserManagementEffect::LoadUsers => {
                tasks.push(load_users_task(state));
            }
            UserManagementEffect::DeleteUser(user_id) => {
                tasks.push(delete_user_task(state, user_id));
            }
            UserManagementEffect::LibraryUpdated => {
                events.push(CrossDomainEvent::LibraryUpdated);
            }
            UserManagementEffect::AuthenticationComplete => {
                events.push(CrossDomainEvent::AuthenticationComplete);
            }
            UserManagementEffect::UserAuthenticated(user, permissions) => {
                events.push(CrossDomainEvent::UserAuthenticated(
                    user,
                    permissions,
                ));
            }
        }
    }

    let task = if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    };

    DomainUpdateResult::with_events(task, events)
}

fn load_users_task(state: &State) -> Task<DomainMessage> {
    let Some(service) = state
        .domains
        .user_management
        .state
        .user_admin_service
        .clone()
    else {
        return Task::done(DomainMessage::UserManagement(
            UserManagementMessage::UsersLoaded(Err(
                "No user administration service available".to_string(),
            )),
        ));
    };

    Task::perform(
        async move {
            service
                .list_users()
                .await
                .map_err(|error| error.to_string())
        },
        |result: Result<Vec<AdminUserInfo>, String>| {
            DomainMessage::UserManagement(UserManagementMessage::UsersLoaded(
                result,
            ))
        },
    )
}

fn delete_user_task(state: &State, user_id: uuid::Uuid) -> Task<DomainMessage> {
    let Some(service) = state
        .domains
        .user_management
        .state
        .user_admin_service
        .clone()
    else {
        return Task::done(DomainMessage::UserManagement(
            UserManagementMessage::DeleteUserError(
                "No user administration service available".to_string(),
            ),
        ));
    };

    Task::perform(
        async move {
            service
                .delete_user(user_id)
                .await
                .map(|()| user_id)
                .map_err(|error| error.to_string())
        },
        |result| match result {
            Ok(user_id) => DomainMessage::UserManagement(
                UserManagementMessage::DeleteUserSuccess(user_id),
            ),
            Err(error) => DomainMessage::UserManagement(
                UserManagementMessage::DeleteUserError(error),
            ),
        },
    )
}
