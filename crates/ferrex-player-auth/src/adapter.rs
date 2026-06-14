//! AuthManager adapter that implements AuthService trait
//!
//! Wraps the existing AuthManager to provide a trait-based interface

use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::manager::AuthManager;
use crate::service::AuthService;
use crate::storage::StoredAuth;
use ferrex_core::player_prelude::{AuthToken, User, UserPermissions};
use ferrex_player_api::auth::{
    AutoLoginScope, DeviceAuthStatus, PlayerAuthResult,
};
use ferrex_player_foundation::repository::{RepositoryError, RepositoryResult};
use log::{info, warn};

/// Adapter that implements AuthService using the existing AuthManager
#[derive(Debug)]
pub struct AuthManagerAdapter {
    manager: Arc<AuthManager>,
}

impl AuthManagerAdapter {
    pub fn new(manager: Arc<AuthManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl AuthService for AuthManagerAdapter {
    async fn login(
        &self,
        username: String,
        pin: String,
        server_url: String,
    ) -> RepositoryResult<(User, UserPermissions)> {
        self.manager
            .login(username, pin, server_url)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!("Login failed: {}", e))
            })
    }

    async fn load_from_keychain(&self) -> RepositoryResult<Option<StoredAuth>> {
        let fingerprint =
            self.manager.device_auth_cache_fingerprint().await.map_err(
                |e| {
                    RepositoryError::QueryFailed(format!(
                        "Device identity load failed: {}",
                        e
                    ))
                },
            )?;

        match self.manager.auth_storage().load_auth(&fingerprint).await {
            Ok(result) => Ok(result),
            Err(e) => {
                let msg = e.to_string();
                // Common case: device fingerprint changed or crypto key invalid => decryption failure.
                // Rationale: do not block startup; clear the stale cache and proceed without stored auth.
                if msg.contains("Decryption failed") {
                    warn!(
                        "[Auth] Keychain decryption failed; clearing cached auth. Hint: device fingerprint likely changed."
                    );
                    if let Err(clear_err) = self.manager.clear_keychain().await
                    {
                        warn!(
                            "[Auth] Failed to clear invalid keychain cache: {}",
                            clear_err
                        );
                    } else {
                        info!("[Auth] Cleared invalid keychain cache");
                    }
                    Ok(None)
                } else {
                    Err(RepositoryError::QueryFailed(format!(
                        "Load from keychain failed: {}",
                        msg
                    )))
                }
            }
        }
    }

    async fn logout(&self) -> RepositoryResult<()> {
        self.manager.logout().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Logout failed: {}", e))
        })
    }

    async fn refresh_access_token(&self) -> RepositoryResult<()> {
        self.manager.refresh_access_token().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Token refresh failed: {}", e))
        })
    }

    async fn get_current_user(&self) -> RepositoryResult<Option<User>> {
        Ok(self.manager.get_current_user().await)
    }

    async fn get_current_permissions(
        &self,
    ) -> RepositoryResult<Option<UserPermissions>> {
        Ok(self.manager.get_current_permissions().await)
    }

    async fn is_first_run(&self) -> RepositoryResult<bool> {
        // Consider first run if no users are returned by the public/users list
        let is_empty = self
            .manager
            .get_all_users()
            .await
            .map(|v| v.is_empty())
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "First run check failed: {}",
                    e
                ))
            })?;
        Ok(is_empty)
    }

    async fn get_all_users(
        &self,
    ) -> RepositoryResult<Vec<crate::dto::UserListItemDto>> {
        self.manager.get_all_users().await.map_err(|e| {
            RepositoryError::QueryFailed(format!("Get all users failed: {}", e))
        })
    }

    async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> RepositoryResult<DeviceAuthStatus> {
        self.manager.check_device_auth(user_id).await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Check device auth failed: {}",
                e
            ))
        })
    }

    async fn change_password(
        &self,
        current_password: String,
        new_password: String,
    ) -> RepositoryResult<()> {
        self.manager
            .change_password(current_password, new_password)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "Change password failed: {}",
                    e
                ))
            })
    }

    async fn set_device_pin(&self, pin: String) -> RepositoryResult<()> {
        self.manager.set_device_pin(pin).await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Set device PIN failed: {}",
                e
            ))
        })
    }

    async fn change_device_pin(
        &self,
        current_pin: String,
        new_pin: String,
    ) -> RepositoryResult<()> {
        self.manager
            .change_device_pin(current_pin, new_pin)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "Change device PIN failed: {}",
                    e
                ))
            })
    }

    async fn remove_device_pin(
        &self,
        current_pin: String,
    ) -> RepositoryResult<()> {
        self.manager
            .remove_device_pin(current_pin)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "Remove device PIN failed: {}",
                    e
                ))
            })
    }

    async fn reset_local_auth_state(&self) -> RepositoryResult<()> {
        self.manager.reset_local_auth_state().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Reset local auth state failed: {}",
                e
            ))
        })
    }

    async fn check_setup_status(&self) -> RepositoryResult<bool> {
        self.manager.check_setup_status().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Check setup status failed: {}",
                e
            ))
        })
    }

    async fn enable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        self.manager.enable_admin_pin_unlock().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Enable admin PIN unlock failed: {}",
                e
            ))
        })
    }

    async fn disable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        self.manager.disable_admin_pin_unlock().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Disable admin PIN unlock failed: {}",
                e
            ))
        })
    }

    async fn authenticate_device(
        &self,
        username: String,
        password: String,
        remember_device: bool,
    ) -> RepositoryResult<PlayerAuthResult> {
        self.manager
            .authenticate_device(username, password, remember_device)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "Device authentication failed: {}",
                    e
                ))
            })
    }

    async fn authenticate_pin(
        &self,
        user_id: Uuid,
        pin: String,
    ) -> RepositoryResult<PlayerAuthResult> {
        self.manager
            .authenticate_pin(user_id, pin)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "PIN authentication failed: {}",
                    e
                ))
            })
    }

    async fn apply_stored_auth(
        &self,
        stored_auth: StoredAuth,
    ) -> RepositoryResult<()> {
        match self.manager.apply_stored_auth(stored_auth).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // If credentials are invalid against the current server, also clear stale user cache
                // so the UI doesn't present users from a previous server instance.
                let msg = e.to_string();
                if msg.contains("Invalid credentials")
                    && let Err(clear_err) =
                        self.manager.clear_current_server_user_cache().await
                {
                    warn!(
                        "[Auth] Failed to clear stale user cache: {}",
                        clear_err
                    );
                }
                Err(RepositoryError::QueryFailed(format!(
                    "Apply stored auth failed: {}",
                    e
                )))
            }
        }
    }

    async fn is_auto_login_enabled(
        &self,
        user_id: &Uuid,
    ) -> RepositoryResult<bool> {
        Ok(self
            .manager
            .auth_storage()
            .is_auto_login_enabled(user_id)
            .await
            .unwrap_or(false))
    }

    async fn is_current_user_auto_login_enabled(
        &self,
    ) -> RepositoryResult<bool> {
        Ok(self.manager.is_auto_login_enabled().await)
    }

    async fn current_device_id(&self) -> RepositoryResult<Uuid> {
        self.manager.current_device_id().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Get current device id failed: {}",
                e
            ))
        })
    }

    async fn current_device_session_id(
        &self,
    ) -> RepositoryResult<Option<Uuid>> {
        self.manager.current_device_session_id().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Get current device session id failed: {}",
                e
            ))
        })
    }

    async fn validate_session(
        &self,
    ) -> RepositoryResult<(User, UserPermissions)> {
        self.manager.validate_session().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Validate session failed: {}",
                e
            ))
        })
    }

    async fn authenticate(
        &self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        server_url: String,
    ) -> RepositoryResult<()> {
        self.manager.auth_state().authenticate(
            user,
            token,
            permissions,
            server_url,
        );
        Ok(())
    }

    async fn save_current_auth(&self) -> RepositoryResult<()> {
        self.manager.save_current_auth().await.map_err(|e| {
            RepositoryError::QueryFailed(format!(
                "Save current auth failed: {}",
                e
            ))
        })
    }

    async fn set_auto_login_scope(
        &self,
        enabled: bool,
        scope: AutoLoginScope,
    ) -> RepositoryResult<()> {
        self.manager
            .set_auto_login_scope(enabled, scope)
            .await
            .map_err(|e| {
                RepositoryError::QueryFailed(format!(
                    "Set auto login failed: {}",
                    e
                ))
            })
    }
}
