use crate::common::messages::CrossDomainEvent;
use crate::domains::auth::manager::DeviceAuthStatus;
use crate::domains::auth::messages as auth;
use crate::domains::auth::security::secure_credential::SecureCredential;
use crate::domains::auth::state_types::AuthState;
use crate::domains::metadata::batch_fetcher;
use crate::infrastructure::services::api::ApiService;
use crate::state_refactored::State;
use ferrex_core::rbac::UserPermissions;
use ferrex_core::user::User;
use iced::Task;

/// Handle loading users for user selection screen
pub fn handle_load_users(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!("[Auth] handle_load_users called - loading users for selection");

    // Set auth flow to LoadingUsers to show the loading screen
    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;

    // Directly load users - we'll check setup status only if no users exist
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
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
                    auth::Message::CheckSetupStatus
                } else {
                    log::info!("[Auth] Successfully loaded {} users", users.len());
                    auth::Message::UsersLoaded(Ok(users))
                }
            }
            Err(e) => {
                log::error!("[Auth] Failed to load users: {}", e);
                // On error, check setup status as fallback
                auth::Message::CheckSetupStatus
            }
        },
    )
}

/// Handle users loaded response
pub fn handle_users_loaded(
    state: &mut State,
    result: Result<Vec<crate::domains::auth::dto::UserListItemDto>, String>,
) -> Task<auth::Message> {
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
            state.domains.auth.state.auth_flow = AuthenticationFlow::SelectingUser {
                users: loaded_users,
                error: None,
            };
        }
        Err(err) => {
            log::error!("[Auth] Error loading users: {}", err);
            state.domains.auth.state.auth_flow = AuthenticationFlow::SelectingUser {
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
pub fn handle_select_user(state: &mut State, user_id: uuid::Uuid) -> Task<auth::Message> {
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
        let user = ferrex_core::user::User {
            id: user_dto.id,
            username: user_dto.username.clone(),
            display_name: user_dto.display_name.clone(),
            avatar_url: user_dto.avatar_url.clone(),
            email: None,     // Not available in UserListItemDto
            is_active: true, // Assume active users are shown in the list
            last_login: user_dto.last_login,
            preferences: ferrex_core::user::UserPreferences::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Update state to checking device
        state.domains.auth.state.auth_flow =
            AuthenticationFlow::CheckingDevice { user: user.clone() };

        // Check device authentication status
        let auth_service = &state.domains.auth.state.auth_service;
        let svc = std::sync::Arc::clone(auth_service);
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
            move |result| auth::Message::DeviceStatusChecked(user_clone, result),
        );
    } else {
        log::error!("[Auth] User {} not found in current auth flow", user_id);
    }

    Task::none()
}

/// Handle device status check result
pub fn handle_device_status_checked(
    state: &mut State,
    user: ferrex_core::user::User,
    result: Result<DeviceAuthStatus, String>,
) -> Task<auth::Message> {
    use crate::domains::auth::types::{AuthenticationFlow, CredentialType};

    log::info!(
        "[Auth] handle_device_status_checked called for user {} with result: {:?}",
        user.username,
        result
    );

    match result {
        Ok(status) => {
            if status.device_registered && status.has_pin {
                // Device is trusted and has PIN - show PIN entry
                state.domains.auth.state.auth_flow = AuthenticationFlow::EnteringCredentials {
                    user,
                    input_type: CredentialType::Pin { max_length: 4 },
                    input: SecureCredential::new(String::new()),
                    show_password: false,
                    remember_device: false,
                    error: None,
                    attempts_remaining: Some(status.remaining_attempts.unwrap_or(5)),
                    loading: false,
                };
                Task::none()
            } else {
                // Check if admin has unlocked PIN for all users
                //let auth_manager = state.domains.auth.state.auth_manager.clone();
                //let user_clone = user.clone();

                // Device not registered/no PIN - show password entry
                state.domains.auth.state.auth_flow = AuthenticationFlow::EnteringCredentials {
                    user,
                    input_type: CredentialType::Password,
                    input: SecureCredential::new(String::new()),
                    show_password: false,
                    remember_device: false,
                    error: None,
                    attempts_remaining: None,
                    loading: false,
                };
                Task::none()
            }
        }
        Err(_) => {
            // Error checking device status - default to password entry
            state.domains.auth.state.auth_flow = AuthenticationFlow::EnteringCredentials {
                user,
                input_type: CredentialType::Password,
                input: SecureCredential::new(String::new()),
                show_password: false,
                remember_device: false,
                error: None,
                attempts_remaining: None,
                loading: false,
            };
            Task::none()
        }
    }
}

/// Handle enable admin PIN unlock
pub fn handle_enable_admin_pin_unlock(state: &mut State) -> Task<auth::Message> {
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    Task::future(async move {
        match svc.enable_admin_pin_unlock().await {
            Ok(_) => auth::Message::AdminPinUnlockToggled(Ok(true)),
            Err(e) => auth::Message::AdminPinUnlockToggled(Err(e.to_string())),
        }
    })
}

/// Handle disable admin PIN unlock
pub fn handle_disable_admin_pin_unlock(state: &mut State) -> Task<auth::Message> {
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    Task::future(async move {
        match svc.disable_admin_pin_unlock().await {
            Ok(_) => auth::Message::AdminPinUnlockToggled(Ok(false)),
            Err(e) => auth::Message::AdminPinUnlockToggled(Err(e.to_string())),
        }
    })
}

/// Handle admin PIN unlock toggle result
pub fn handle_admin_pin_unlock_toggled(
    state: &mut State,
    result: Result<bool, String>,
) -> Task<auth::Message> {
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
    user: ferrex_core::user::User,
    permissions: ferrex_core::rbac::UserPermissions,
) -> Task<auth::Message> {
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
                    auth::Message::WatchStatusLoaded(Ok(watch_state))
                }
                Err(e) => {
                    // Even on error, we're still authenticated
                    auth::Message::WatchStatusLoaded(Err(e.to_string()))
                }
            }
        },
    )
}

/// Handle login error
pub fn handle_login_error(state: &mut State, error: String) -> Task<auth::Message> {
    log::error!("Login failed: {}", error);

    // This is now handled by the AuthFlow handlers
    // Legacy login error handling - can be removed once all views are migrated
    Task::none()
}

/// Handle back to user selection
pub fn handle_back_to_user_selection(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;

    // Reload users
    handle_load_users(state)
}

/// Handle logout
pub fn handle_logout(state: &mut State) -> Task<auth::Message> {
    // Use trait-based auth service
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    Task::perform(
        async move {
            let _ = svc.logout().await;
        },
        |_| auth::Message::LogoutComplete,
    )
}

/// Handle logout complete
pub fn handle_logout_complete(state: &mut State) -> Task<auth::Message> {
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
pub fn handle_check_auth_status(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    log::info!("[Auth] handle_check_auth_status called - auto-login successful");

    // Auto-login has already authenticated the user in main.rs
    // We should mark as authenticated and load libraries (both top-level and domain state)
    state.is_authenticated = true;
    state.domains.auth.state.is_authenticated = true;

    // Set auth flow to CheckingAutoLogin so handle_login_success knows this is auto-login
    state.domains.auth.state.auth_flow = AuthenticationFlow::CheckingAutoLogin;

    // Get user and permissions from auth state
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);

    let (user, permissions) = tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current().block_on(async move {
            let user = svc.get_current_user().await.ok().flatten();
            let permissions = svc.get_current_permissions().await.ok().flatten();
            (user, permissions)
        })
    });

    if let (Some(user), Some(permissions)) = (user, permissions) {
        log::info!("[Auth] Auto-login authenticated as user: {}", user.username);
        // Store permissions in state
        state.domains.auth.state.user_permissions = Some(permissions.clone());
        // Return a LoginSuccess message to trigger the proper event flow
        // This ensures cross-domain events (AuthenticationComplete) are emitted
        return Task::done(auth::Message::LoginSuccess(user, permissions));
    }

    // If we couldn't get user/permissions, fall back to loading users
    log::error!("[Auth] Auto-login succeeded but couldn't retrieve user/permissions");
    state.domains.auth.state.auth_flow = AuthenticationFlow::LoadingUsers;
    handle_load_users(state)
}

/// Handle auth status confirmed with PIN (user has valid stored auth and PIN)
pub fn handle_auth_status_confirmed_with_pin(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] User has valid stored auth and PIN, proceeding to load libraries");

    // Mark as authenticated (both top-level and domain state)
    state.is_authenticated = true;
    state.domains.auth.state.is_authenticated = true;

    // Load permissions from stored auth
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    // Block to obtain permissions synchronously for immediate use
    state.domains.auth.state.user_permissions = tokio::task::block_in_place(move || {
        tokio::runtime::Handle::current()
            .block_on(async move { svc.get_current_permissions().await.ok().flatten() })
    });

    // Fetch watch status and then signal authentication complete
    let api_service = &state.domains.auth.state.api_service;
    let api_service = api_service.clone();
    Task::perform(
        async move { api_service.get_watch_state().await },
        |result| match result {
            Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
            Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
        },
    )
}

/// Handle check setup status
pub fn handle_check_setup_status(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] handle_check_setup_status called - checking if first-run setup is needed");

    // Use trait-based auth service
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);

    Task::perform(
        async move {
            log::info!("[Auth] Calling auth_service.check_setup_status()");
            svc.check_setup_status().await.map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(needs_setup) => {
                log::info!(
                    "[Auth] Setup status check result: needs_setup = {}",
                    needs_setup
                );
                auth::Message::SetupStatusChecked(needs_setup)
            }
            Err(e) => {
                log::error!("Failed to check setup status: {}", e);
                // If we can't check, assume setup is not needed
                auth::Message::SetupStatusChecked(false)
            }
        },
    )
}

/// Handle setup status checked response
pub fn handle_setup_status_checked(state: &mut State, needs_setup: bool) -> Task<auth::Message> {
    log::info!(
        "[Auth] handle_setup_status_checked called with needs_setup = {}",
        needs_setup
    );

    if needs_setup {
        log::info!("First-run setup needed, showing admin setup");
        // Initialize first-run setup state
        state.domains.auth.state.auth_flow =
            crate::domains::auth::types::AuthenticationFlow::FirstRunSetup {
                username: String::new(),
                password: crate::domains::auth::security::SecureCredential::new(String::new()),
                confirm_password: crate::domains::auth::security::SecureCredential::new(
                    String::new(),
                ),
                display_name: String::new(),
                setup_token: String::new(),
                show_password: false,
                error: None,
                loading: false,
            };
        Task::none()
    } else {
        log::info!("[Auth] No setup needed, checking for auto-login");
        // Set state to checking auto-login
        state.domains.auth.state.auth_flow =
            crate::domains::auth::types::AuthenticationFlow::CheckingAutoLogin;

        // Check if we have cached auth and auto-login is enabled
        let auth_service = &state.domains.auth.state.auth_service;
        let svc = std::sync::Arc::clone(auth_service);
        Task::perform(
            async move {
                log::info!("[Auth] Checking for cached auth and auto-login");

                // Check if we have cached auth
                if let Ok(Some(stored_auth)) = svc.load_from_keychain().await {
                    // Check if auto-login is enabled for this user
                    let device_auto_login = svc
                        .is_auto_login_enabled(&stored_auth.user.id)
                        .await
                        .unwrap_or(false);

                    let auto_login_enabled =
                        stored_auth.user.preferences.auto_login_enabled && device_auto_login;

                    log::info!(
                        "[Auth] Auto-login check - Server: {}, Device: {}, Combined: {}",
                        stored_auth.user.preferences.auto_login_enabled,
                        device_auto_login,
                        auto_login_enabled
                    );

                    if auto_login_enabled {
                        // Try to apply the stored auth
                        match svc.apply_stored_auth(stored_auth.clone()).await {
                            Ok(_) => {
                                log::info!("[Auth] Auto-login successful");
                                return auth::Message::AutoLoginSuccessful(stored_auth.user);
                            }
                            Err(e) => {
                                log::error!("[Auth] Auto-login failed: {}", e);
                            }
                        }
                    } else {
                        log::info!("[Auth] Auto-login disabled, proceeding to user selection");
                    }
                } else {
                    log::info!("[Auth] No cached auth found");
                }

                // If we get here, auto-login didn't work, load users
                auth::Message::AutoLoginCheckComplete
            },
            |msg| msg,
        )
    }
}

/// Handle auto-login check complete - proceed to load users
pub fn handle_auto_login_check_complete(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] Auto-login check complete, loading users");

    // Load users for normal flow
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    Task::perform(
        async move {
            log::info!("[Auth] Fetching all users from server");
            svc.get_all_users().await.map_err(|e| e.to_string())
        },
        |result| match result {
            Ok(users) => {
                log::info!("[Auth] Successfully loaded {} users", users.len());
                auth::Message::UsersLoaded(Ok(users))
            }
            Err(e) => {
                log::error!("[Auth] Failed to load users: {}", e);
                auth::Message::UsersLoaded(Err(e))
            }
        },
    )
}

/// Handle successful auto-login
pub fn handle_auto_login_successful(state: &mut State, user: User) -> Task<auth::Message> {
    log::info!("[Auth] Auto-login successful for user: {}", user.username);

    // Query permissions from auth service
    let auth_service = &state.domains.auth.state.auth_service;
    let svc = std::sync::Arc::clone(auth_service);
    let user_clone = user.clone();
    return Task::perform(
        async move {
            (
                user_clone,
                svc.get_current_permissions().await.ok().flatten(),
            )
        },
        |(user, permissions)| {
            if let Some(perms) = permissions {
                auth::Message::LoginSuccess(user, perms)
            } else {
                let user_id = user.id;
                auth::Message::LoginSuccess(
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
    );
}

/// Handle watch status loaded
pub fn handle_watch_status_loaded(
    state: &mut State,
    result: Result<ferrex_core::watch_status::UserWatchState, String>,
) -> Task<auth::Message> {
    match result {
        Ok(watch_state) => {
            log::info!(
                "Watch status loaded successfully: {} in progress, {} completed",
                watch_state.in_progress.len(),
                watch_state.completed.len()
            );
            state.domains.media.state.user_watch_state = Some(watch_state);
        }
        Err(e) => {
            log::error!("Failed to load watch status: {}", e);
            // Don't show error to user - watch status is not critical
        }
    }

    // Initialize batch metadata fetcher now that authentication is fully complete
    // This ensures it's only initialized once after the entire auth flow
    if state.batch_metadata_fetcher.is_none() {
        let api_service = &state.domains.auth.state.api_service;
        // BatchMetadataFetcher now uses ApiClientAdapter
        let batch_fetcher = std::sync::Arc::new(batch_fetcher::BatchMetadataFetcher::new(
            api_service.clone(),
            std::sync::Arc::clone(&state.domains.media.state.media_store),
        ));
        state.batch_metadata_fetcher = Some(batch_fetcher);
        log::info!("[BatchMetadataFetcher] Initialized ONCE after auth flow completed");
    } else {
        log::warn!("[BatchMetadataFetcher] Already initialized - preventing duplicate initialization");
    }

    // Authentication flow is complete
    // BatchMetadataFetcher has been initialized, authentication is ready
    Task::none()
}

// New device authentication flow handlers

/// Handle credential input update
pub fn handle_auth_flow_update_credential(state: &mut State, input: String) -> Task<auth::Message> {
    use crate::domains::auth::types::{AuthenticationFlow, CredentialType};

    if let AuthenticationFlow::EnteringCredentials {
        input: current_input,
        input_type,
        error,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *current_input = SecureCredential::new(input.clone());
        *error = None;

        // Auto-submit when PIN is complete
        if matches!(input_type, CredentialType::Pin { .. }) && input.len() == 4 {
            return Task::done(auth::Message::SubmitCredential);
        }
    }
    Task::none()
}

/// Handle credential submission
pub fn handle_auth_flow_submit_credential(state: &mut State) -> Task<auth::Message> {
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
        let svc = std::sync::Arc::clone(auth_service);
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
                    |result| auth::Message::AuthResult(result),
                );
            }
            CredentialType::Pin { .. } => {
                return Task::perform(
                    async move {
                        svc.authenticate_pin(user_clone.id, input_clone.as_str().to_string())
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |result| auth::Message::AuthResult(result),
                );
            }
        }
    }
    Task::none()
}

/// Handle authentication result
pub fn handle_auth_flow_auth_result(
    state: &mut State,
    result: Result<crate::domains::auth::manager::PlayerAuthResult, String>,
) -> Task<auth::Message> {
    use crate::domains::auth::types::{AuthenticationFlow, AuthenticationMode, CredentialType};

    match result {
        Ok(auth_result) => {
            // Mark as authenticated (both top-level and domain state)
            state.is_authenticated = true;
            state.domains.auth.state.is_authenticated = true;
            state.domains.auth.state.user_permissions = Some(auth_result.permissions.clone());

            // Check if we need to set up a PIN
            if let AuthenticationFlow::EnteringCredentials {
                input_type: CredentialType::Password,
                remember_device: true,
                ..
            } = &state.domains.auth.state.auth_flow
            {
                if !auth_result.device_has_pin {
                    // Need to set up PIN
                    state.domains.auth.state.auth_flow = AuthenticationFlow::SettingUpPin {
                        user: auth_result.user.clone(),
                        pin: SecureCredential::new(String::new()),
                        confirm_pin: SecureCredential::new(String::new()),
                        error: None,
                    };
                    return Task::none();
                }
            }

            // Successfully authenticated
            state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
                user: auth_result.user.clone(),
                mode: AuthenticationMode::Online,
            };

            // BatchMetadataFetcher initialization moved to handle_auth_flow_completed
            // to ensure it's only initialized once after full auth flow completes

            // Fetch watch status and then signal authentication complete
            let api_service = &state.domains.auth.state.api_service;
            let api_service = api_service.clone();
            Task::perform(
                async move { api_service.get_watch_state().await },
                |result| match result {
                    Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
                    Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
                },
            )
        }
        Err(error) => {
            if let AuthenticationFlow::EnteringCredentials {
                error: view_error,
                loading,
                attempts_remaining,
                ..
            } = &mut state.domains.auth.state.auth_flow
            {
                // Check if error indicates lockout before moving error
                let is_lockout = error.contains("locked") || error.contains("attempts");

                *view_error = Some(error);
                *loading = false;

                // Update attempts remaining if it's a lockout error
                if is_lockout {
                    if let Some(remaining) = attempts_remaining {
                        *remaining = remaining.saturating_sub(1);
                    }
                }
            }
            Task::none()
        }
    }
}

/// Handle PIN setup submission
pub fn handle_auth_flow_submit_pin(state: &mut State) -> Task<auth::Message> {
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
            |result| auth::Message::PinSet(result),
        );
    }
    Task::none()
}

/// Handle PIN set result
pub fn handle_auth_flow_pin_set(
    state: &mut State,
    result: Result<(), String>,
) -> Task<auth::Message> {
    use crate::domains::auth::types::{AuthenticationFlow, AuthenticationMode};

    match result {
        Ok(()) => {
            // PIN set successfully, complete authentication
            if let AuthenticationFlow::SettingUpPin { user, .. } =
                &state.domains.auth.state.auth_flow
            {
                state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
                    user: user.clone(),
                    mode: AuthenticationMode::Online,
                };

                // BatchMetadataFetcher initialization moved to handle_auth_flow_completed
                // to ensure it's only initialized once after full auth flow completes

                // PIN setup complete - fetch watch status and signal authentication complete
                let api_service = &state.domains.auth.state.api_service;
                let api_service = api_service.clone();
                return Task::perform(
                    async move { api_service.get_watch_state().await },
                    |result| match result {
                        Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
                        Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
                    },
                );
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
pub fn handle_auth_flow_toggle_password_visibility(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }
    Task::none()
}

/// Handle remember device toggle
pub fn handle_auth_flow_toggle_remember_device(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials {
        remember_device, ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *remember_device = !*remember_device;
    }
    Task::none()
}

// Stub implementations for legacy handlers - TO BE REMOVED when views are migrated

pub fn handle_show_pin_entry(
    _state: &mut State,
    _user: ferrex_core::user::User,
) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_show_create_user(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_pin_digit_pressed(_state: &mut State, _digit: char) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_pin_backspace(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_pin_clear(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_pin_submit(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_show_password_login(_state: &mut State, _username: String) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_password_login_update_username(
    _state: &mut State,
    _username: String,
) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_password_login_update_password(
    _state: &mut State,
    _password: String,
) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_password_login_toggle_visibility(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_password_login_toggle_remember(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

pub fn handle_password_login_submit(_state: &mut State) -> Task<auth::Message> {
    // Legacy handler - replaced by AuthFlow
    Task::none()
}

// Admin setup handlers

pub fn handle_update_setup_field(
    state: &mut State,
    field: auth::SetupField,
) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        match field {
            auth::SetupField::Username(value) => *username = value,
            auth::SetupField::Password(value) => {
                *password = crate::domains::auth::security::SecureCredential::new(value)
            }
            auth::SetupField::ConfirmPassword(value) => {
                *confirm_password = crate::domains::auth::security::SecureCredential::new(value)
            }
            auth::SetupField::DisplayName(value) => *display_name = value,
            auth::SetupField::SetupToken(value) => *setup_token = value,
        }
    }

    Task::none()
}

pub fn handle_toggle_setup_password_visibility(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::FirstRunSetup { show_password, .. } =
        &mut state.domains.auth.state.auth_flow
    {
        *show_password = !*show_password;
    }

    Task::none()
}

pub fn handle_submit_setup(state: &mut State) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        error,
        loading,
        ..
    } = &mut state.domains.auth.state.auth_flow.clone()
    {
        // Validate inputs
        if username.is_empty() {
            *error = Some("Username is required".to_string());
            return Task::none();
        }

        if password.as_str().is_empty() {
            *error = Some("Password is required".to_string());
            return Task::none();
        }

        if password.as_str() != confirm_password.as_str() {
            *error = Some("Passwords do not match".to_string());
            return Task::none();
        }

        // Set loading state
        if let AuthenticationFlow::FirstRunSetup {
            loading: ref mut l, ..
        } = &mut state.domains.auth.state.auth_flow
        {
            *l = true;
        }

        // Submit to server
        let api_service = state.domains.auth.state.api_service.clone();
        let username = username.clone();
        let password = password.as_str().to_string();
        let display_name = if display_name.is_empty() {
            None
        } else {
            Some(display_name.clone())
        };
        let setup_token = if setup_token.is_empty() {
            None
        } else {
            Some(setup_token.clone())
        };

        return Task::perform(
            async move {
                // Create initial admin requires special handling
                if let Some(pin) = display_name {
                    api_service
                        .create_initial_admin(username, password, Some(pin))
                        .await
                        .map_err(|e| e.to_string())
                        .map(|(user, token)| ferrex_core::user::AuthToken {
                            access_token: token.access_token,
                            refresh_token: token.refresh_token,
                            expires_in: 900,
                        })
                } else {
                    api_service
                        .create_initial_admin(username, password, None)
                        .await
                        .map_err(|e| e.to_string())
                        .map(|(user, token)| ferrex_core::user::AuthToken {
                            access_token: token.access_token,
                            refresh_token: token.refresh_token,
                            expires_in: 900,
                        })
                }
            },
            |result| match result {
                Ok(auth_token) => {
                    log::info!("[Auth] Admin setup successful");
                    auth::Message::SetupComplete(auth_token.access_token, auth_token.refresh_token)
                }
                Err(e) => {
                    log::error!("[Auth] Admin setup failed: {}", e);
                    auth::Message::SetupError(e.to_string())
                }
            },
        );
    }

    Task::none()
}

pub fn handle_setup_complete(
    state: &mut State,
    access_token: String,
    refresh_token: String,
) -> Task<auth::Message> {
    log::info!("[Auth] Admin setup complete, storing tokens");

    // Store the auth tokens
    let svc = state.domains.auth.state.auth_service.clone();
    let api_service = state.domains.auth.state.api_service.clone();
    let server_url = api_service.build_url("");

    // Create auth token structure
    let auth_token = ferrex_core::user::AuthToken {
        access_token,
        refresh_token,
        expires_in: 900, // 15 minutes default
    };

    return Task::perform(
        async move {
            // Set the token on the API service
            api_service.set_token(Some(auth_token.clone())).await;

            // Now fetch the current user
            let user: ferrex_core::user::User = match api_service.get("/api/users/me").await {
                Ok(user) => user,
                Err(e) => return Err(format!("Failed to get user: {}", e)),
            };

            // Get user permissions
            let permissions: ferrex_core::rbac::UserPermissions =
                match api_service.get("/api/users/me/permissions").await {
                    Ok(perms) => perms,
                    Err(e) => {
                        log::warn!(
                            "[Auth] Failed to get permissions, using default admin permissions: {}",
                            e
                        );
                        // Create default admin permissions
                        ferrex_core::rbac::UserPermissions {
                            user_id: user.id,
                            roles: vec![],
                            permissions: std::collections::HashMap::new(),
                            permission_details: None,
                        }
                    }
                };

            // Update auth state
            if let Err(e) = svc
                .authenticate(user.clone(), auth_token, permissions.clone(), server_url)
                .await
            {
                log::error!("[Auth] Failed to set auth state: {}", e);
            }

            // Save to keychain
            if let Err(e) = svc.save_current_auth().await {
                log::warn!("[Auth] Failed to save auth after setup: {}", e);
            }

            Ok((user, permissions))
        },
        |result: Result<(ferrex_core::user::User, ferrex_core::rbac::UserPermissions), String>| {
            match result {
                Ok((user, permissions)) => {
                    log::info!("[Auth] Retrieved admin user: {}", user.username);
                    auth::Message::LoginSuccess(user, permissions)
                }
                Err(e) => {
                    log::error!("[Auth] Failed to complete setup: {}", e);
                    auth::Message::SetupError(e)
                }
            }
        },
    );
}

pub fn handle_setup_error(state: &mut State, error: String) -> Task<auth::Message> {
    use crate::domains::auth::types::AuthenticationFlow;

    if let AuthenticationFlow::FirstRunSetup {
        error: ref mut err,
        loading: ref mut l,
        ..
    } = &mut state.domains.auth.state.auth_flow
    {
        *err = Some(error);
        *l = false;
    }

    Task::none()
}
