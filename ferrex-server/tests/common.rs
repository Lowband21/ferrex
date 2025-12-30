use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use axum::Router;
use ferrex_core::domain::scan::orchestration::{
    budget::InMemoryBudget,
    config::OrchestratorConfig,
    persistence::{PostgresCursorRepository, PostgresQueueService},
};
use ferrex_server::{
    application::auth::AuthApplicationFacade,
    handlers::users::UserService,
    infra::{
        app_context::AppContext,
        app_state::AppState,
        cache::{MovieBatchesCache, SeriesBundlesCache},
        config::{
            AuthConfig, CacheConfig, Config, ConfigMetadata, CorsConfig,
            DatabaseConfig, FfmpegConfig, HstsSettings, MediaConfig,
            ScannerConfig, SecurityConfig, ServerConfig,
        },
        orchestration::ScanOrchestrator,
        scan::scan_manager::ScanControlPlane,
        startup::StartupHooks,
        thumbnail_service::ThumbnailService,
        websocket::ConnectionManager,
    },
    routes::create_api_router,
};

use ferrex_core::domain::setup::SetupClaimService;
use ferrex_core::{
    application::unit_of_work::AppUnitOfWork,
    database::PostgresDatabase,
    domain::users::auth::{
        AuthCrypto,
        domain::{
            AuthenticationService, DeviceSessionRepository, DeviceTrustService,
            PinManagementService, UserAuthenticationRepository,
            repositories::{
                AuthEventRepository, AuthSessionRepository,
                DeviceChallengeRepository, RefreshTokenRepository,
            },
        },
        infrastructure::repositories::{
            PostgresAuthEventRepository, PostgresAuthSessionRepository,
            PostgresDeviceChallengeRepository, PostgresDeviceSessionRepository,
            PostgresRefreshTokenRepository, PostgresUserAuthRepository,
        },
    },
    infra::{image_service::ImageService, providers::TmdbApiProvider},
};
use sqlx::PgPool;
use tempfile::TempDir;
use tokio::sync::Mutex;

// Code is used by test modules, but not in this scope
#[allow(unused)]
#[derive(Debug)]
pub struct TestApp {
    pub router: Router<AppState>,
    pub state: AppState,
    _tempdir: TempDir,
}

#[allow(unused)]
impl TestApp {
    pub fn into_parts(self) -> (Router<AppState>, AppState, TempDir) {
        (self.router, self.state, self._tempdir)
    }
}

#[allow(unused)]
pub async fn build_test_app_with_hooks<H: StartupHooks>(
    pool: PgPool,
    hooks: &H,
) -> Result<TestApp> {
    // SAFETY: tests run in isolation and set the env var before any child threads read it.
    unsafe {
        std::env::set_var("FERREX_DISABLE_FFMPEG", "1");
        // Prevent host-level setup tokens from constraining the default claim flow path.
        std::env::remove_var("FERREX_SETUP_TOKEN");
    }

    let tempdir =
        tempfile::tempdir().context("failed to create temporary directory")?;
    let cache_root = tempdir.path().join("cache");
    let transcode_cache_dir = cache_root.join("transcode");
    let thumbnail_cache_dir = cache_root.join("thumbnails");
    let image_cache_dir = cache_root.join("images");

    std::fs::create_dir_all(&transcode_cache_dir)
        .context("failed to create transcode cache directory")?;
    std::fs::create_dir_all(&thumbnail_cache_dir)
        .context("failed to create thumbnail cache directory")?;
    std::fs::create_dir_all(&image_cache_dir)
        .context("failed to create image cache directory")?;

    let mut config = Config {
        server: ServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
        },
        database: DatabaseConfig { primary_url: None },
        redis: None,
        media: MediaConfig { root: None },
        cache: CacheConfig {
            root: cache_root.clone(),
            images: image_cache_dir.clone(),
            transcode: transcode_cache_dir.clone(),
            thumbnails: thumbnail_cache_dir.clone(),
        },
        ffmpeg: FfmpegConfig {
            ffmpeg_path: "ffmpeg".into(),
            ffprobe_path: "ffprobe".into(),
        },
        cors: CorsConfig {
            allowed_origins: vec![],
            allowed_methods: vec!["GET".into(), "POST".into()],
            allowed_headers: vec!["authorization".into()],
            allow_credentials: false,
        },
        security: SecurityConfig {
            enforce_https: false,
            trust_proxy_headers: false,
            hsts: HstsSettings {
                max_age: 31_536_000,
                include_subdomains: false,
                preload: false,
            },
        },
        dev_mode: true,
        auth: AuthConfig {
            password_pepper: "test-pepper".into(),
            token_key: "test-token-key".into(),
            setup_token: None,
        },
        scanner: ScannerConfig::default(),
        rate_limiter: None,
        metadata: ConfigMetadata::default(),
    };

    config
        .ensure_directories()
        .context("failed to prepare cache directories for test config")?;
    config
        .normalize_paths()
        .context("failed to canonicalize cache directories for test config")?;

    let postgres = Arc::new(PostgresDatabase::from_pool(pool.clone()));
    let unit_of_work = Arc::new(
        AppUnitOfWork::from_postgres(postgres.clone())
            .map_err(|err| anyhow!("failed to build unit of work: {err}"))?,
    );

    let image_service = Arc::new(ImageService::new(
        unit_of_work.media_files_read.clone(),
        unit_of_work.images.clone(),
        image_cache_dir,
    ));

    let thumbnail_service = Arc::new(
        ThumbnailService::new(
            cache_root.clone(),
            unit_of_work.media_files_read.clone(),
        )
        .context("failed to construct thumbnail service")?,
    );

    let tmdb_provider = Arc::new(TmdbApiProvider::new());

    let queue_service: Arc<PostgresQueueService> = Arc::new(
        PostgresQueueService::new(pool.clone())
            .await
            .map_err(|err| anyhow!("failed to create queue service: {err}"))?,
    );
    let cursor_repository =
        Arc::new(PostgresCursorRepository::new(pool.clone()));
    let orchestrator_config = OrchestratorConfig::default();
    let budget =
        Arc::new(InMemoryBudget::new(orchestrator_config.budget.clone()));
    let orchestrator = Arc::new(
        ScanOrchestrator::new(
            orchestrator_config,
            tmdb_provider.clone(),
            image_service.clone(),
            unit_of_work.clone(),
            queue_service.clone(),
            cursor_repository,
            budget,
        )
        .map_err(|err| {
            anyhow!("failed to initialise scan orchestrator: {err}")
        })?,
    );

    let scan_control = Arc::new(ScanControlPlane::with_quiescence_window(
        unit_of_work.clone(),
        orchestrator,
        Duration::from_secs(1),
    ));

    let auth_crypto = Arc::new(
        AuthCrypto::new("integration-test-pepper", "integration-test-hmac")
            .context("failed to initialise AuthCrypto")?,
    );

    let user_auth_repo: Arc<dyn UserAuthenticationRepository> =
        Arc::new(PostgresUserAuthRepository::new(pool.clone()));
    let device_repo: Arc<dyn DeviceSessionRepository> = Arc::new(
        PostgresDeviceSessionRepository::new(pool.clone(), auth_crypto.clone()),
    );
    let refresh_repo: Arc<dyn RefreshTokenRepository> =
        Arc::new(PostgresRefreshTokenRepository::new(pool.clone()));
    let session_repo: Arc<dyn AuthSessionRepository> =
        Arc::new(PostgresAuthSessionRepository::new(pool.clone()));
    let event_repo: Arc<dyn AuthEventRepository> =
        Arc::new(PostgresAuthEventRepository::new(pool.clone()));

    let challenge_repo: Arc<dyn DeviceChallengeRepository> =
        Arc::new(PostgresDeviceChallengeRepository::new(pool.clone()));

    let auth_service = Arc::new(
        AuthenticationService::new(
            user_auth_repo.clone(),
            device_repo.clone(),
            refresh_repo.clone(),
            session_repo.clone(),
            auth_crypto.clone(),
        )
        .with_event_repository(event_repo.clone())
        .with_challenge_repository(challenge_repo.clone()),
    );

    let device_trust_service = Arc::new(DeviceTrustService::new(
        user_auth_repo.clone(),
        device_repo.clone(),
        event_repo.clone(),
        session_repo.clone(),
        refresh_repo.clone(),
    ));

    let pin_service = Arc::new(PinManagementService::new(
        user_auth_repo.clone(),
        device_repo.clone(),
        event_repo.clone(),
        auth_crypto.clone(),
    ));

    let auth_facade = Arc::new(AuthApplicationFacade::new(
        auth_service.clone(),
        device_trust_service,
        pin_service,
        unit_of_work.clone(),
    ));

    let setup_claim_service = Arc::new(SetupClaimService::new(
        unit_of_work.setup_claims.clone(),
        auth_crypto.clone(),
    ));

    let config_arc = Arc::new(config);
    let websocket_manager = Arc::new(ConnectionManager::new());
    let admin_sessions = Arc::new(Mutex::new(HashMap::new()));
    let app_context = Arc::new(AppContext::new(
        Arc::clone(&config_arc),
        unit_of_work.clone(),
        postgres.clone(),
        scan_control.clone(),
        thumbnail_service.clone(),
        image_service.clone(),
        Arc::clone(&websocket_manager),
        Arc::clone(&auth_facade),
        auth_crypto.clone(),
        setup_claim_service.clone(),
        false,
        #[cfg(feature = "demo")]
        None,
    ));

    let state = AppState::new(
        app_context,
        admin_sessions,
        Arc::new(SeriesBundlesCache::new()),
        Arc::new(MovieBatchesCache::new()),
    );

    hooks
        .run(
            state.context_handle(),
            &state,
            #[cfg(feature = "demo")]
            None,
        )
        .await
        .context("startup hooks failed")?;

    UserService::new(&state)
        .ensure_admin_role_exists()
        .await
        .context("failed to seed RBAC defaults")?;

    let router = create_api_router(state.clone());

    Ok(TestApp {
        router,
        state,
        _tempdir: tempdir,
    })
}
