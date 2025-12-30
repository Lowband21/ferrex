use std::{collections::HashMap, sync::Arc};

use anyhow::Error as AnyhowError;
use ferrex_core::{
    application::unit_of_work::AppUnitOfWork,
    database::repository_ports::users::UsersRepository,
    domain::users::{
        auth::domain::{
            aggregates::DeviceSession,
            repositories::AuthSessionRecord,
            services::{
                AuthEventContext, AuthenticationError, AuthenticationService,
                DeviceTrustError, DeviceTrustService, PasswordChangeActor,
                PasswordChangeRequest, PinManagementError,
                PinManagementService, TokenBundle,
            },
            value_objects::{DeviceFingerprint, PinPolicy, RevocationReason},
        },
        user::{User, UserSession},
    },
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// Aggregates auth-focused domain services for HTTP handlers.
#[derive(Clone)]
pub struct AuthApplicationFacade {
    auth_service: Arc<AuthenticationService>,
    device_trust_service: Arc<DeviceTrustService>,
    pin_management_service: Arc<PinManagementService>,
    unit_of_work: Arc<AppUnitOfWork>,
}

impl std::fmt::Debug for AuthApplicationFacade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthApplicationFacade")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum AuthFacadeError {
    #[error(transparent)]
    Authentication(#[from] AuthenticationError),
    #[error(transparent)]
    DeviceTrust(#[from] DeviceTrustError),
    #[error(transparent)]
    PinManagement(#[from] PinManagementError),
    #[error("user not found")]
    UserNotFound,
    #[error(transparent)]
    Storage(#[from] AnyhowError),
}

impl AuthApplicationFacade {
    pub fn new(
        auth_service: Arc<AuthenticationService>,
        device_trust_service: Arc<DeviceTrustService>,
        pin_management_service: Arc<PinManagementService>,
        unit_of_work: Arc<AppUnitOfWork>,
    ) -> Self {
        Self {
            auth_service,
            device_trust_service,
            pin_management_service,
            unit_of_work,
        }
    }

    pub fn auth_service(&self) -> Arc<AuthenticationService> {
        self.auth_service.clone()
    }

    pub fn device_trust_service(&self) -> Arc<DeviceTrustService> {
        self.device_trust_service.clone()
    }

    pub fn pin_management_service(&self) -> Arc<PinManagementService> {
        self.pin_management_service.clone()
    }

    pub fn unit_of_work(&self) -> Arc<AppUnitOfWork> {
        self.unit_of_work.clone()
    }

    pub fn users_repository(&self) -> Arc<dyn UsersRepository> {
        self.unit_of_work.users.clone()
    }

    pub async fn get_pin_client_salt(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<u8>, AuthFacadeError> {
        let salt = self.auth_service.get_pin_client_salt(user_id).await?;
        Ok(salt)
    }

    pub async fn device_password_login(
        &self,
        username: &str,
        password: &str,
        fingerprint: DeviceFingerprint,
        device_name: String,
        context: AuthEventContext,
    ) -> Result<(TokenBundle, DeviceSession), AuthFacadeError> {
        let bundle = self
            .auth_service
            .authenticate_with_password(username, password)
            .await?;

        let session = self
            .device_trust_service
            .register_device(
                bundle.user_id,
                fingerprint,
                device_name,
                Some(context),
            )
            .await?;

        Ok((bundle, session))
    }

    pub async fn get_user_by_id(
        &self,
        user_id: Uuid,
    ) -> Result<User, AuthFacadeError> {
        self.unit_of_work
            .users
            .get_user_by_id(user_id)
            .await
            .map_err(|err| AuthFacadeError::Storage(err.into()))?
            .ok_or(AuthFacadeError::UserNotFound)
    }

    pub async fn list_user_devices(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<DeviceSession>, AuthFacadeError> {
        let sessions = self.device_trust_service.list_devices(user_id).await?;
        Ok(sessions)
    }

    pub async fn get_device_session(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
    ) -> Result<DeviceSession, AuthFacadeError> {
        let session = self
            .device_trust_service
            .get_device(user_id, fingerprint)
            .await?;
        Ok(session)
    }

    pub async fn change_password(
        &self,
        request: PasswordChangeRequest,
    ) -> Result<(), AuthFacadeError> {
        if let PasswordChangeActor::AdminInitiated { admin_user_id } =
            &request.actor
        {
            let is_admin = self
                .unit_of_work
                .rbac
                .user_has_role(*admin_user_id, "admin")
                .await
                .map_err(|err| AuthFacadeError::Storage(err.into()))?;

            if !is_admin {
                return Err(AuthFacadeError::Authentication(
                    AuthenticationError::InvalidCredentials,
                ));
            }
        }

        self.auth_service
            .change_password(request)
            .await
            .map_err(AuthFacadeError::from)
    }

    pub async fn get_device_by_id(
        &self,
        session_id: Uuid,
    ) -> Result<DeviceSession, AuthFacadeError> {
        let session = self
            .device_trust_service
            .get_device_by_session_id(session_id)
            .await?;
        Ok(session)
    }

    pub async fn device_user_listing(
        &self,
        fingerprint: &DeviceFingerprint,
    ) -> Result<(bool, Vec<User>, HashMap<Uuid, bool>), AuthFacadeError> {
        let known = self
            .device_trust_service
            .is_known_device(fingerprint)
            .await?;

        let users = self
            .unit_of_work
            .users
            .get_all_users()
            .await
            .map_err(|err| AuthFacadeError::Storage(err.into()))?;

        let pin_map = if known {
            self.device_trust_service
                .pin_status_by_device(fingerprint)
                .await?
                .into_iter()
                .map(|status| (status.user_id, status.has_pin))
                .collect()
        } else {
            HashMap::new()
        };

        Ok((known, users, pin_map))
    }

    pub async fn revoke_device(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        reason: Option<String>,
        context: Option<AuthEventContext>,
    ) -> Result<(), AuthFacadeError> {
        self.device_trust_service
            .revoke_device(user_id, fingerprint, reason, context)
            .await?;
        Ok(())
    }

    pub async fn set_device_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        new_pin: String,
        policy: &PinPolicy,
        context: Option<AuthEventContext>,
    ) -> Result<(), AuthFacadeError> {
        self.pin_management_service
            .set_pin(user_id, fingerprint, new_pin, policy, context)
            .await?;
        Ok(())
    }

    pub async fn list_user_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<UserSession>, AuthFacadeError> {
        let records = self.auth_service.list_sessions_for_user(user_id).await?;
        Ok(records.into_iter().map(Self::map_session_record).collect())
    }

    pub async fn revoke_user_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<(), AuthFacadeError> {
        let record = self
            .auth_service
            .find_session_by_id(session_id)
            .await?
            .ok_or(AuthenticationError::SessionExpired)?;

        if record.user_id != user_id {
            return Err(AuthFacadeError::Authentication(
                AuthenticationError::InvalidCredentials,
            ));
        }

        self.auth_service
            .revoke_session_by_id(session_id, RevocationReason::UserLogout)
            .await?;

        Ok(())
    }

    pub async fn revoke_all_user_sessions(
        &self,
        user_id: Uuid,
    ) -> Result<(), AuthFacadeError> {
        self.auth_service
            .revoke_all_sessions_for_user(user_id, RevocationReason::UserLogout)
            .await?;
        Ok(())
    }

    pub async fn rotate_device_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        current_pin: &str,
        new_pin: String,
        policy: &PinPolicy,
        max_attempts: u8,
        context: Option<AuthEventContext>,
    ) -> Result<(), AuthFacadeError> {
        self.pin_management_service
            .rotate_pin(
                user_id,
                fingerprint,
                current_pin,
                new_pin,
                policy,
                max_attempts,
                context,
            )
            .await?;
        Ok(())
    }

    pub async fn clear_device_pin(
        &self,
        user_id: Uuid,
        fingerprint: &DeviceFingerprint,
        current_pin: &str,
        max_attempts: u8,
        context: Option<AuthEventContext>,
    ) -> Result<(), AuthFacadeError> {
        self.pin_management_service
            .clear_pin(user_id, fingerprint, current_pin, max_attempts, context)
            .await?;
        Ok(())
    }
}

impl AuthApplicationFacade {
    fn map_session_record(record: AuthSessionRecord) -> UserSession {
        let device_name = record
            .metadata
            .get("device_name")
            .and_then(Value::as_str)
            .map(|s| s.to_string());

        UserSession {
            id: record.id,
            user_id: record.user_id,
            device_name,
            ip_address: record.ip_address,
            user_agent: record.user_agent,
            last_active: record.last_activity.timestamp_millis(),
            created_at: record.created_at.timestamp_millis(),
        }
    }
}
