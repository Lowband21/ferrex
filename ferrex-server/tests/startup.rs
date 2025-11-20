use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "demo")]
use ferrex_server::demo::DemoCoordinator;
use ferrex_server::infra::{
    app_context::AppContext, app_state::AppState, startup::StartupHooks,
};
use sqlx::PgPool;

#[path = "support/mod.rs"]
mod support;

use support::build_test_app_with_hooks;

struct RecordingHooks {
    called: Arc<AtomicBool>,
}

#[async_trait]
impl StartupHooks for RecordingHooks {
    async fn run(
        &self,
        _context: Arc<AppContext>,
        _state: &AppState,
        #[cfg(feature = "demo")] _demo_coordinator: Option<
            Arc<DemoCoordinator>,
        >,
    ) -> Result<()> {
        self.called.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn build_test_app_invokes_custom_startup_hooks(
    pool: PgPool,
) -> Result<()> {
    let flag = Arc::new(AtomicBool::new(false));
    let hooks = RecordingHooks {
        called: Arc::clone(&flag),
    };

    let app = build_test_app_with_hooks(pool, &hooks).await?;
    // keep the test app in scope to ensure hooks completed before assertion
    drop(app);

    assert!(flag.load(Ordering::SeqCst));
    Ok(())
}
