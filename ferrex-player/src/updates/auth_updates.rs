use crate::auth_manager::DeviceAuthStatus;
use crate::{
    messages::{auth, cross_domain, CrossDomainEvent, DomainMessage},
    security::SecureCredential,
    state::{State, ViewState},
};
use iced::Task;

/// Handle loading users for user selection screen
pub fn handle_load_users(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] handle_load_users called - loading users for selection");

    // Directly load users - we'll check setup status only if no users exist
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        Task::perform(
            async move {
                log::info!("[Auth] Fetching all users from server");
                auth_manager.get_all_users().await
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
    } else {
        log::error!("[Auth] No auth_manager available");
        Task::none()
    }
}

/// Handle users loaded response
pub fn handle_users_loaded(
    state: &mut State,
    result: Result<Vec<crate::auth_dto::UserListItemDto>, String>,
) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;

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
            state.auth_flow = AuthenticationFlow::SelectingUser {
                users: loaded_users,
                error: None,
            };
        }
        Err(err) => {
            log::error!("[Auth] Error loading users: {}", err);
            state.auth_flow = AuthenticationFlow::SelectingUser {
                users: Vec::new(),
                error: Some(err),
            };
        }
    }

    log::info!("[Auth] Auth flow after update: {:?}", state.auth_flow);
    Task::none()
}

/// Handle user selection
pub fn handle_select_user(state: &mut State, user_id: uuid::Uuid) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;

    log::info!("[Auth] handle_select_user called for user_id: {}", user_id);

    // Clone the user before modifying state
    let selected_user_dto = match &state.auth_flow {
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
            email: None, // Not available in UserListItemDto
            is_active: true, // Assume active users are shown in the list
            last_login: user_dto.last_login,
            preferences: ferrex_core::user::UserPreferences::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Update state to checking device
        state.auth_flow = AuthenticationFlow::CheckingDevice { user: user.clone() };

        // Check device authentication status
        if let Some(auth_manager) = &state.auth_manager {
            let auth_manager = auth_manager.clone();
            let user_id = user.id;
            let user_clone = user.clone();

            log::info!("[Auth] Checking device auth status for user {}", user_id);

            return Task::perform(
                async move { auth_manager.check_device_auth(user_id).await },
                move |result| {
                    auth::Message::DeviceStatusChecked(
                        user_clone,
                        result.map_err(|e| e.to_string()),
                    )
                },
            );
        } else {
            log::error!("[Auth] No auth_manager available");
        }
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
    use crate::state::{AuthenticationFlow, CredentialType};

    log::info!("[Auth] handle_device_status_checked called for user {} with result: {:?}", user.username, result);

    match result {
        Ok(status) => {
            if status.device_registered && status.has_pin {
                // Device is trusted and has PIN - show PIN entry
                state.auth_flow = AuthenticationFlow::EnteringCredentials {
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
                let auth_manager = state.auth_manager.clone();
                let user_clone = user.clone();
                
                // Device not registered/no PIN - show password entry
                state.auth_flow = AuthenticationFlow::EnteringCredentials {
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
            state.auth_flow = AuthenticationFlow::EnteringCredentials {
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
    let auth_manager = state.auth_manager.clone();
    
    Task::future(async move {
        if let Some(auth_manager) = auth_manager {
            match auth_manager.enable_admin_pin_unlock().await {
                Ok(_) => auth::Message::AdminPinUnlockToggled(Ok(true)),
                Err(e) => auth::Message::AdminPinUnlockToggled(Err(e.to_string())),
            }
        } else {
            auth::Message::AdminPinUnlockToggled(Err("Auth manager not initialized".to_string()))
        }
    })
}

/// Handle disable admin PIN unlock
pub fn handle_disable_admin_pin_unlock(state: &mut State) -> Task<auth::Message> {
    let auth_manager = state.auth_manager.clone();
    
    Task::future(async move {
        if let Some(auth_manager) = auth_manager {
            match auth_manager.disable_admin_pin_unlock().await {
                Ok(_) => auth::Message::AdminPinUnlockToggled(Ok(false)),
                Err(e) => auth::Message::AdminPinUnlockToggled(Err(e.to_string())),
            }
        } else {
            auth::Message::AdminPinUnlockToggled(Err("Auth manager not initialized".to_string()))
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
            log::info!("Admin PIN unlock is now {}", if enabled { "enabled" } else { "disabled" });
            // Could show a notification or update UI state here
            Task::none()
        }
        Err(error) => {
            log::error!("Failed to toggle admin PIN unlock: {}", error);
            state.error_message = Some(format!("Failed to toggle admin PIN unlock: {}", error));
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
    use crate::state::AuthenticationFlow;

    log::info!("Login successful for user: {}", user.username);

    // Mark as authenticated
    state.is_authenticated = true;

    // Store permissions
    state.user_permissions = Some(permissions.clone());

    // Clear auth flow
    state.auth_flow = AuthenticationFlow::default();

    // API client should already be initialized from main.rs
    // The auth_manager already set the token in the shared api_client instance

    // Metadata service is already initialized during app startup in main.rs

    // Initialize batch metadata fetcher now that we have authentication
    log::debug!(
        "Checking api_client availability: is_some = {}",
        state.api_client.is_some()
    );
    if let Some(api_client) = &state.api_client {
        let batch_fetcher =
            std::sync::Arc::new(crate::batch_metadata_fetcher::BatchMetadataFetcher::new(
                std::sync::Arc::new(api_client.clone()),
            ));
        state.batch_metadata_fetcher = Some(batch_fetcher);
        log::info!("[BatchMetadataFetcher] Initialized after successful login with authenticated ApiClient");
    } else {
        log::error!("No ApiClient available after login - BatchMetadataFetcher not initialized");
        log::error!(
            "State debug - server_url: {}, auth_manager is_some: {}",
            state.server_url,
            state.auth_manager.is_some()
        );
    }

    // After successful login, fetch watch status and then trigger library loading
    // We'll chain the watch status loading with a completion message
    if let Some(api_client) = &state.api_client {
        let api_client = api_client.clone();
        let user_clone = user.clone();
        let permissions_clone = permissions.clone();

        Task::perform(
            async move { api_client.get_watch_state().await },
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
    } else {
        // No API client, but still authenticated - signal completion
        Task::done(auth::Message::WatchStatusLoaded(Err(
            "No API client available".to_string(),
        )))
    }
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
    use crate::state::AuthenticationFlow;

    state.auth_flow = AuthenticationFlow::LoadingUsers;

    // Reload users
    handle_load_users(state)
}

/// Handle logout
pub fn handle_logout(state: &mut State) -> Task<auth::Message> {
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();

        Task::perform(
            async move {
                let _ = auth_manager.logout().await;
            },
            |_| auth::Message::LogoutComplete,
        )
    } else {
        Task::none()
    }
}

/// Handle logout complete
pub fn handle_logout_complete(state: &mut State) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;

    // Reset authentication state
    state.is_authenticated = false;
    state.auth_flow = AuthenticationFlow::LoadingUsers;
    state.user_permissions = None;

    // Clear any loaded media data
    state.media_store.write().unwrap().clear();
    state.libraries.clear();
    state.current_library_id = None;

    // Load users for selection
    handle_load_users(state)
}

// Legacy create user handler removed

// Legacy password login handlers removed - replaced by AuthFlow handlers

/// Handle auth status check (when stored auth is loaded from keychain)
pub fn handle_check_auth_status(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] handle_check_auth_status called - auto-login successful");
    
    // Auto-login has already authenticated the user in main.rs
    // We should mark as authenticated and load libraries
    state.is_authenticated = true;
    
    // Get user and permissions from auth state
    if let Some(auth_manager) = &state.auth_manager {
        let (user, permissions) = auth_manager.auth_state().with_state(|s| {
            match s {
                crate::auth_state::AuthState::Authenticated { user, permissions, .. } => {
                    (Some(user.clone()), Some(permissions.clone()))
                }
                _ => (None, None)
            }
        });
        
        if let (Some(user), Some(permissions)) = (user, permissions) {
            log::info!("[Auth] Auto-login authenticated as user: {}", user.username);
            // Store permissions in state
            state.user_permissions = Some(permissions.clone());
            // Now proceed with login success flow
            return handle_login_success(state, user, permissions);
        }
    }
    
    // If we couldn't get user/permissions, fall back to loading users
    log::error!("[Auth] Auto-login succeeded but couldn't retrieve user/permissions");
    handle_load_users(state)
}

/// Handle auth status confirmed with PIN (user has valid stored auth and PIN)
pub fn handle_auth_status_confirmed_with_pin(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] User has valid stored auth and PIN, proceeding to load libraries");

    // Mark as authenticated
    state.is_authenticated = true;

    // Load permissions from stored auth
    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();
        // Block on getting permissions since we need them immediately
        state.user_permissions = tokio::task::block_in_place(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { auth_manager.get_current_permissions().await })
        });
    }

    // Initialize batch metadata fetcher now that we have authentication
    log::debug!(
        "Checking api_client availability after auth confirmed: is_some = {}",
        state.api_client.is_some()
    );
    if let Some(api_client) = &state.api_client {
        let batch_fetcher =
            std::sync::Arc::new(crate::batch_metadata_fetcher::BatchMetadataFetcher::new(
                std::sync::Arc::new(api_client.clone()),
            ));
        state.batch_metadata_fetcher = Some(batch_fetcher);
        log::info!("[BatchMetadataFetcher] Initialized after auth confirmation with authenticated ApiClient");
    } else {
        log::error!(
            "No ApiClient available after auth confirmation - BatchMetadataFetcher not initialized"
        );
    }

    // Fetch watch status and then signal authentication complete
    if let Some(api_client) = &state.api_client {
        let api_client = api_client.clone();
        Task::perform(
            async move { api_client.get_watch_state().await },
            |result| match result {
                Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
                Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
            },
        )
    } else {
        // No API client, but still authenticated - proceed to completion
        Task::done(auth::Message::WatchStatusLoaded(Err(
            "No API client available".to_string(),
        )))
    }
}

/// Handle check setup status
pub fn handle_check_setup_status(state: &mut State) -> Task<auth::Message> {
    log::info!("[Auth] handle_check_setup_status called - checking if first-run setup is needed");

    if let Some(auth_manager) = &state.auth_manager {
        let auth_manager = auth_manager.clone();

        Task::perform(
            async move {
                log::info!("[Auth] Calling auth_manager.check_setup_status()");
                auth_manager.check_setup_status().await
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
    } else {
        log::error!("[Auth] No auth_manager available in handle_check_setup_status");
        Task::none()
    }
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
        state.auth_flow = crate::state::AuthenticationFlow::FirstRunSetup {
            username: String::new(),
            password: crate::security::SecureCredential::new(String::new()),
            confirm_password: crate::security::SecureCredential::new(String::new()),
            display_name: String::new(),
            setup_token: String::new(),
            show_password: false,
            error: None,
            loading: false,
        };
        Task::none()
    } else {
        log::info!("[Auth] No setup needed, continuing to load users");
        // Load users for normal flow
        if let Some(auth_manager) = &state.auth_manager {
            let auth_manager = auth_manager.clone();
            Task::perform(
                async move {
                    log::info!("[Auth] Fetching all users from server");
                    auth_manager.get_all_users().await
                },
                |result| match result {
                    Ok(users) => {
                        log::info!("[Auth] Successfully loaded {} users", users.len());
                        auth::Message::UsersLoaded(Ok(users))
                    }
                    Err(e) => {
                        log::error!("[Auth] Failed to load users: {}", e);
                        auth::Message::UsersLoaded(Err(e.to_string()))
                    }
                },
            )
        } else {
            log::error!("[Auth] No auth_manager available");
            Task::none()
        }
    }
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
            state.user_watch_state = Some(watch_state);
        }
        Err(e) => {
            log::error!("Failed to load watch status: {}", e);
            // Don't show error to user - watch status is not critical
        }
    }

    // Authentication flow is complete - emit cross-domain event
    Task::done(auth::Message::_EmitCrossDomainEvent(
        CrossDomainEvent::AuthenticationComplete,
    ))
}

// New device authentication flow handlers

/// Handle credential input update
pub fn handle_auth_flow_update_credential(state: &mut State, input: String) -> Task<auth::Message> {
    use crate::state::{AuthenticationFlow, CredentialType};

    if let AuthenticationFlow::EnteringCredentials {
        input: current_input,
        input_type,
        error,
        ..
    } = &mut state.auth_flow
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
    use crate::state::{AuthenticationFlow, CredentialType};

    if let AuthenticationFlow::EnteringCredentials {
        user,
        input_type,
        input,
        remember_device,
        loading,
        ..
    } = &mut state.auth_flow.clone()
    {
        if let Some(auth_manager) = &state.auth_manager {
            let auth_manager = auth_manager.clone();
            let user_clone = user.clone();
            let input_clone = input.clone();
            let remember = *remember_device;

            *loading = true;

            match input_type {
                CredentialType::Password => {
                    return Task::perform(
                        async move {
                            auth_manager
                                .authenticate_device(user_clone.username, input_clone.as_str().to_string(), remember)
                                .await
                        },
                        |result| auth::Message::AuthResult(result.map_err(|e| e.to_string())),
                    );
                }
                CredentialType::Pin { .. } => {
                    return Task::perform(
                        async move {
                            auth_manager
                                .authenticate_pin(user_clone.id, input_clone.as_str().to_string())
                                .await
                        },
                        |result| auth::Message::AuthResult(result.map_err(|e| e.to_string())),
                    );
                }
            }
        }
    }
    Task::none()
}

/// Handle authentication result
pub fn handle_auth_flow_auth_result(
    state: &mut State,
    result: Result<crate::auth_manager::PlayerAuthResult, String>,
) -> Task<auth::Message> {
    use crate::state::{AuthenticationFlow, AuthenticationMode, CredentialType};

    match result {
        Ok(auth_result) => {
            // Mark as authenticated
            state.is_authenticated = true;
            state.user_permissions = Some(auth_result.permissions.clone());

            // Check if we need to set up a PIN
            if let AuthenticationFlow::EnteringCredentials {
                input_type: CredentialType::Password,
                remember_device: true,
                ..
            } = &state.auth_flow
            {
                if !auth_result.device_has_pin {
                    // Need to set up PIN
                    state.auth_flow = AuthenticationFlow::SettingUpPin {
                        user: auth_result.user.clone(),
                        pin: SecureCredential::new(String::new()),
                        confirm_pin: SecureCredential::new(String::new()),
                        error: None,
                    };
                    return Task::none();
                }
            }

            // Successfully authenticated
            state.auth_flow = AuthenticationFlow::Authenticated {
                user: auth_result.user.clone(),
                mode: AuthenticationMode::Online,
            };

            // Initialize batch metadata fetcher now that we have authentication
            if let Some(api_client) = &state.api_client {
                let batch_fetcher =
                    std::sync::Arc::new(crate::batch_metadata_fetcher::BatchMetadataFetcher::new(
                        std::sync::Arc::new(api_client.clone()),
                    ));
                state.batch_metadata_fetcher = Some(batch_fetcher);
                log::info!("[BatchMetadataFetcher] Initialized after successful auth flow with authenticated ApiClient");
            } else {
                log::error!(
                    "No ApiClient available after auth flow - BatchMetadataFetcher not initialized"
                );
            }

            // Fetch watch status and then signal authentication complete
            if let Some(api_client) = &state.api_client {
                let api_client = api_client.clone();
                Task::perform(
                    async move { api_client.get_watch_state().await },
                    |result| match result {
                        Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
                        Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
                    },
                )
            } else {
                Task::done(auth::Message::WatchStatusLoaded(Err(
                    "No API client available".to_string(),
                )))
            }
        }
        Err(error) => {
            if let AuthenticationFlow::EnteringCredentials {
                error: view_error,
                loading,
                attempts_remaining,
                ..
            } = &mut state.auth_flow
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
    use crate::state::AuthenticationFlow;

    if let AuthenticationFlow::SettingUpPin {
        pin,
        confirm_pin,
        error,
        ..
    } = &mut state.auth_flow
    {
        if pin != confirm_pin {
            *error = Some("PINs do not match".to_string());
            return Task::none();
        }

        if pin.len() != 4 {
            *error = Some("PIN must be 4 digits".to_string());
            return Task::none();
        }

        if let Some(auth_manager) = &state.auth_manager {
            let auth_manager = auth_manager.clone();
            let pin_value = pin.as_str().to_string();

            return Task::perform(
                async move { auth_manager.set_device_pin(pin_value).await },
                |result| auth::Message::PinSet(result.map_err(|e| e.to_string())),
            );
        }
    }
    Task::none()
}

/// Handle PIN set result
pub fn handle_auth_flow_pin_set(
    state: &mut State,
    result: Result<(), String>,
) -> Task<auth::Message> {
    use crate::state::{AuthenticationFlow, AuthenticationMode};

    match result {
        Ok(()) => {
            // PIN set successfully, complete authentication
            if let AuthenticationFlow::SettingUpPin { user, .. } = &state.auth_flow {
                state.auth_flow = AuthenticationFlow::Authenticated {
                    user: user.clone(),
                    mode: AuthenticationMode::Online,
                };

                // Initialize batch metadata fetcher now that we have authentication
                if let Some(api_client) = &state.api_client {
                    let batch_fetcher = std::sync::Arc::new(
                        crate::batch_metadata_fetcher::BatchMetadataFetcher::new(
                            std::sync::Arc::new(api_client.clone()),
                        ),
                    );
                    state.batch_metadata_fetcher = Some(batch_fetcher);
                    log::info!("[BatchMetadataFetcher] Initialized after PIN setup with authenticated ApiClient");
                } else {
                    log::error!("No ApiClient available after PIN setup - BatchMetadataFetcher not initialized");
                }

                // PIN setup complete - fetch watch status and signal authentication complete
                if let Some(api_client) = &state.api_client {
                    let api_client = api_client.clone();
                    return Task::perform(
                        async move { api_client.get_watch_state().await },
                        |result| match result {
                            Ok(watch_state) => auth::Message::WatchStatusLoaded(Ok(watch_state)),
                            Err(e) => auth::Message::WatchStatusLoaded(Err(e.to_string())),
                        },
                    );
                } else {
                    return Task::done(auth::Message::WatchStatusLoaded(Err(
                        "No API client available".to_string(),
                    )));
                }
            }
        }
        Err(error) => {
            if let AuthenticationFlow::SettingUpPin {
                error: view_error, ..
            } = &mut state.auth_flow
            {
                *view_error = Some(error);
            }
        }
    }
    Task::none()
}

/// Handle password visibility toggle
pub fn handle_auth_flow_toggle_password_visibility(state: &mut State) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials { show_password, .. } = &mut state.auth_flow {
        *show_password = !*show_password;
    }
    Task::none()
}

/// Handle remember device toggle
pub fn handle_auth_flow_toggle_remember_device(state: &mut State) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;

    if let AuthenticationFlow::EnteringCredentials {
        remember_device, ..
    } = &mut state.auth_flow
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

pub fn handle_update_setup_field(state: &mut State, field: auth::SetupField) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;
    
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        ..
    } = &mut state.auth_flow
    {
        match field {
            auth::SetupField::Username(value) => *username = value,
            auth::SetupField::Password(value) => *password = crate::security::SecureCredential::new(value),
            auth::SetupField::ConfirmPassword(value) => *confirm_password = crate::security::SecureCredential::new(value),
            auth::SetupField::DisplayName(value) => *display_name = value,
            auth::SetupField::SetupToken(value) => *setup_token = value,
        }
    }
    
    Task::none()
}

pub fn handle_toggle_setup_password_visibility(state: &mut State) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;
    
    if let AuthenticationFlow::FirstRunSetup { show_password, .. } = &mut state.auth_flow {
        *show_password = !*show_password;
    }
    
    Task::none()
}

pub fn handle_submit_setup(state: &mut State) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;
    
    if let AuthenticationFlow::FirstRunSetup {
        username,
        password,
        confirm_password,
        display_name,
        setup_token,
        error,
        loading,
        ..
    } = &mut state.auth_flow.clone()
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
        if let AuthenticationFlow::FirstRunSetup { loading: ref mut l, .. } = &mut state.auth_flow {
            *l = true;
        }
        
        // Submit to server
        if let Some(api_client) = &state.api_client {
            let api_client = api_client.clone();
            let username = username.clone();
            let password = password.as_str().to_string();
            let display_name = if display_name.is_empty() { None } else { Some(display_name.clone()) };
            let setup_token = if setup_token.is_empty() { None } else { Some(setup_token.clone()) };
            
            return Task::perform(
                async move {
                    api_client.create_initial_admin(username, password, display_name, setup_token).await
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
    }
    
    Task::none()
}

pub fn handle_setup_complete(state: &mut State, access_token: String, refresh_token: String) -> Task<auth::Message> {
    log::info!("[Auth] Admin setup complete, storing tokens");
    
    // Store the auth tokens
    if let Some(auth_manager) = &state.auth_manager {
        if let Some(api_client) = &state.api_client {
            let auth_manager = auth_manager.clone();
            let api_client = api_client.clone();
            let server_url = api_client.build_url("");
            
            // Create auth token structure
            let auth_token = ferrex_core::user::AuthToken {
                access_token,
                refresh_token,
                expires_in: 900, // 15 minutes default
            };
            
            return Task::perform(
                async move {
                    // Set the token on the API client
                    api_client.set_token(Some(auth_token.clone())).await;
                    
                    // Now fetch the current user
                    let user: ferrex_core::user::User = match api_client.get("/api/users/me").await {
                        Ok(user) => user,
                        Err(e) => return Err(format!("Failed to get user: {}", e)),
                    };
                    
                    // Get user permissions
                    let permissions: ferrex_core::rbac::UserPermissions = match api_client.get("/api/users/me/permissions").await {
                        Ok(perms) => perms,
                        Err(e) => {
                            log::warn!("[Auth] Failed to get permissions, using default admin permissions: {}", e);
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
                    auth_manager.auth_state().authenticate(
                        user.clone(),
                        auth_token,
                        permissions.clone(),
                        server_url,
                    );
                    
                    // Save to keychain
                    if let Err(e) = auth_manager.save_current_auth().await {
                        log::warn!("[Auth] Failed to save auth after setup: {}", e);
                    }
                    
                    Ok((user, permissions))
                },
                |result: Result<(ferrex_core::user::User, ferrex_core::rbac::UserPermissions), String>| match result {
                    Ok((user, permissions)) => {
                        log::info!("[Auth] Retrieved admin user: {}", user.username);
                        auth::Message::LoginSuccess(user, permissions)
                    }
                    Err(e) => {
                        log::error!("[Auth] Failed to complete setup: {}", e);
                        auth::Message::SetupError(e)
                    }
                },
            );
        }
    }
    
    // Fallback - continue to user selection
    Task::done(auth::Message::LoadUsers)
}

pub fn handle_setup_error(state: &mut State, error: String) -> Task<auth::Message> {
    use crate::state::AuthenticationFlow;
    
    if let AuthenticationFlow::FirstRunSetup { 
        error: ref mut err, 
        loading: ref mut l,
        ..
    } = &mut state.auth_flow
    {
        *err = Some(error);
        *l = false;
    }
    
    Task::none()
}
