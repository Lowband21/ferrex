//! Authentication state machine for managing auth flow

use super::{AuthError, DeviceCheckResult, DeviceRegistration};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Authentication state representing the current stage of auth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthState {
    /// No authentication in progress
    Unauthenticated,

    /// User has been selected, need to check device trust
    UserSelected { user_id: Uuid },

    /// Device check complete, awaiting password
    AwaitingPassword { user_id: Uuid, device_id: Uuid },

    /// Device is trusted, awaiting PIN
    AwaitingPin {
        user_id: Uuid,
        device_registration: DeviceRegistration,
    },

    /// Successfully authenticated
    Authenticated {
        user_id: Uuid,
        session_token: String,
        device_id: Uuid,
    },

    /// Setting up PIN after password auth
    SettingUpPin {
        user_id: Uuid,
        session_token: String,
        device_id: Uuid,
    },
}

/// Events that trigger state transitions
#[derive(Debug, Clone)]
pub enum AuthEvent {
    /// User selected from list
    UserSelected(Uuid),

    /// Device check completed
    DeviceCheckComplete(DeviceCheckResult),

    /// Password authentication successful
    PasswordAuthSuccess {
        session_token: String,
        device_id: Uuid,
        requires_pin_setup: bool,
    },

    /// PIN authentication successful
    PinAuthSuccess { session_token: String },

    /// Authentication failed
    AuthFailed(AuthError),

    /// User cancelled authentication
    Cancelled,

    /// PIN setup completed
    PinSetupComplete,

    /// User skipped PIN setup
    PinSetupSkipped,
}

/// Result of a state transition
#[derive(Debug, Clone)]
pub enum TransitionResult {
    /// Transition successful, new state applied
    Success,

    /// Transition not valid from current state
    InvalidTransition { from_state: String, event: String },

    /// Authentication completed
    Authenticated {
        user_id: Uuid,
        session_token: String,
        device_id: Uuid,
    },

    /// Authentication failed
    Failed(AuthError),
}

impl AuthState {
    /// Attempt to transition to a new state based on an event
    pub fn transition(&mut self, event: AuthEvent) -> TransitionResult {
        let (new_state, result) = match (&self, &event) {
            // From Unauthenticated
            (AuthState::Unauthenticated, AuthEvent::UserSelected(user_id)) => {
                let new_state = AuthState::UserSelected { user_id: *user_id };
                (new_state, TransitionResult::Success)
            }

            // From UserSelected
            (AuthState::UserSelected { user_id }, AuthEvent::DeviceCheckComplete(check_result)) => {
                match check_result {
                    DeviceCheckResult::Trusted(reg) if reg.requires_pin() => {
                        let new_state = AuthState::AwaitingPin {
                            user_id: *user_id,
                            device_registration: reg.clone(),
                        };
                        (new_state, TransitionResult::Success)
                    }
                    _ => {
                        let device_id = Uuid::now_v7();
                        let new_state = AuthState::AwaitingPassword {
                            user_id: *user_id,
                            device_id,
                        };
                        (new_state, TransitionResult::Success)
                    }
                }
            }

            // From AwaitingPassword
            (
                AuthState::AwaitingPassword {
                    user_id,
                    device_id: _device_id,
                },
                AuthEvent::PasswordAuthSuccess {
                    session_token,
                    device_id: auth_device_id,
                    requires_pin_setup,
                },
            ) => {
                if *requires_pin_setup {
                    let new_state = AuthState::SettingUpPin {
                        user_id: *user_id,
                        session_token: session_token.clone(),
                        device_id: *auth_device_id,
                    };
                    (new_state, TransitionResult::Success)
                } else {
                    let new_state = AuthState::Authenticated {
                        user_id: *user_id,
                        session_token: session_token.clone(),
                        device_id: *auth_device_id,
                    };
                    let result = TransitionResult::Authenticated {
                        user_id: *user_id,
                        session_token: session_token.clone(),
                        device_id: *auth_device_id,
                    };
                    (new_state, result)
                }
            }

            // From AwaitingPin
            (
                AuthState::AwaitingPin {
                    user_id,
                    device_registration,
                },
                AuthEvent::PinAuthSuccess { session_token },
            ) => {
                let new_state = AuthState::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: device_registration.device_id,
                };
                let result = TransitionResult::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: device_registration.device_id,
                };
                (new_state, result)
            }

            // From SettingUpPin
            (
                AuthState::SettingUpPin {
                    user_id,
                    session_token,
                    device_id,
                },
                AuthEvent::PinSetupComplete,
            ) => {
                let new_state = AuthState::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: *device_id,
                };
                let result = TransitionResult::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: *device_id,
                };
                (new_state, result)
            }

            (
                AuthState::SettingUpPin {
                    user_id,
                    session_token,
                    device_id,
                },
                AuthEvent::PinSetupSkipped,
            ) => {
                let new_state = AuthState::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: *device_id,
                };
                let result = TransitionResult::Authenticated {
                    user_id: *user_id,
                    session_token: session_token.clone(),
                    device_id: *device_id,
                };
                (new_state, result)
            }

            // Handle auth failures from any state
            (_, AuthEvent::AuthFailed(error)) => {
                let result = TransitionResult::Failed(error.clone());
                (AuthState::Unauthenticated, result)
            }

            // Handle cancellation from any state
            (_, AuthEvent::Cancelled) => (AuthState::Unauthenticated, TransitionResult::Success),

            // Invalid transition
            _ => {
                let result = TransitionResult::InvalidTransition {
                    from_state: format!("{:?}", self),
                    event: format!("{:?}", event),
                };
                return result;
            }
        };

        *self = new_state;
        result
    }

    /// Get a human-readable description of the current state
    pub fn description(&self) -> &'static str {
        match self {
            AuthState::Unauthenticated => "Not authenticated",
            AuthState::UserSelected { .. } => "User selected, checking device",
            AuthState::AwaitingPassword { .. } => "Enter password to continue",
            AuthState::AwaitingPin { .. } => "Enter PIN to continue",
            AuthState::Authenticated { .. } => "Successfully authenticated",
            AuthState::SettingUpPin { .. } => "Set up a PIN for quick access",
        }
    }

    /// Check if authentication is complete
    pub fn is_authenticated(&self) -> bool {
        matches!(self, AuthState::Authenticated { .. })
    }

    /// Get the current user ID if available
    pub fn user_id(&self) -> Option<Uuid> {
        match self {
            AuthState::Unauthenticated => None,
            AuthState::UserSelected { user_id }
            | AuthState::AwaitingPassword { user_id, .. }
            | AuthState::AwaitingPin { user_id, .. }
            | AuthState::Authenticated { user_id, .. }
            | AuthState::SettingUpPin { user_id, .. } => Some(*user_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_password_flow() {
        let mut state = AuthState::Unauthenticated;
        let user_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();

        // Select user
        // TODO: Replace with proper domain transition
        // let result = domains::ui::transition(AuthEvent::UserSelected(user_id));
        let result = state.transition(AuthEvent::UserSelected(user_id));
        assert!(matches!(result, TransitionResult::Success));
        assert!(matches!(state, AuthState::UserSelected { .. }));

        // Device not trusted, need password
        let result = state.transition(AuthEvent::DeviceCheckComplete(
            DeviceCheckResult::NotRegistered,
        ));
        assert!(matches!(result, TransitionResult::Success));
        assert!(matches!(state, AuthState::AwaitingPassword { .. }));

        // Password auth success
        let result = state.transition(AuthEvent::PasswordAuthSuccess {
            session_token: "token123".to_string(),
            device_id,
            requires_pin_setup: true,
        });
        assert!(matches!(result, TransitionResult::Success));
        assert!(matches!(state, AuthState::SettingUpPin { .. }));

        // Complete PIN setup
        let result = state.transition(AuthEvent::PinSetupComplete);
        assert!(matches!(result, TransitionResult::Authenticated { .. }));
        assert!(state.is_authenticated());
    }

    #[test]
    fn test_pin_flow() {
        let mut state = AuthState::Unauthenticated;
        let user_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();

        // Create a device registration with PIN
        let device_reg = DeviceRegistration {
            id: Uuid::now_v7(),
            user_id,
            device_id,
            device_name: "Test Device".to_string(),
            platform: crate::auth::device::Platform::Linux,
            app_version: "1.0.0".to_string(),
            pin_configured: true,
            registered_at: chrono::Utc::now(),
            last_used_at: chrono::Utc::now(),
            expires_at: None,
            revoked: false,
            revoked_by: None,
            revoked_at: None,
        };

        // Select user
        state.transition(AuthEvent::UserSelected(user_id));

        // Device is trusted with PIN
        let result = state.transition(AuthEvent::DeviceCheckComplete(DeviceCheckResult::Trusted(
            device_reg,
        )));
        assert!(matches!(result, TransitionResult::Success));
        assert!(matches!(state, AuthState::AwaitingPin { .. }));

        // PIN auth success
        let result = state.transition(AuthEvent::PinAuthSuccess {
            session_token: "token123".to_string(),
        });
        assert!(matches!(result, TransitionResult::Authenticated { .. }));
        assert!(state.is_authenticated());
    }
}
