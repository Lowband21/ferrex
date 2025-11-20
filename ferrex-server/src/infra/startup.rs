use std::{sync::Arc, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "demo")]
use tracing::info;
use tracing::warn;

use crate::{
    infra::{app_context::AppContext, app_state::AppState},
    users::UserService,
};

#[cfg(feature = "demo")]
use crate::demo::DemoCoordinator;

#[async_trait]
pub trait StartupHooks: Send + Sync {
    async fn run(
        &self,
        _context: Arc<AppContext>,
        state: &AppState,
        #[cfg(feature = "demo")] demo_coordinator: Option<Arc<DemoCoordinator>>,
    ) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct ProdStartupHooks;

#[async_trait]
impl StartupHooks for ProdStartupHooks {
    async fn run(
        &self,
        _context: Arc<AppContext>,
        state: &AppState,
        #[cfg(feature = "demo")] demo_coordinator: Option<Arc<DemoCoordinator>>,
    ) -> Result<()> {
        #[cfg(feature = "demo")]
        if let Some(coordinator) = demo_coordinator.as_ref() {
            coordinator.ensure_demo_user(state).await?;
            info!("Demo user ensured");
        }

        if let Err(err) =
            UserService::new(state).ensure_admin_role_exists().await
        {
            warn!(error = %err, "Failed to bootstrap RBAC defaults");
        }

        let cleanup_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                cleanup_state.cleanup_expired_admin_sessions().await;
            }
        });

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopStartupHooks;

#[async_trait]
impl StartupHooks for NoopStartupHooks {
    async fn run(
        &self,
        _context: Arc<AppContext>,
        _state: &AppState,
        #[cfg(feature = "demo")] _demo_coordinator: Option<
            Arc<DemoCoordinator>,
        >,
    ) -> Result<()> {
        Ok(())
    }
}
