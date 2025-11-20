use std::sync::Arc;

use crate::domains::ui::update_handlers::curated;
use crate::{
    domains::auth::{
        manager::DeviceAuthStatus, messages as auth,
        security::secure_credential::SecureCredential,
    },
    infra::services::auth::AuthService,
    state::State,
};
use ferrex_core::player_prelude as core;
use ferrex_core::player_prelude::{User, UserPermissions};
use iced::Task;

/// Handle loading users for user selection screen
pub fn handle_load_users(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!("[Auth] handle_load_users called - loading users for selection");

    // Set auth flow to LoadingUsers to show the loading screen
    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;

    // Directly load users - we'll check setup status only if no users exist
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
    Task::perform(
        async move {
            log::info!("[Auth] Fetching all users from server via AuthService");
            svc.get_all_users().await.map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(users) => {
                if users.is_empty() {
                    // No users found, check if setup is needed
                    log::info!("[Auth] No users found, checking setup status");
                    auth::AuthMessage::CheckSetupStatus
                } else {
                    log::info!(
                        "[Auth] Successfully loaded {} users",
                        users.len()
                    );
                    auth::AuthMessage::UsersLoaded(Ok(users))
                }
            }
            Err(e) => {
                log::error!("[Auth] Failed to load users: {}", e);
                // On error, check setup status as fallback
                auth::AuthMessage::CheckSetupStatus
            }
        },
    )
}

/// Transition into the auto-login check flow when setup is not required.
pub fn transition_to_auto_login_check(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!("[Auth] No setup needed, checking for auto-login");
    state.domains.auth.state.auth_flow = AuthenticationFlow::CheckingAutoLogin;

    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = Arc::clone(auth_service);

    Task::perform(
        async move {
            log::info!("[Auth] Checking for cached auth and auto-login");

            match svc.load_from_keychain().await {
                Ok(Some(stored_auth)) => {
                    let device_auto_login = svc
                        .is_auto_login_enabled(&stored_auth.user.id)
                        .await
                        .unwrap_or(false);

                    let auto_login_enabled =
                        stored_auth.user.preferences.auto_login_enabled
                            && device_auto_login;

                    log::info!(
                        "[Auth] Auto-login check - Server: {}, Device: {}, Combined: {}",
                        stored_auth.user.preferences.auto_login_enabled,
                        device_auto_login,
                        auto_login_enabled
                    );

                    if auto_login_enabled {
                        match svc.apply_stored_auth(stored_auth.clone()).await {
                            Ok(_) => {
                                log::info!("[Auth] Auto-login successful");
                                return auth::AuthMessage::AutoLoginSuccessful(
                                    stored_auth.user,
                                );
                            }
                            Err(e) => {
                                log::error!("[Auth] Auto-login failed: {}", e);
                            }
                        }
                    } else {
                        log::info!(
                            "[Auth] Auto-login disabled, proceeding to user selection"
                        );
                    }
                }
                Ok(None) => {
                    log::info!("[Auth] No cached auth found");
                }
                Err(e) => {
                    log::error!("[Auth] Failed to load cached auth: {}", e);
                }
            }

            auth::AuthMessage::AutoLoginCheckComplete
        },
        |msg| msg,
    )
}

/// Handle users loaded response
pub fn handle_users_loaded(
    state: &mut State,
    result: Result<Vec<crate::domains::auth::dto::UserListItemDto>, String>,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!(
        "[Auth] handle_users_loaded called with result: {:?}",
        result.as_ref().map(|users| users.len())
    );

    match result {
        Ok(loaded_users) => {
            log::info!(
                "[Auth] Setting auth_flow to SelectingUser with {} users",
                loaded_users.len()
            );
            if loaded_users.is_empty() {
                // No cached users and not in setup flow -> show pre-auth login
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::PreAuthLogin {
                        username: String::new(),
                        password: SecureCredential::new(String::new()),
                        show_password: false,
                        remember_device: state
                            .domains
                            .auth
                            .state
                            .auto_login_enabled,
                        error: None,
                        loading: false,
                    };
            } else {
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::SelectingUser {
                        users: loaded_users,
                        error: None,
                    };
            }
        }
        Err(err) => {
            log::error!("[Auth] Error loading users: {}", err);
            state.domains.auth.state.auth_flow =
                AuthenticationFlow::SelectingUser {
                    users: Vec::new(),
                    error: Some(err),
                };
        }
    }

    log::info!(
        "[Auth] Auth flow after update: {:?}",
        state.domains.auth.state.auth_flow
    );
    Task::none()
}

/// Handle user selection
pub fn handle_select_user(
    state: &mut State,
    user_id: uuid::Uuid,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!("[Auth] handle_select_user called for user_id: {}", user_id);

    // Clone the user before modifying state
    let selected_user_dto = match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::SelectingUser { users, .. } => {
            users.iter().find(|u| u.id == user_id).cloned()
        }
        _ => None,
    };

    if let Some(user_dto) = selected_user_dto {
        log::info!("[Auth] Found user: {}", user_dto.username);

        // We need to create a full User object from UserListItemDto for the auth flow
        // For now, we'll create a minimal User object with the info we have
        let user = core::User {
            id: user_dto.id,
            username: user_dto.username.clone(),
            display_name: user_dto.display_name.clone(),
            avatar_url: user_dto.avatar_url.clone(),
            email: None,     // Not available in UserListItemDto
            is_active: true, // Assume active users are shown in the list
            last_login: user_dto.last_login,
            preferences: core::UserPreferences::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Update state to checking device
        state.domains.auth.state.auth_flow =
            AuthenticationFlow::CheckingDevice { user: user.clone() };

        // Check device authentication status
        let auth_service = &state.domains.auth.state.auth_service;
        let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
        let user_id = user.id;
        let user_clone = user.clone();

        log::info!(
            "[Auth] Checking device auth status for user {} via AuthService",
            user_id
        );

        return Task::perform(
            async move {
                svc.check_device_auth(user_id)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| {
                auth::AuthMessage::DeviceStatusChecked(user_clone, result)
            },
        );
    } else {
        log::error!("[Auth] User {} not found in current auth flow", user_id);
    }

    Task::none()
}

/// Handle device status check result
pub fn handle_device_status_checked(
    state: &mut State,
    user: core::User,
    result: Result<DeviceAuthStatus, String>,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{AuthenticationFlow, CredentialType};

    log::info!(
        "[Auth] handle_device_status_checked called for user {} with result: {:?}",
        user.username,
        result
    );

    let default_remember = state.domains.auth.state.auto_login_enabled;
    let auth_service = Arc::clone(&state.domains.auth.state.auth_service);
    let user_id = user.id;

    match result {
        Ok(status) => {
            if status.device_registered && status.has_pin {
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::EnteringCredentials {
                        user,
                        input_type: CredentialType::Pin { max_length: 4 },
                        input: SecureCredential::new(String::new()),
                        show_password: false,
                        remember_device: default_remember,
                        error: None,
                        attempts_remaining: Some(
                            status.remaining_attempts.unwrap_or(5),
                        ),
                        loading: false,
                    };
            } else {
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::EnteringCredentials {
                        user,
                        input_type: CredentialType::Password,
                        input: SecureCredential::new(String::new()),
                        show_password: false,
                        remember_device: default_remember,
                        error: None,
                        attempts_remaining: None,
                        loading: false,
                    };
            }
        }
        Err(_) => {
            state.domains.auth.state.auth_flow =
                AuthenticationFlow::EnteringCredentials {
                    user,
                    input_type: CredentialType::Password,
                    input: SecureCredential::new(String::new()),
                    show_password: false,
                    remember_device: default_remember,
                    error: None,
                    attempts_remaining: None,
                    loading: false,
                };
        }
    }

    Task::perform(
        async move {
            auth_service
                .is_auto_login_enabled(&user_id)
                .await
                .unwrap_or(false)
        },
        auth::AuthMessage::RememberDeviceSynced,
    )
}

/// Handle enable admin PIN unlock
pub fn handle_enable_admin_pin_unlock(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
    Task::future(async move {
        match svc.enable_admin_pin_unlock().await {
            Ok(_) => auth::AuthMessage::AdminPinUnlockToggled(Ok(true)),
            Err(e) => auth::AuthMessage::AdminPinUnlockToggled(Err(e.to_string())),
        }
    })
}

/// Handle disable admin PIN unlock
pub fn handle_disable_admin_pin_unlock(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
    Task::future(async move {
        match svc.disable_admin_pin_unlock().await {
            Ok(_) => auth::AuthMessage::AdminPinUnlockToggled(Ok(false)),
            Err(e) => auth::AuthMessage::AdminPinUnlockToggled(Err(e.to_string())),
        }
    })
}

/// Handle admin PIN unlock toggle result
pub fn handle_admin_pin_unlock_toggled(
    state: &mut State,
    result: Result<bool, String>,
) -> Task<auth::AuthMessage> {
    match result {
        Ok(enabled) => {
            // Update any UI state if needed
            log::info!(
                "Admin PIN unlock is now {}",
                if enabled { "enabled" } else { "disabled" }
            );
            // Could show a notification or update UI state here
            Task::none()
        }
        Err(error) => {
            log::error!("Failed to toggle admin PIN unlock: {}", error);
            state.domains.ui.state.error_message =
                Some(format!("Failed to toggle admin PIN unlock: {}", error));
            Task::none()
        }
    }
}

// Legacy PIN entry handlers removed - replaced by AuthFlow handlers

/// Handle login success
pub fn handle_login_success(
    state: &mut State,
    user: core::User,
    permissions: core::UserPermissions,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{AuthenticationFlow, AuthenticationMode};

    log::info!("Login successful for user: {}", user.username);

    // Mark as authenticated (both top-level and domain state)
    state.is_authenticated = true;
    state.domains.auth.state.is_authenticated = true;

    // Store permissions
    state.domains.auth.state.user_permissions = Some(permissions.clone());

    // Determine authentication mode based on current flow
    let auth_mode = match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::CheckingAutoLogin => AuthenticationMode::AutoLogin,
        _ => AuthenticationMode::Online,
    };

    // Set authenticated state with proper mode
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: user.clone(),
        mode: auth_mode,
    };

    // API client should already be initialized from main.rs
    // The auth_manager already set the token in the shared api_client instance

    // Metadata service is already initialized during app startup in main.rs

    // BatchMetadataFetcher initialization moved to handle_auth_flow_completed
    // to ensure it's only initialized once after full auth flow completes

    // After successful login, fetch watch status and then trigger library loading
    // We'll chain the watch status loading with a completion message
    let api_service = state.domains.auth.state.api_service.clone();
    //let user_clone = user.clone();
    //let permissions_clone = permissions.clone();

    Task::perform(
        async move { api_service.get_watch_state().await },
        move |result| {
            // First handle the watch status result
            match result {
                Ok(watch_state) => {
                    // Store watch state and signal authentication complete
                    auth::AuthMessage::WatchStatusLoaded(Ok(watch_state))
                }
                Err(e) => {
                    // Even on error, we're still authenticated
                    auth::AuthMessage::WatchStatusLoaded(Err(e.to_string()))
                }
            }
        },
    )
}

/// Handle back to user selection
pub fn handle_back_to_user_selection(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;

    // Reload users
    handle_load_users(state)
}

/// Handle logout
pub fn handle_logout(state: &mut State) -> Task<auth::AuthMessage> {
    // Use trait-based auth service
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
    Task::perform(
        async move {
            let _ = svc.logout().await;
        },
        |_| auth::AuthMessage::LogoutComplete,
    )
}

/// Handle logout complete
pub fn handle_logout_complete(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    // Reset authentication state (both top-level and domain state)
    state.is_authenticated = false;
    state.domains.auth.state.is_authenticated = false;
    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;
    state.domains.auth.state.user_permissions = None;

    // Load users after logout
    handle_load_users(state)
}

// Legacy create user handler removed

// Legacy password login handlers removed - replaced by AuthFlow handlers

/// Handle auth status check (when stored auth is loaded from keychain)
pub fn handle_check_auth_status(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!(
        "[Auth] handle_check_auth_status called - auto-login successful"
    );

    state.domains.auth.state.auth_flow = AuthenticationFlow::CheckingAutoLogin;

    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = Arc::clone(auth_service);

    Task::perform(
        async move { svc.validate_session().await.map_err(|e| e.to_string()) },
        |result| match result {
            Ok((user, permissions)) => {
                auth::AuthMessage::LoginSuccess(user, permissions)
            }
            Err(err) => {
                log::warn!("[Auth] Auto-login validation failed: {}", err);
                auth::AuthMessage::AutoLoginCheckComplete
            }
        },
    )
}

/// Handle auth status confirmed with PIN (user has valid stored auth and PIN)
pub fn handle_auth_status_confirmed_with_pin(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    log::info!(
        "[Auth] User has valid stored auth and PIN, proceeding to load libraries"
    );

    // Mark as authenticated (both top-level and domain state)
    state.is_authenticated = true;
    state.domains.auth.state.is_authenticated = true;

    // Unify completion signaling with the LoginSuccess path so that
    // AuthenticationComplete is always emitted from a single branch.
    // We synchronously fetch the current user and permissions and
    // immediately dispatch LoginSuccess; the LoginSuccess handler
    // will take care of watch-state fetching and further initialization.

    // Obtain current identity synchronously in this non-async context
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);

    let (maybe_user, maybe_perms) = tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current().block_on(async move {
            let user = svc.get_current_user().await.ok().flatten();
            let perms = svc.get_current_permissions().await.ok().flatten();
            (user, perms)
        })
    });

    // Fallbacks in case the service doesn't return identity immediately
    let user = maybe_user.or_else(|| match &state.domains.auth.state.auth_flow {
        crate::domains::auth::types::AuthenticationFlow::EnteringCredentials { user, .. } => {
            Some(user.clone())
        }
        crate::domains::auth::types::AuthenticationFlow::Authenticated { user, .. } => {
            Some(user.clone())
        }
        _ => None,
    });

    // Update cached permissions if available
    if let Some(perms) = maybe_perms.clone() {
        state.domains.auth.state.user_permissions = Some(perms);
    }

    match (user, maybe_perms) {
        (Some(user), Some(perms)) => {
            Task::done(auth::AuthMessage::LoginSuccess(user, perms))
        }
        // If permissions are unavailable, synthesize an empty set so we can still
        // drive the unified LoginSuccess path and emit AuthenticationComplete.
        (Some(user), None) => {
            let perms = ferrex_core::player_prelude::UserPermissions {
                user_id: user.id,
                roles: Vec::new(),
                permissions: std::collections::HashMap::new(),
                permission_details: None,
            };
            Task::done(auth::AuthMessage::LoginSuccess(user, perms))
        }
        // If the user can't be determined (shouldn't happen in this path),
        // fall back to previous behavior: proceed with watch-state only.
        (None, _) => {
            let api_service = &state.domains.auth.state.api_service;
            let api_service = api_service.clone();
            Task::perform(
                async move { api_service.get_watch_state().await },
                |result| match result {
                    Ok(watch_state) => {
                        auth::AuthMessage::WatchStatusLoaded(Ok(watch_state))
                    }
                    Err(e) => {
                        auth::AuthMessage::WatchStatusLoaded(Err(e.to_string()))
                    }
                },
            )
        }
    }
}

/// Handle auto-login check complete - proceed to load users
pub fn handle_auto_login_check_complete(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    log::info!("[Auth] Auto-login check complete, loading users");

    // Reset auth state and reuse the existing user-loading flow logic
    state.is_authenticated = false;
    state.domains.auth.state.is_authenticated = false;
    state.domains.auth.state.user_permissions = None;

    handle_load_users(state)
}

/// Handle successful auto-login
pub fn handle_auto_login_successful(
    state: &mut State,
    user: User,
) -> Task<auth::AuthMessage> {
    log::info!("[Auth] Auto-login successful for user: {}", user.username);

    state.domains.auth.state.auto_login_enabled =
        user.preferences.auto_login_enabled;
    state.domains.settings.preferences.auto_login_enabled =
        user.preferences.auto_login_enabled;

    // Query permissions from auth service
    let auth_service = &state.domains.auth.state.auth_service;
    let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
    let user_clone = user.clone();
    Task::perform(
        async move {
            (
                user_clone,
                svc.get_current_permissions().await.ok().flatten(),
            )
        },
        |(user, permissions)| {
            if let Some(perms) = permissions {
                auth::AuthMessage::LoginSuccess(user, perms)
            } else {
                let user_id = user.id;
                auth::AuthMessage::LoginSuccess(
                    user,
                    UserPermissions {
                        user_id,
                        roles: Vec::new(),
                        permissions: std::collections::HashMap::new(),
                        permission_details: None,
                    },
                )
            }
        },
    )
}

/// Handle watch status loaded
pub fn handle_watch_status_loaded(
    state: &mut State,
    result: Result<core::UserWatchState, String>,
) -> Task<auth::AuthMessage> {
    match result {
        Ok(watch_state) => {
            log::info!(
                "Watch status loaded successfully: {} in progress, {} completed",
                watch_state.in_progress.len(),
                watch_state.completed.len()
            );
            state.domains.media.state.user_watch_state = Some(watch_state);
            // Update curated carousels now that we have watch state
            curated::recompute_and_init_curated_carousels(state);
            curated::emit_initial_curated_snapshots(state);
        }
        Err(e) => {
            log::error!("Failed to load watch status: {}", e);
        }
    }
    Task::none()
}

// New device authentication flow handlers

/// Handle credential input update
pub fn handle_auth_flow_update_credential(
    state: &mut State,
    input: String,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{AuthenticationFlow, CredentialType};

    match &mut state.domains.auth.state.auth_flow {
        AuthenticationFlow::EnteringCredentials {
            input: current_input,
            input_type,
            error,
            ..
        } => {
            *current_input = SecureCredential::new(input.clone());
            *error = None;

            // Auto-submit when PIN is complete
            if matches!(input_type, CredentialType::Pin { .. })
                && current_input.len() == 4
            {
                return Task::done(auth::AuthMessage::SubmitCredential);
            }
        }

        // Allow PreAuthLogin password input to update via the same message
        AuthenticationFlow::PreAuthLogin {
            password, error, ..
        } => {
            *password = SecureCredential::new(input);
            *error = None;
        }

        _ => {}
    }
    Task::none()
}

/// Handle credential submission
pub fn handle_auth_flow_submit_credential(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{AuthenticationFlow, CredentialType};

    if let AuthenticationFlow::EnteringCredentials {
        user,
        input_type,
        input,
        remember_device,
        loading,
        ..
    } = &mut state.domains.auth.state.auth_flow.clone()
    {
        let auth_service = &state.domains.auth.state.auth_service;
        let svc: Arc<dyn AuthService> = std::sync::Arc::clone(auth_service);
        let user_clone = user.clone();
        let input_clone = input.clone();
        let remember = *remember_device;

        *loading = true;

        match input_type {
            CredentialType::Password => {
                return Task::perform(
                    async move {
                        svc.authenticate_device(
                            user_clone.username,
                            input_clone.as_str().to_string(),
                            remember,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    auth::AuthMessage::AuthResult,
                );
            }
            CredentialType::Pin { .. } => {
                return Task::perform(
                    async move {
                        svc.authenticate_pin(
                            user_clone.id,
                            input_clone.as_str().to_string(),
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    auth::AuthMessage::AuthResult,
                );
            }
        }
    }
    Task::none()
}

/// Handle pre-auth username updates
pub fn handle_pre_auth_update_username(
    state: &mut State,
    username: String,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;
    if let AuthenticationFlow::PreAuthLogin {
        username: u, error, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *u = username;
        *error = None;
    }
    Task::none()
}

/// Handle pre-auth password visibility toggle
pub fn handle_pre_auth_toggle_password_visibility(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;
    if let AuthenticationFlow::PreAuthLogin { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }
    Task::none()
}

/// Handle pre-auth remember device toggle
pub fn handle_pre_auth_toggle_remember_device(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;
    if let AuthenticationFlow::PreAuthLogin {
        remember_device, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *remember_device = !*remember_device;
        state.domains.auth.state.auto_login_enabled = *remember_device;
    }
    Task::none()
}

/// Handle pre-auth submit
pub fn handle_pre_auth_submit(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::PreAuthLogin {
        username,
        password,
        remember_device,
        loading,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *error = None;
        if username.trim().is_empty() {
            *error = Some("Username is required".to_string());
            return Task::none();
        }
        if password.as_str().is_empty() {
            *error = Some("Password is required".to_string());
            return Task::none();
        }

        let svc: Arc<dyn AuthService> =
            std::sync::Arc::clone(&state.domains.auth.state.auth_service);
        let username = username.clone();
        let password = password.as_str().to_string();
        let remember = *remember_device;
        *loading = true;

        return Task::perform(
            async move {
                svc.authenticate_device(username, password, remember)
                    .await
                    .map_err(|e| e.to_string())
            },
            auth::AuthMessage::AuthResult,
        );
    }
    Task::none()
}

/// Handle authentication result
pub fn handle_auth_flow_auth_result(
    state: &mut State,
    result: Result<crate::domains::auth::manager::PlayerAuthResult, String>,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{
        AuthenticationFlow, AuthenticationMode, CredentialType,
    };

    match result {
        Ok(auth_result) => {
            let auto_login_enabled =
                auth_result.user.preferences.auto_login_enabled;

            if let AuthenticationFlow::EnteringCredentials {
                remember_device,
                ..
            } = &mut state.domains.auth.state.auth_flow
            {
                *remember_device = auto_login_enabled;
            }

            state.domains.auth.state.auto_login_enabled = auto_login_enabled;

            // Mark as authenticated (both top-level and domain state)
            state.is_authenticated = true;
            state.domains.auth.state.is_authenticated = true;
            state.domains.auth.state.user_permissions =
                Some(auth_result.permissions.clone());

            // Check if we need to set up a PIN
            // Support both regular flow and pre-auth password flow
            let needs_pin_setup = match &state.domains.auth.state.auth_flow {
                AuthenticationFlow::EnteringCredentials {
                    input_type: CredentialType::Password,
                    remember_device: true,
                    ..
                } => !auth_result.device_has_pin,
                AuthenticationFlow::PreAuthLogin {
                    remember_device, ..
                } => *remember_device && !auth_result.device_has_pin,
                _ => false,
            };

            if needs_pin_setup {
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::SettingUpPin {
                        user: auth_result.user.clone(),
                        pin: SecureCredential::new(String::new()),
                        confirm_pin: SecureCredential::new(String::new()),
                        error: None,
                    };
                return Task::none();
            }

            // Successfully authenticated
            state.domains.auth.state.auth_flow =
                AuthenticationFlow::Authenticated {
                    user: auth_result.user.clone(),
                    mode: AuthenticationMode::Online,
                };

            // Unify with auto-login path: dispatch LoginSuccess immediately so
            // AuthenticationComplete is emitted regardless of watch-state outcome.
            Task::done(auth::AuthMessage::LoginSuccess(
                auth_result.user,
                auth_result.permissions,
            ))
        }
        Err(error) => {
            let mut flow = state.domains.auth.state.auth_flow.clone();
            match &mut flow {
                AuthenticationFlow::EnteringCredentials {
                    error: view_error,
                    loading,
                    attempts_remaining,
                    ..
                } => {
                    let is_lockout =
                        error.contains("locked") || error.contains("attempts");
                    *view_error = Some(error);
                    *loading = false;
                    if is_lockout {
                        if let Some(remaining) = attempts_remaining {
                            *remaining = remaining.saturating_sub(1);
                        }
                    }
                }
                AuthenticationFlow::PreAuthLogin {
                    error: view_error,
                    loading,
                    ..
                } => {
                    *view_error = Some(error);
                    *loading = false;
                }
                _ => {}
            }

            state.domains.auth.state.auth_flow = flow;
            Task::none()
        }
    }
}

/// Handle PIN setup submission
pub fn handle_auth_flow_submit_pin(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::SettingUpPin {
        pin,
        confirm_pin,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        if pin != confirm_pin {
            *error = Some("PINs do not match".to_string());
            return Task::none();
        }

        if pin.len() != 4 {
            *error = Some("PIN must be 4 digits".to_string());
            return Task::none();
        }

        let auth_service = &state.domains.auth.state.auth_service;
        let svc = std::sync::Arc::clone(auth_service);
        let pin_value = pin.as_str().to_string();

        return Task::perform(
            async move {
                svc.set_device_pin(pin_value)
                    .await
                    .map_err(|e| e.to_string())
            },
            auth::AuthMessage::PinSet,
        );
    }
    Task::none()
}

/// Handle PIN set result
pub fn handle_auth_flow_pin_set(
    state: &mut State,
    result: Result<(), String>,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::{AuthenticationFlow, AuthenticationMode};

    match result {
        Ok(()) => {
            let auth_flow = &state.domains.auth.state.auth_flow.clone();
            // PIN set successfully, complete authentication
            if let AuthenticationFlow::SettingUpPin { user, .. } = auth_flow {
                state.domains.auth.state.auth_flow =
                    AuthenticationFlow::Authenticated {
                        user: user.clone(),
                        mode: AuthenticationMode::Online,
                    };

                // BatchMetadataFetcher initialization moved to handle_auth_flow_completed
                // to ensure it's only initialized once after full auth flow completes

                // Reuse the LoginSuccess path to ensure AuthenticationComplete
                // is emitted even if watch-state retrieval fails.
                let permissions = state
                    .domains
                    .auth
                    .state
                    .user_permissions
                    .clone()
                    .unwrap_or_else(|| {
                        ferrex_core::player_prelude::UserPermissions {
                            user_id: user.id,
                            roles: Vec::new(),
                            permissions: std::collections::HashMap::new(),
                            permission_details: None,
                        }
                    });
                return Task::done(auth::AuthMessage::LoginSuccess(
                    user.clone(),
                    permissions,
                ));
            }
        }
        Err(error) => {
            if let AuthenticationFlow::SettingUpPin {
                error: view_error, ..
            } = &mut state.domains.auth.state.auth_flow
            {
                *view_error = Some(error);
            }
        }
    }
    Task::none()
}

/// Handle password visibility toggle
pub fn handle_auth_flow_toggle_password_visibility(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }
    Task::none()
}

/// Handle remember device toggle
pub fn handle_auth_flow_toggle_remember_device(
    state: &mut State,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials {
        remember_device, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *remember_device = !*remember_device;
        state.domains.auth.state.auto_login_enabled = *remember_device;
    }
    Task::none()
}

pub fn handle_remember_device_synced(
    state: &mut State,
    enabled: bool,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    state.domains.auth.state.auto_login_enabled = enabled;

    if let AuthenticationFlow::EnteringCredentials {
        remember_device, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *remember_device = enabled;
    }

    Task::none()
}

/// Transition into PIN setup when we have an authenticated user context.
pub fn handle_auth_flow_setup_pin(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    let user = match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::Authenticated { user, .. } => Some(user.clone()),
        AuthenticationFlow::EnteringCredentials { user, .. } => {
            Some(user.clone())
        }
        AuthenticationFlow::SettingUpPin { .. } => None,
        _ => None,
    };

    if let Some(user) = user {
        state.domains.auth.state.auth_flow = AuthenticationFlow::SettingUpPin {
            user,
            pin: SecureCredential::new(String::new()),
            confirm_pin: SecureCredential::new(String::new()),
            error: None,
        };
    }

    Task::none()
}

/// Update the PIN being entered during setup.
pub fn handle_auth_flow_update_pin(
    state: &mut State,
    raw_value: String,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::SettingUpPin { pin, error, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        let normalized = raw_value
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(4)
            .collect::<String>();

        *pin = SecureCredential::new(normalized);
        *error = None;
    }

    Task::none()
}

/// Update the confirmation PIN during setup.
pub fn handle_auth_flow_update_confirm_pin(
    state: &mut State,
    raw_value: String,
) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::SettingUpPin {
        confirm_pin, error, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        let normalized = raw_value
            .chars()
            .filter(|c| c.is_ascii_digit())
            .take(4)
            .collect::<String>();

        *confirm_pin = SecureCredential::new(normalized);
        *error = None;
    }

    Task::none()
}

/// Retry the current authentication step.
pub fn handle_auth_flow_retry(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    match &state.domains.auth.state.auth_flow {
        AuthenticationFlow::LoadingUsers
        | AuthenticationFlow::SelectingUser { .. } => handle_load_users(state),
        AuthenticationFlow::CheckingDevice { user } => {
            let svc: Arc<dyn AuthService> =
                Arc::clone(&state.domains.auth.state.auth_service);
            let user_clone = user.clone();
            let user_id = user_clone.id;

            Task::perform(
                async move {
                    svc.check_device_auth(user_id)
                        .await
                        .map_err(|e| e.to_string())
                },
                move |result| {
                    auth::AuthMessage::DeviceStatusChecked(user_clone, result)
                },
            )
        }
        AuthenticationFlow::EnteringCredentials { .. } => {
            handle_auth_flow_submit_credential(state)
        }
        _ => Task::none(),
    }
}

/// Handle local back navigation for the auth flow.
pub fn handle_auth_flow_back(state: &mut State) -> Task<auth::AuthMessage> {
    use crate::domains::auth::types::AuthenticationFlow;

    match state.domains.auth.state.auth_flow.clone() {
        AuthenticationFlow::EnteringCredentials { .. }
        | AuthenticationFlow::CheckingDevice { .. } => {
            handle_back_to_user_selection(state)
        }
        AuthenticationFlow::SettingUpPin { .. } => {
            handle_auth_flow_pin_set(state, Ok(()))
        }
        _ => Task::none(),
    }
}

pub fn handle_show_create_user(_state: &mut State) -> Task<auth::AuthMessage> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}
