use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use iced::{Preset, Task};
use uuid::Uuid;

use crate::app::bootstrap::{self, AppConfig};
use crate::common::messages::DomainMessage;
use crate::domains::auth::dto::UserListItemDto;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::types::{
    AuthenticationFlow, SetupClaimStatus, SetupClaimUi,
};
use crate::domains::settings::state::{PreferencesState, SettingsView};
use crate::domains::ui::types::ViewState;
use crate::domains::ui::views::settings::device_management::{
    DeviceManagementState, UserDevice,
};
use crate::state::State;

pub fn collect(config: &Arc<AppConfig>) -> Vec<Preset<State, DomainMessage>> {
    let mut presets = Vec::new();

    presets.push(first_run_preset(Arc::clone(config)));
    presets.push(user_selection_preset(Arc::clone(config)));
    presets.push(admin_session_preset(Arc::clone(config)));
    presets.push(auth_devices_preset(Arc::clone(config)));
    presets.push(library_loaded_preset(Arc::clone(config)));

    presets
}

fn first_run_preset(config: Arc<AppConfig>) -> Preset<State, DomainMessage> {
    Preset::new("FirstRun", move || {
        let mut state = bootstrap::base_state(&config);
        bootstrap::reset_to_first_run(&mut state);

        state.domains.auth.state.auth_flow =
            AuthenticationFlow::FirstRunSetup {
                username: String::new(),
                password: SecureCredential::from(""),
                confirm_password: SecureCredential::from(""),
                display_name: String::new(),
                setup_token: String::new(),
                claim_token: String::new(),
                show_password: false,
                error: None,
                loading: false,
                claim: SetupClaimUi {
                    device_name: "Ferrex Test Device".into(),
                    claim_id: Some(Uuid::nil()),
                    claim_code: Some("000000".into()),
                    expires_at: Some(Utc::now() + chrono::Duration::minutes(5)),
                    claim_token: None,
                    lan_only: true,
                    last_error: None,
                    status: SetupClaimStatus::Idle,
                    is_requesting: false,
                    is_confirming: false,
                },
                setup_token_required: true,
            };

        (state, Task::none())
    })
}

fn user_selection_preset(
    config: Arc<AppConfig>,
) -> Preset<State, DomainMessage> {
    Preset::new("UserSelection", move || {
        let mut state = bootstrap::base_state(&config);
        bootstrap::reset_to_first_run(&mut state);

        state.domains.auth.state.auth_flow =
            AuthenticationFlow::SelectingUser {
                users: sample_users(false),
                error: None,
            };

        (state, Task::none())
    })
}

fn admin_session_preset(
    config: Arc<AppConfig>,
) -> Preset<State, DomainMessage> {
    Preset::new("AdminSession", move || {
        let mut state = bootstrap::base_state(&config);

        state.is_authenticated = true;
        state.domains.auth.state.is_authenticated = true;
        state.domains.auth.state.user_permissions =
            Some(sample_admin_permissions());
        state.domains.auth.state.auth_flow =
            AuthenticationFlow::SelectingUser {
                users: sample_users(true),
                error: None,
            };

        (state, Task::none())
    })
}

fn auth_devices_preset(config: Arc<AppConfig>) -> Preset<State, DomainMessage> {
    Preset::new("AuthenticatedWithDevices", move || {
        let mut state = bootstrap::base_state(&config);

        state.is_authenticated = true;
        state.domains.auth.state.is_authenticated = true;
        state.domains.auth.state.user_permissions =
            Some(sample_admin_permissions());
        state.domains.auth.state.auth_flow =
            AuthenticationFlow::Authenticated {
                user: sample_user("demo_admin"),
                mode: crate::domains::auth::types::AuthenticationMode::Online,
            };

        state.domains.settings.current_view = SettingsView::DeviceManagement;
        state.domains.settings.device_management_state =
            DeviceManagementState {
                devices: vec![
                    UserDevice {
                        device_id: "current-device".into(),
                        device_name: "Ferrex Player".into(),
                        device_type: "Desktop".into(),
                        last_active: Utc::now(),
                        is_current_device: true,
                        location: Some("Test Lab".into()),
                    },
                    UserDevice {
                        device_id: "tablet".into(),
                        device_name: "Living Room Tablet".into(),
                        device_type: "Tablet".into(),
                        last_active: Utc::now() - chrono::Duration::hours(5),
                        is_current_device: false,
                        location: Some("Living Room".into()),
                    },
                ],
                loading: false,
                error_message: None,
            };

        state.domains.settings.preferences = PreferencesState {
            auto_login_enabled: true,
            theme: Default::default(),
            loading: false,
            error: None,
        };

        state.domains.ui.state.view = ViewState::UserSettings;

        (state, Task::none())
    })
}

fn library_loaded_preset(
    config: Arc<AppConfig>,
) -> Preset<State, DomainMessage> {
    Preset::new("LibraryLoaded", move || {
        let mut state = bootstrap::base_state(&config);

        state.is_authenticated = true;
        state.domains.auth.state.is_authenticated = true;
        state.domains.auth.state.user_permissions =
            Some(sample_admin_permissions());
        state.domains.ui.state.view = ViewState::Library;

        state.domains.ui.state.current_library_id =
            Some(ferrex_core::player_prelude::LibraryID::new());
        state.domains.ui.state.display_mode =
            crate::domains::ui::types::DisplayMode::Library;

        (state, Task::none())
    })
}

fn sample_users(include_admin_session: bool) -> Vec<UserListItemDto> {
    let mut users = vec![UserListItemDto {
        id: Uuid::now_v7(),
        username: "demo_admin".into(),
        display_name: "Demo Admin".into(),
        avatar_url: None,
        has_pin: include_admin_session,
        last_login: Some(Utc::now() - chrono::Duration::hours(1)),
    }];

    users.push(UserListItemDto {
        id: Uuid::now_v7(),
        username: "guest".into(),
        display_name: "Guest".into(),
        avatar_url: None,
        has_pin: include_admin_session,
        last_login: None,
    });

    users
}

fn sample_admin_permissions() -> ferrex_core::player_prelude::UserPermissions {
    ferrex_core::player_prelude::UserPermissions {
        user_id: Uuid::now_v7(),
        roles: vec![ferrex_core::player_prelude::Role {
            id: Uuid::now_v7(),
            name: "admin".into(),
            description: Some("Administrator".into()),
            is_system: true,
            created_at: Utc::now().timestamp(),
        }],
        permissions: HashMap::from([
            ("user:create".into(), true),
            ("system:admin".into(), true),
        ]),
        permission_details: None,
    }
}

fn sample_user(username: &str) -> ferrex_core::player_prelude::User {
    ferrex_core::player_prelude::User {
        id: Uuid::now_v7(),
        username: username.into(),
        display_name: username.to_string(),
        avatar_url: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_login: Some(Utc::now()),
        is_active: true,
        email: None,
        preferences: ferrex_core::player_prelude::UserPreferences::default(),
    }
}
