use std::{collections::HashMap, fmt, sync::Arc};

use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

#[cfg(feature = "demo")]
use crate::demo::DemoCoordinator;
use crate::infra::config::Config;
use crate::infra::scan::scan_manager::ScanControlPlane;
use crate::infra::websocket::ConnectionManager;
use crate::media::prep::thumbnail_service::ThumbnailService;
use ferrex_core::ImageService;
use ferrex_core::application::unit_of_work::AppUnitOfWork;
use ferrex_core::auth::{AuthCrypto, domain::services::AuthenticationService};
use ferrex_core::database::PostgresDatabase;

#[derive(Clone)]
pub struct AppState {
    pub unit_of_work: Arc<AppUnitOfWork>,
    pub postgres: Arc<PostgresDatabase>,
    pub cache_enabled: bool,
    pub config: Arc<Config>,
    pub scan_control: Arc<ScanControlPlane>,
    //pub transcoding_service: Arc<TranscodingService>,
    pub thumbnail_service: Arc<ThumbnailService>,
    pub image_service: Arc<ImageService>,
    pub websocket_manager: Arc<ConnectionManager>,
    pub auth_service: Arc<AuthenticationService>,
    pub auth_crypto: Arc<AuthCrypto>,
    /// Track admin sessions per device for PIN authentication eligibility
    pub admin_sessions: Arc<Mutex<HashMap<Uuid, AdminSessionInfo>>>,
    #[cfg(feature = "demo")]
    pub demo: Option<Arc<DemoCoordinator>>,
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
    pub async fn is_admin_authenticated_on_device(&self, device_id: Uuid) -> bool {
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
        // Verify the user is actually an admin
        let user = self
            .unit_of_work
            .users
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow!("User not found"))?;

        // TODO: Add proper admin role checking once role system is implemented
        // For now, assume the caller has already verified admin status

        let mut admin_sessions = self.admin_sessions.lock().await;
        let session_info = AdminSessionInfo::new(user_id, device_id, session_token);
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

    pub async fn get_admin_session(&self, device_id: Uuid) -> Option<AdminSessionInfo> {
        let admin_sessions = self.admin_sessions.lock().await;
        admin_sessions
            .get(&device_id)
            .filter(|session| session.is_valid())
            .cloned()
    }
}
