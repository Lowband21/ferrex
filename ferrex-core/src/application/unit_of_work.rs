use std::any::type_name_of_val;
use std::fmt;
use std::sync::Arc;

#[cfg(feature = "database")]
use crate::database::ports::{
    folder_inventory::FolderInventoryRepository,
    images::ImageRepository,
    indices::IndicesRepository,
    library::LibraryRepository,
    media_files::{MediaFilesReadPort, MediaFilesWritePort},
    media_references::MediaReferencesRepository,
    processing_status::ProcessingStatusRepository,
    query::QueryRepository,
    rbac::RbacRepository,
    security_settings::SecuritySettingsRepository,
    setup_claims::SetupClaimsRepository,
    sync_sessions::SyncSessionsRepository,
    users::UsersRepository,
    watch_metrics::WatchMetricsReadPort,
    watch_status::WatchStatusRepository,
};
#[cfg(feature = "database")]
use crate::database::{
    infrastructure::postgres::{
        PostgresFolderInventoryRepository, PostgresImageRepository,
        PostgresIndicesRepository, PostgresLibraryRepository,
        PostgresMediaReferencesRepository, PostgresMediaRepository,
        PostgresProcessingStatusRepository, PostgresQueryRepository,
        PostgresRbacRepository, PostgresSecuritySettingsRepository,
        PostgresSetupClaimsRepository, PostgresSyncSessionsRepository,
        PostgresUsersRepository, PostgresWatchMetricsRepository,
        PostgresWatchStatusRepository,
    },
    postgres::PostgresDatabase,
};

/// Aggregates all repository ports used by application services.
///
/// This composition-based façade replaces the monolithic database interface in
/// application code while keeping construction/testing straightforward.
#[derive(Clone)]
pub struct AppUnitOfWork {
    pub libraries: Arc<dyn LibraryRepository>,
    pub media_refs: Arc<dyn MediaReferencesRepository>,
    pub media_files_read: Arc<dyn MediaFilesReadPort>,
    pub media_files_write: Arc<dyn MediaFilesWritePort>,
    pub images: Arc<dyn ImageRepository>,
    pub query: Arc<dyn QueryRepository>,

    pub users: Arc<dyn UsersRepository>,
    pub rbac: Arc<dyn RbacRepository>,
    pub security_settings: Arc<dyn SecuritySettingsRepository>,
    pub setup_claims: Arc<dyn SetupClaimsRepository>,

    pub watch_status: Arc<dyn WatchStatusRepository>,
    pub watch_metrics: Arc<dyn WatchMetricsReadPort>,

    pub sync_sessions: Arc<dyn SyncSessionsRepository>,

    pub folder_inventory: Arc<dyn FolderInventoryRepository>,
    pub processing_status: Arc<dyn ProcessingStatusRepository>,
    pub indices: Arc<dyn IndicesRepository>,
}

impl fmt::Debug for AppUnitOfWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppUnitOfWork")
            .field("libraries", &type_name_of_val(self.libraries.as_ref()))
            .field("media_refs", &type_name_of_val(self.media_refs.as_ref()))
            .field(
                "media_files_read",
                &type_name_of_val(self.media_files_read.as_ref()),
            )
            .field(
                "media_files_write",
                &type_name_of_val(self.media_files_write.as_ref()),
            )
            .field("images", &type_name_of_val(self.images.as_ref()))
            .field("query", &type_name_of_val(self.query.as_ref()))
            .field("users", &type_name_of_val(self.users.as_ref()))
            .field("rbac", &type_name_of_val(self.rbac.as_ref()))
            .field(
                "security_settings",
                &type_name_of_val(self.security_settings.as_ref()),
            )
            .field(
                "setup_claims",
                &type_name_of_val(self.setup_claims.as_ref()),
            )
            .field(
                "watch_status",
                &type_name_of_val(self.watch_status.as_ref()),
            )
            .field(
                "watch_metrics",
                &type_name_of_val(self.watch_metrics.as_ref()),
            )
            .field(
                "sync_sessions",
                &type_name_of_val(self.sync_sessions.as_ref()),
            )
            .field(
                "folder_inventory",
                &type_name_of_val(self.folder_inventory.as_ref()),
            )
            .field(
                "processing_status",
                &type_name_of_val(self.processing_status.as_ref()),
            )
            .field("indices", &type_name_of_val(self.indices.as_ref()))
            .finish()
    }
}

#[derive(Default)]
pub struct AppUnitOfWorkBuilder {
    libraries: Option<Arc<dyn LibraryRepository>>,
    media_refs: Option<Arc<dyn MediaReferencesRepository>>,
    media_files_read: Option<Arc<dyn MediaFilesReadPort>>,
    media_files_write: Option<Arc<dyn MediaFilesWritePort>>,
    images: Option<Arc<dyn ImageRepository>>,
    query: Option<Arc<dyn QueryRepository>>,

    users: Option<Arc<dyn UsersRepository>>,
    rbac: Option<Arc<dyn RbacRepository>>,
    security_settings: Option<Arc<dyn SecuritySettingsRepository>>,
    setup_claims: Option<Arc<dyn SetupClaimsRepository>>,

    watch_status: Option<Arc<dyn WatchStatusRepository>>,
    watch_metrics: Option<Arc<dyn WatchMetricsReadPort>>,

    sync_sessions: Option<Arc<dyn SyncSessionsRepository>>,

    folder_inventory: Option<Arc<dyn FolderInventoryRepository>>,
    processing_status: Option<Arc<dyn ProcessingStatusRepository>>,
    indices: Option<Arc<dyn IndicesRepository>>,
}

impl fmt::Debug for AppUnitOfWorkBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppUnitOfWorkBuilder")
            .field("libraries", &self.libraries.is_some())
            .field("media_refs", &self.media_refs.is_some())
            .field("media_files_read", &self.media_files_read.is_some())
            .field("media_files_write", &self.media_files_write.is_some())
            .field("images", &self.images.is_some())
            .field("query", &self.query.is_some())
            .field("users", &self.users.is_some())
            .field("rbac", &self.rbac.is_some())
            .field("security_settings", &self.security_settings.is_some())
            .field("setup_claims", &self.setup_claims.is_some())
            .field("watch_status", &self.watch_status.is_some())
            .field("watch_metrics", &self.watch_metrics.is_some())
            .field("sync_sessions", &self.sync_sessions.is_some())
            .field("folder_inventory", &self.folder_inventory.is_some())
            .field("processing_status", &self.processing_status.is_some())
            .field("indices", &self.indices.is_some())
            .finish()
    }
}

impl AppUnitOfWorkBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_libraries(mut self, repo: Arc<dyn LibraryRepository>) -> Self {
        self.libraries = Some(repo);
        self
    }
    pub fn with_media_refs(
        mut self,
        repo: Arc<dyn MediaReferencesRepository>,
    ) -> Self {
        self.media_refs = Some(repo);
        self
    }
    pub fn with_media_files_read(
        mut self,
        repo: Arc<dyn MediaFilesReadPort>,
    ) -> Self {
        self.media_files_read = Some(repo);
        self
    }
    pub fn with_media_files_write(
        mut self,
        repo: Arc<dyn MediaFilesWritePort>,
    ) -> Self {
        self.media_files_write = Some(repo);
        self
    }
    pub fn with_images(mut self, repo: Arc<dyn ImageRepository>) -> Self {
        self.images = Some(repo);
        self
    }
    pub fn with_query(mut self, repo: Arc<dyn QueryRepository>) -> Self {
        self.query = Some(repo);
        self
    }
    pub fn with_users(mut self, repo: Arc<dyn UsersRepository>) -> Self {
        self.users = Some(repo);
        self
    }
    pub fn with_rbac(mut self, repo: Arc<dyn RbacRepository>) -> Self {
        self.rbac = Some(repo);
        self
    }
    pub fn with_setup_claims(
        mut self,
        repo: Arc<dyn SetupClaimsRepository>,
    ) -> Self {
        self.setup_claims = Some(repo);
        self
    }
    pub fn with_watch_status(
        mut self,
        repo: Arc<dyn WatchStatusRepository>,
    ) -> Self {
        self.watch_status = Some(repo);
        self
    }
    pub fn with_watch_metrics(
        mut self,
        repo: Arc<dyn WatchMetricsReadPort>,
    ) -> Self {
        self.watch_metrics = Some(repo);
        self
    }
    pub fn with_sync_sessions(
        mut self,
        repo: Arc<dyn SyncSessionsRepository>,
    ) -> Self {
        self.sync_sessions = Some(repo);
        self
    }
    pub fn with_folder_inventory(
        mut self,
        repo: Arc<dyn FolderInventoryRepository>,
    ) -> Self {
        self.folder_inventory = Some(repo);
        self
    }
    pub fn with_processing_status(
        mut self,
        repo: Arc<dyn ProcessingStatusRepository>,
    ) -> Self {
        self.processing_status = Some(repo);
        self
    }
    pub fn with_indices(mut self, repo: Arc<dyn IndicesRepository>) -> Self {
        self.indices = Some(repo);
        self
    }

    /// Build a validated AppUnitOfWork. Returns a string error if any required
    /// repository is missing. Keep errors simple for ease of use at call sites.
    pub fn build(self) -> Result<AppUnitOfWork, String> {
        Ok(AppUnitOfWork {
            libraries: self
                .libraries
                .ok_or_else(|| "missing LibraryRepository".to_string())?,
            media_refs: self.media_refs.ok_or_else(|| {
                "missing MediaReferencesRepository".to_string()
            })?,
            media_files_read: self
                .media_files_read
                .ok_or_else(|| "missing MediaFilesReadPort".to_string())?,
            media_files_write: self
                .media_files_write
                .ok_or_else(|| "missing MediaFilesWritePort".to_string())?,
            images: self
                .images
                .ok_or_else(|| "missing ImageRepository".to_string())?,
            query: self
                .query
                .ok_or_else(|| "missing QueryRepository".to_string())?,
            users: self
                .users
                .ok_or_else(|| "missing UsersRepository".to_string())?,
            rbac: self
                .rbac
                .ok_or_else(|| "missing RbacRepository".to_string())?,
            security_settings: self.security_settings.ok_or_else(|| {
                "missing SecuritySettingsRepository".to_string()
            })?,
            setup_claims: self
                .setup_claims
                .ok_or_else(|| "missing SetupClaimsRepository".to_string())?,
            watch_status: self
                .watch_status
                .ok_or_else(|| "missing WatchStatusRepository".to_string())?,
            watch_metrics: self
                .watch_metrics
                .ok_or_else(|| "missing WatchMetricsReadPort".to_string())?,
            sync_sessions: self
                .sync_sessions
                .ok_or_else(|| "missing SyncSessionsRepository".to_string())?,
            folder_inventory: self.folder_inventory.ok_or_else(|| {
                "missing FolderInventoryRepository".to_string()
            })?,
            processing_status: self.processing_status.ok_or_else(|| {
                "missing ProcessingStatusRepository".to_string()
            })?,
            indices: self
                .indices
                .ok_or_else(|| "missing IndicesRepository".to_string())?,
        })
    }
}

#[cfg(feature = "database")]
impl AppUnitOfWork {
    /// Convenience helper to compose all Postgres-backed repositories into a unit of work.
    pub fn from_postgres(db: Arc<PostgresDatabase>) -> Result<Self, String> {
        AppUnitOfWorkBuilder::new().with_postgres(db).build()
    }
}

#[cfg(feature = "database")]
impl AppUnitOfWorkBuilder {
    /// Populate the builder with Postgres-backed repository adapters.
    pub fn with_postgres(mut self, db: Arc<PostgresDatabase>) -> Self {
        let pool = db.pool().clone();

        let libraries: Arc<dyn LibraryRepository> =
            Arc::new(PostgresLibraryRepository::new(pool.clone()));
        self.libraries = Some(libraries);

        let media_refs: Arc<dyn MediaReferencesRepository> =
            Arc::new(PostgresMediaReferencesRepository::new(pool.clone()));
        self.media_refs = Some(media_refs);

        let media_repo = Arc::new(PostgresMediaRepository::new(pool.clone()));
        self.media_files_read = Some(media_repo.clone());
        self.media_files_write = Some(media_repo.clone());

        let images: Arc<dyn ImageRepository> =
            Arc::new(PostgresImageRepository::new(pool.clone()));
        self.images = Some(images);

        let query: Arc<dyn QueryRepository> =
            Arc::new(PostgresQueryRepository::new(pool.clone()));
        self.query = Some(query);

        let users: Arc<dyn UsersRepository> =
            Arc::new(PostgresUsersRepository::new(pool.clone()));
        self.users = Some(users);

        let rbac: Arc<dyn RbacRepository> =
            Arc::new(PostgresRbacRepository::new(pool.clone()));
        self.rbac = Some(rbac);

        let security: Arc<dyn SecuritySettingsRepository> =
            Arc::new(PostgresSecuritySettingsRepository::new(pool.clone()));
        self.security_settings = Some(security);

        let setup_claims: Arc<dyn SetupClaimsRepository> =
            Arc::new(PostgresSetupClaimsRepository::new(pool.clone()));
        self.setup_claims = Some(setup_claims);

        let watch_status: Arc<dyn WatchStatusRepository> =
            Arc::new(PostgresWatchStatusRepository::new(pool.clone()));
        self.watch_status = Some(watch_status);

        let watch_metrics: Arc<dyn WatchMetricsReadPort> =
            Arc::new(PostgresWatchMetricsRepository::new(pool.clone()));
        self.watch_metrics = Some(watch_metrics);

        let sync_sessions: Arc<dyn SyncSessionsRepository> =
            Arc::new(PostgresSyncSessionsRepository::new(pool.clone()));
        self.sync_sessions = Some(sync_sessions);

        let folder_inventory: Arc<dyn FolderInventoryRepository> =
            Arc::new(PostgresFolderInventoryRepository::new(pool.clone()));
        self.folder_inventory = Some(folder_inventory);

        let indices: Arc<dyn IndicesRepository> =
            Arc::new(PostgresIndicesRepository::new(pool.clone()));
        self.indices = Some(indices);

        let processing_status: Arc<dyn ProcessingStatusRepository> =
            Arc::new(PostgresProcessingStatusRepository::new(pool));
        self.processing_status = Some(processing_status);

        self
    }
}
