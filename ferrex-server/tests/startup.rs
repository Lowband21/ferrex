use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use axum::Router;
use axum_test::TestServer;
use ferrex_core::api::routes::v1;
#[cfg(feature = "demo")]
use ferrex_server::demo::DemoCoordinator;
use ferrex_server::infra::{
    app_context::AppContext, app_state::AppState, startup::StartupHooks,
};
use sqlx::PgPool;
use std::net::SocketAddr;

mod common;
use common::build_test_app_with_hooks;

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
    let (router, state, tempdir) = app.into_parts();

    assert!(
        tempdir.path().join("cache").exists(),
        "test app should create cache directory structure"
    );

    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow!(err.to_string()))?;

    // A minimal smoke-check that the router is fully wired and ready to serve requests.
    let response = server.get(v1::setup::STATUS).await;
    response.assert_status_ok();

    assert!(flag.load(Ordering::SeqCst));
    Ok(())
}
