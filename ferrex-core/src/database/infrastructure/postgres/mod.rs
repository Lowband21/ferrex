//! PostgreSQL infrastructure adapters implementing the database ports.

pub mod repositories;

pub use repositories::folder_inventory::PostgresFolderInventoryRepository;
pub use repositories::images::PostgresImageRepository;
pub use repositories::indices::PostgresIndicesRepository;
pub use repositories::library::PostgresLibraryRepository;
pub use repositories::media::PostgresMediaRepository;
pub use repositories::media_references::PostgresMediaReferencesRepository;
pub use repositories::processing_status::PostgresProcessingStatusRepository;
pub use repositories::query::PostgresQueryRepository;
pub use repositories::rbac::PostgresRbacRepository;
pub use repositories::security_settings::PostgresSecuritySettingsRepository;
pub use repositories::setup_claims::PostgresSetupClaimsRepository;
pub use repositories::sync_sessions::PostgresSyncSessionsRepository;
pub use repositories::users::PostgresUsersRepository;
pub use repositories::watch_metrics::PostgresWatchMetricsRepository;
pub use repositories::watch_status::PostgresWatchStatusRepository;
