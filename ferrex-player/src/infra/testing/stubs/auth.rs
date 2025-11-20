use crate::{
    domains::auth::{
        dto::UserListItemDto,
        manager::{AutoLoginScope, DeviceAuthStatus, PlayerAuthResult},
        storage::StoredAuth,
    },
    infra::{
        repository::{RepositoryError, RepositoryResult},
        services::auth::AuthService,
    },
};

use ferrex_core::{
    domain::users::auth::domain::value_objects::SessionScope,
    player_prelude::{AuthToken, Role, User, UserPermissions, UserPreferences},
};

use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StubAuthService {
    inner: Arc<RwLock<InnerAuthState>>,
}

#[derive(Debug, Clone)]
struct InnerAuthState {
    is_first_run: bool,
    users: Vec<User>,
    permissions: HashMap<Uuid, UserPermissions>,
    auto_login: HashMap<Uuid, bool>,
    current_user: Option<User>,
    current_permissions: Option<UserPermissions>,
    stored_auth: Option<StoredAuth>,
    device_status: HashMap<Uuid, DeviceAuthStatus>,
    device_id: Uuid,
    auth_token: Option<AuthToken>,
}

impl Default for StubAuthService {
    fn default() -> Self {
        Self::new()
    }
}

impl StubAuthService {
    pub fn new() -> Self {
        let admin_id = Uuid::now_v7();
        let demo_admin = User {
            id: admin_id,
            username: "demo_admin".into(),
            display_name: "Demo Admin".into(),
            avatar_url: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: Some(Utc::now()),
            is_active: true,
            email: Some("admin@example.com".into()),
            preferences: UserPreferences::default(),
        };

        let permissions = UserPermissions {
            user_id: admin_id,
            roles: vec![Role {
                id: Uuid::now_v7(),
                name: "admin".into(),
                description: Some("Administrator".into()),
                is_system: true,
                created_at: Utc::now().timestamp(),
            }],
            permissions: HashMap::from([
                ("system:admin".into(), true),
                ("user:create".into(), true),
            ]),
            permission_details: None,
        };

        let device_id = Uuid::now_v7();

        let mut device_status = HashMap::new();
        device_status.insert(
            admin_id,
            DeviceAuthStatus {
                device_registered: true,
                has_pin: false,
                remaining_attempts: Some(5),
            },
        );

        let inner = InnerAuthState {
            is_first_run: false,
            users: vec![demo_admin.clone()],
            permissions: HashMap::from([(admin_id, permissions.clone())]),
            auto_login: HashMap::new(),
            current_user: None,
            current_permissions: None,
            stored_auth: None,
            device_status,
            device_id,
            auth_token: None,
        };

        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub fn with_users(
        self,
        users: Vec<User>,
        permissions: HashMap<Uuid, UserPermissions>,
    ) -> Self {
        let mut guard = self.inner.write().expect("lock poisoned");
        guard.users = users.clone();
        guard.permissions = permissions;
        guard.device_status.clear();
        for user in users {
            guard.device_status.insert(
                user.id,
                DeviceAuthStatus {
                    device_registered: true,
                    has_pin: false,
                    remaining_attempts: Some(5),
                },
            );
        }
        drop(guard);
        self
    }

    pub fn set_first_run(&self, value: bool) {
        if let Ok(mut guard) = self.inner.write() {
            guard.is_first_run = value;
        }
    }

    fn user_list(&self) -> Vec<UserListItemDto> {
        let guard = self.inner.read().expect("lock poisoned");
        guard
            .users
            .iter()
            .map(|user| UserListItemDto {
                id: user.id,
                username: user.username.clone(),
                display_name: user.display_name.clone(),
                avatar_url: user.avatar_url.clone(),
                has_pin: guard
                    .device_status
                    .get(&user.id)
                    .map(|status| status.has_pin)
                    .unwrap_or(false),
                last_login: user.last_login,
            })
            .collect()
    }

    #[allow(unused)]
    fn permissions_for(&self, user_id: &Uuid) -> Option<UserPermissions> {
        self.inner
            .read()
            .expect("lock poisoned")
            .permissions
            .get(user_id)
            .cloned()
    }

    fn get_user(&self, username: &str) -> Option<User> {
        self.inner
            .read()
            .expect("lock poisoned")
            .users
            .iter()
            .find(|u| u.username == username)
            .cloned()
    }
}

impl InnerAuthState {
    fn auth_result(
        &self,
        user: &User,
    ) -> RepositoryResult<(User, UserPermissions)> {
        match self.permissions.get(&user.id) {
            Some(perms) => Ok((user.clone(), perms.clone())),
            None => Err(RepositoryError::QueryFailed(format!(
                "No permissions registered for user {}",
                user.username
            ))),
        }
    }

    fn player_auth_result(
        &self,
        user: &User,
    ) -> RepositoryResult<PlayerAuthResult> {
        let permissions =
            self.permissions.get(&user.id).cloned().ok_or_else(|| {
                RepositoryError::QueryFailed("Missing permissions".into())
            })?;
        let status = self.device_status.get(&user.id).cloned().unwrap_or(
            DeviceAuthStatus {
                device_registered: true,
                has_pin: false,
                remaining_attempts: Some(5),
            },
        );

        Ok(PlayerAuthResult {
            user: user.clone(),
            permissions,
            device_has_pin: status.has_pin,
        })
    }
}

#[async_trait]
impl AuthService for StubAuthService {
    async fn login(
        &self,
        username: String,
        _pin: String,
        _server_url: String,
    ) -> RepositoryResult<(User, UserPermissions)> {
        let user = self.get_user(&username).ok_or_else(|| {
            RepositoryError::NotFound {
                entity_type: "User".into(),
                id: username.clone(),
            }
        })?;

        let mut guard = self.inner.write().expect("lock poisoned");
        let (user, permissions) = guard.auth_result(&user)?;
        guard.current_user = Some(user.clone());
        guard.current_permissions = Some(permissions.clone());
        Ok((user, permissions))
    }

    async fn logout(&self) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.current_user = None;
            guard.current_permissions = None;
            guard.auth_token = None;
        }
        Ok(())
    }

    async fn refresh_access_token(&self) -> RepositoryResult<()> {
        Ok(())
    }

    async fn get_current_user(&self) -> RepositoryResult<Option<User>> {
        Ok(self
            .inner
            .read()
            .expect("lock poisoned")
            .current_user
            .clone())
    }

    async fn get_current_permissions(
        &self,
    ) -> RepositoryResult<Option<UserPermissions>> {
        Ok(self
            .inner
            .read()
            .expect("lock poisoned")
            .current_permissions
            .clone())
    }

    async fn is_first_run(&self) -> RepositoryResult<bool> {
        Ok(self.inner.read().expect("lock poisoned").is_first_run)
    }

    async fn get_all_users(&self) -> RepositoryResult<Vec<UserListItemDto>> {
        Ok(self.user_list())
    }

    async fn check_device_auth(
        &self,
        user_id: Uuid,
    ) -> RepositoryResult<DeviceAuthStatus> {
        let guard = self.inner.read().expect("lock poisoned");
        guard.device_status.get(&user_id).cloned().ok_or_else(|| {
            RepositoryError::NotFound {
                entity_type: "DeviceAuthStatus".into(),
                id: user_id.to_string(),
            }
        })
    }

    async fn set_device_pin(&self, pin: String) -> RepositoryResult<()> {
        let has_pin = !pin.trim().is_empty();
        if let Ok(mut guard) = self.inner.write() {
            let user_id = guard.current_user.as_ref().map(|user| user.id);
            if let Some(user_id) = user_id {
                guard.device_status.insert(
                    user_id,
                    DeviceAuthStatus {
                        device_registered: true,
                        has_pin,
                        remaining_attempts: Some(5),
                    },
                );
            }
        }
        Ok(())
    }

    async fn check_setup_status(&self) -> RepositoryResult<bool> {
        Ok(self.inner.read().expect("lock poisoned").is_first_run)
    }

    async fn enable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        Ok(())
    }

    async fn disable_admin_pin_unlock(&self) -> RepositoryResult<()> {
        Ok(())
    }

    async fn authenticate_device(
        &self,
        username: String,
        _password: String,
        remember_device: bool,
    ) -> RepositoryResult<PlayerAuthResult> {
        let user = self.get_user(&username).ok_or_else(|| {
            RepositoryError::NotFound {
                entity_type: "User".into(),
                id: username.clone(),
            }
        })?;

        let result = self
            .inner
            .read()
            .expect("lock poisoned")
            .player_auth_result(&user)?;

        if let Ok(mut guard) = self.inner.write() {
            guard.current_user = Some(result.user.clone());
            guard.current_permissions = Some(result.permissions.clone());
            guard.auth_token = Some(AuthToken {
                access_token: format!("token-{}", username),
                refresh_token: "refresh-token".into(),
                expires_in: 3600,
                session_id: Some(Uuid::now_v7()),
                device_session_id: Some(guard.device_id),
                user_id: Some(result.user.id),
                scope: SessionScope::Full,
            });
            guard.auto_login.insert(result.user.id, remember_device);
        }

        Ok(result)
    }

    async fn authenticate_pin(
        &self,
        user_id: Uuid,
        _pin: String,
    ) -> RepositoryResult<PlayerAuthResult> {
        let guard = self.inner.read().expect("lock poisoned");
        let user = guard
            .users
            .iter()
            .find(|u| u.id == user_id)
            .cloned()
            .ok_or_else(|| RepositoryError::NotFound {
                entity_type: "User".into(),
                id: user_id.to_string(),
            })?;

        guard.player_auth_result(&user)
    }

    async fn load_from_keychain(&self) -> RepositoryResult<Option<StoredAuth>> {
        Ok(self
            .inner
            .read()
            .expect("lock poisoned")
            .stored_auth
            .clone())
    }

    async fn apply_stored_auth(
        &self,
        stored_auth: StoredAuth,
    ) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.current_user = Some(stored_auth.user.clone());
            guard.current_permissions = stored_auth.permissions.clone();
            guard.auth_token = Some(stored_auth.token.clone());
            guard.stored_auth = Some(stored_auth);
        }
        Ok(())
    }

    async fn is_auto_login_enabled(
        &self,
        user_id: &Uuid,
    ) -> RepositoryResult<bool> {
        Ok(*self
            .inner
            .read()
            .expect("lock poisoned")
            .auto_login
            .get(user_id)
            .unwrap_or(&false))
    }

    async fn validate_session(
        &self,
    ) -> RepositoryResult<(User, UserPermissions)> {
        let guard = self.inner.read().expect("lock poisoned");
        match (
            guard.current_user.clone(),
            guard.current_permissions.clone(),
        ) {
            (Some(user), Some(perms)) => Ok((user, perms)),
            _ => Err(RepositoryError::QueryFailed(
                "No active session available".into(),
            )),
        }
    }

    async fn is_current_user_auto_login_enabled(
        &self,
    ) -> RepositoryResult<bool> {
        let guard = self.inner.read().expect("lock poisoned");
        if let Some(user) = guard.current_user.as_ref() {
            Ok(*guard.auto_login.get(&user.id).unwrap_or(&false))
        } else {
            Ok(false)
        }
    }

    async fn current_device_id(&self) -> RepositoryResult<Uuid> {
        Ok(self.inner.read().expect("lock poisoned").device_id)
    }

    async fn authenticate(
        &self,
        user: User,
        token: AuthToken,
        permissions: UserPermissions,
        _server_url: String,
    ) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            guard.current_user = Some(user.clone());
            guard.current_permissions = Some(permissions.clone());
            guard.auth_token = Some(token.clone());
            guard.permissions.insert(user.id, permissions);
        }
        Ok(())
    }

    async fn save_current_auth(&self) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write()
            && let (Some(user), Some(token)) =
                (guard.current_user.clone(), guard.auth_token.clone())
        {
            let stored = StoredAuth {
                token,
                user,
                server_url: "https://localhost:3000".into(),
                permissions: guard.current_permissions.clone(),
                stored_at: Utc::now(),
                device_trust_expires_at: Some(Utc::now() + Duration::days(30)),
                refresh_token: Some("refresh-token".into()),
            };
            guard.stored_auth = Some(stored);
        }
        Ok(())
    }

    async fn set_auto_login_scope(
        &self,
        enabled: bool,
        scope: AutoLoginScope,
    ) -> RepositoryResult<()> {
        if let Ok(mut guard) = self.inner.write() {
            match scope {
                AutoLoginScope::DeviceOnly | AutoLoginScope::UserDefault => {
                    let user_id =
                        guard.current_user.as_ref().map(|user| user.id);
                    if let Some(user_id) = user_id {
                        guard.auto_login.insert(user_id, enabled);
                    }
                }
            }
        }
        Ok(())
    }
}
