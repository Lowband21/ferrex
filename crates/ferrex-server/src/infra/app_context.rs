use std::{fmt, sync::Arc};

#[cfg(feature = "demo")]
use crate::demo::DemoCoordinator;
use crate::infra::thumbnail_service::ThumbnailService;
use crate::{
    application::auth::AuthApplicationFacade,
    infra::{
        config::Config, scan::scan_manager::ScanControlPlane,
        websocket::ConnectionManager,
    },
};
use ferrex_core::domain::setup::SetupClaimService;
use ferrex_core::{
    application::unit_of_work::AppUnitOfWork,
    database::{
        PostgresDatabase, repository_ports::setup_claims::SetupClaimsRepository,
    },
    domain::users::auth::AuthCrypto,
    infra::media::image_service::ImageService,
};

#[derive(Clone)]
pub struct AppContext {
    config: Arc<Config>,
    unit_of_work: Arc<AppUnitOfWork>,
    postgres: Arc<PostgresDatabase>,
    scan_control: Arc<ScanControlPlane>,
    thumbnail_service: Arc<ThumbnailService>,
    image_service: Arc<ImageService>,
    websocket_manager: Arc<ConnectionManager>,
    auth_facade: Arc<AuthApplicationFacade>,
    auth_crypto: Arc<AuthCrypto>,
    setup_claim_service: Arc<SetupClaimService<dyn SetupClaimsRepository>>,
    cache_enabled: bool,
    #[cfg(feature = "demo")]
    demo: Option<Arc<DemoCoordinator>>,
}

impl fmt::Debug for AppContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppContext").finish_non_exhaustive()
    }
}

impl AppContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        unit_of_work: Arc<AppUnitOfWork>,
        postgres: Arc<PostgresDatabase>,
        scan_control: Arc<ScanControlPlane>,
        thumbnail_service: Arc<ThumbnailService>,
        image_service: Arc<ImageService>,
        websocket_manager: Arc<ConnectionManager>,
        auth_facade: Arc<AuthApplicationFacade>,
        auth_crypto: Arc<AuthCrypto>,
        setup_claim_service: Arc<SetupClaimService<dyn SetupClaimsRepository>>,
        cache_enabled: bool,
        #[cfg(feature = "demo")] demo: Option<Arc<DemoCoordinator>>,
    ) -> Self {
        Self {
            config,
            unit_of_work,
            postgres,
            scan_control,
            thumbnail_service,
            image_service,
            websocket_manager,
            auth_facade,
            auth_crypto,
            setup_claim_service,
            cache_enabled,
            #[cfg(feature = "demo")]
            demo,
        }
    }

    pub fn config(&self) -> &Config {
        self.config.as_ref()
    }

    pub fn config_handle(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }

    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }

    pub fn unit_of_work(&self) -> Arc<AppUnitOfWork> {
        Arc::clone(&self.unit_of_work)
    }

    pub fn postgres(&self) -> Arc<PostgresDatabase> {
        Arc::clone(&self.postgres)
    }

    pub fn scan_control(&self) -> Arc<ScanControlPlane> {
        Arc::clone(&self.scan_control)
    }

    pub fn thumbnail_service(&self) -> Arc<ThumbnailService> {
        Arc::clone(&self.thumbnail_service)
    }

    pub fn image_service(&self) -> Arc<ImageService> {
        Arc::clone(&self.image_service)
    }

    pub fn websocket_manager(&self) -> Arc<ConnectionManager> {
        Arc::clone(&self.websocket_manager)
    }

    pub fn auth_facade(&self) -> Arc<AuthApplicationFacade> {
        Arc::clone(&self.auth_facade)
    }

    pub fn auth_crypto(&self) -> Arc<AuthCrypto> {
        Arc::clone(&self.auth_crypto)
    }

    pub fn setup_claim_service(
        &self,
    ) -> Arc<SetupClaimService<dyn SetupClaimsRepository>> {
        Arc::clone(&self.setup_claim_service)
    }

    #[cfg(feature = "demo")]
    pub fn demo(&self) -> Option<Arc<DemoCoordinator>> {
        self.demo.clone()
    }
}
