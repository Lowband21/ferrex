use std::{collections::HashMap, fmt, sync::Arc};

use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

use crate::application::auth::AuthApplicationFacade;
use crate::infra::app_context::AppContext;
use crate::infra::config::Config;
use crate::infra::scan::scan_manager::ScanControlPlane;
use crate::infra::websocket::ConnectionManager;
use crate::media::prep::thumbnail_service::ThumbnailService;
use ferrex_core::application::unit_of_work::AppUnitOfWork;
use ferrex_core::auth::{
    AuthCrypto,
    domain::{
        services::{
            AuthenticationService, DeviceTrustService, PinManagementService,
        },
        value_objects::SessionScope,
    },
};
use ferrex_core::database::PostgresDatabase;
use ferrex_core::database::ports::setup_claims::SetupClaimsRepository;
use ferrex_core::image_service::ImageService;
use ferrex_core::setup::SetupClaimService;

#[cfg(feature = "demo")]
use crate::demo::DemoCoordinator;

#[derive(Clone)]
pub struct AppState {
    context: Arc<AppContext>,
    /// Track admin sessions per device for PIN authentication eligibility
    pub admin_sessions: Arc<Mutex<HashMap<Uuid, AdminSessionInfo>>>,
}

impl fmt::Debug for AppState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct AdminSessionInfo {
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub authenticated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub session_token: String,
}

impl AdminSessionInfo {
    pub fn new(user_id: Uuid, device_id: Uuid, session_token: String) -> Self {
        let now = Utc::now();
        Self {
            user_id,
            device_id,
            authenticated_at: now,
            expires_at: now + Duration::hours(24),
            session_token,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.expires_at > Utc::now()
    }
}

impl AppState {
    pub fn new(
        context: Arc<AppContext>,
        admin_sessions: Arc<Mutex<HashMap<Uuid, AdminSessionInfo>>>,
    ) -> Self {
        Self {
            context,
            admin_sessions,
        }
    }

    pub fn context(&self) -> &AppContext {
        &self.context
    }

    pub fn context_handle(&self) -> Arc<AppContext> {
        Arc::clone(&self.context)
    }

    pub fn config(&self) -> &Config {
        self.context.config()
    }

    pub fn config_handle(&self) -> Arc<Config> {
        self.context.config_handle()
    }

    pub fn cache_enabled(&self) -> bool {
        self.context.cache_enabled()
    }

    pub fn unit_of_work(&self) -> Arc<AppUnitOfWork> {
        self.context.unit_of_work()
    }

    pub fn postgres(&self) -> Arc<PostgresDatabase> {
        self.context.postgres()
    }

    pub fn scan_control(&self) -> Arc<ScanControlPlane> {
        self.context.scan_control()
    }

    pub fn thumbnail_service(&self) -> Arc<ThumbnailService> {
        self.context.thumbnail_service()
    }

    pub fn image_service(&self) -> Arc<ImageService> {
        self.context.image_service()
    }

    pub fn websocket_manager(&self) -> Arc<ConnectionManager> {
        self.context.websocket_manager()
    }

    pub fn auth_facade(&self) -> Arc<AuthApplicationFacade> {
        self.context.auth_facade()
    }

    pub fn auth_crypto(&self) -> Arc<AuthCrypto> {
        self.context.auth_crypto()
    }

    pub fn setup_claim_service(
        &self,
    ) -> Arc<SetupClaimService<dyn SetupClaimsRepository>> {
        self.context.setup_claim_service()
    }

    #[cfg(feature = "demo")]
    pub fn demo(&self) -> Option<Arc<DemoCoordinator>> {
        self.context.demo()
    }

    pub fn auth_service(&self) -> Arc<AuthenticationService> {
        self.auth_facade().auth_service()
    }

    pub fn device_trust_service(&self) -> Arc<DeviceTrustService> {
        self.auth_facade().device_trust_service()
    }

    pub fn pin_management_service(&self) -> Arc<PinManagementService> {
        self.auth_facade().pin_management_service()
    }

    pub async fn is_admin_authenticated_on_device(
        &self,
        device_id: Uuid,
    ) -> bool {
        let admin_sessions = self.admin_sessions.lock().await;
        admin_sessions
            .get(&device_id)
            .map(|session| session.is_valid())
            .unwrap_or(false)
    }

    pub async fn register_admin_session(
        &self,
        user_id: Uuid,
        device_id: Uuid,
        session_token: String,
    ) -> Result<(), anyhow::Error> {
        // Verify the user exists
        let unit_of_work = self.unit_of_work();
        let user = unit_of_work
            .users
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow!("User not found"))?;

        // Defense-in-depth: verify the caller is an admin based on RBAC.
        // Admin middleware should already have enforced this at the route level,
        // but we re-check here to avoid accidental misuse.
        let perms = unit_of_work
            .rbac
            .get_user_permissions(user.id)
            .await
            .map_err(|err| anyhow!("Failed to load user permissions: {err}"))?;
        if !perms.has_role("admin")
            && !perms.has_all_permissions(&[
                "users:read",
                "users:create",
                "users:update",
                "users:delete",
                "users:manage_roles",
            ])
        {
            return Err(anyhow!(
                "Admin access required to register admin sessions"
            ));
        }

        // Validate the provided session token belongs to the admin and has full scope
        let validated_session = self
            .auth_service()
            .validate_session_token(&session_token)
            .await
            .map_err(|err| {
                anyhow!("Failed to validate admin session: {err}")
            })?;

        if validated_session.user_id != user_id {
            return Err(anyhow!(
                "Session token does not belong to requesting admin"
            ));
        }

        if validated_session.scope != SessionScope::Full {
            return Err(anyhow!(
                "Session scope '{}' is not permitted for admin PIN registration",
                validated_session.scope
            ));
        }

        if let Some(bound_device) = validated_session.device_session_id
            && bound_device != device_id
        {
            return Err(anyhow!(
                "Session is bound to a different device (expected {device_id}, got {bound_device})"
            ));
        }

        let mut admin_sessions = self.admin_sessions.lock().await;
        let session_info =
            AdminSessionInfo::new(user_id, device_id, session_token);
        admin_sessions.insert(device_id, session_info);

        info!(
            "Admin session registered for device {} by user {}",
            device_id, user_id
        );
        Ok(())
    }

    pub async fn remove_admin_session(&self, device_id: Uuid) {
        let mut admin_sessions = self.admin_sessions.lock().await;
        if admin_sessions.remove(&device_id).is_some() {
            info!("Admin session removed for device {}", device_id);
        }
    }

    pub async fn cleanup_expired_admin_sessions(&self) {
        let mut admin_sessions = self.admin_sessions.lock().await;
        let initial_count = admin_sessions.len();
        admin_sessions.retain(|_, session| session.is_valid());
        let removed_count = initial_count - admin_sessions.len();

        if removed_count > 0 {
            info!("Cleaned up {} expired admin sessions", removed_count);
        }
    }

    pub async fn get_admin_session(
        &self,
        device_id: Uuid,
    ) -> Option<AdminSessionInfo> {
        let admin_sessions = self.admin_sessions.lock().await;
        admin_sessions
            .get(&device_id)
            .filter(|session| session.is_valid())
            .cloned()
    }
}
