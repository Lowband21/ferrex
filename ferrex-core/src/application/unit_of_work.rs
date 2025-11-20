use std::sync::Arc;

use crate::database::ports::{
    folder_inventory::FolderInventoryRepository,
    images::ImageRepository,
    library::LibraryRepository,
    media_files::{MediaFilesReadPort, MediaFilesWritePort},
    media_references::MediaReferencesRepository,
    processing_status::ProcessingStatusRepository,
    query::QueryRepository,
    rbac::RbacRepository,
    sync_sessions::SyncSessionsRepository,
    users::UsersRepository,
    watch_metrics::WatchMetricsReadPort,
    watch_status::WatchStatusRepository,
};
#[cfg(feature = "database")]
use crate::database::{
    infrastructure::postgres::{
        PostgresFolderInventoryRepository, PostgresImageRepository, PostgresLibraryRepository,
        PostgresMediaReferencesRepository, PostgresMediaRepository,
        PostgresProcessingStatusRepository, PostgresQueryRepository, PostgresRbacRepository,
        PostgresSyncSessionsRepository, PostgresUsersRepository, PostgresWatchMetricsRepository,
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

    pub watch_status: Arc<dyn WatchStatusRepository>,
    pub watch_metrics: Arc<dyn WatchMetricsReadPort>,

    pub sync_sessions: Arc<dyn SyncSessionsRepository>,

    pub folder_inventory: Arc<dyn FolderInventoryRepository>,
    pub processing_status: Arc<dyn ProcessingStatusRepository>,
}

pub struct AppUnitOfWorkBuilder {
    libraries: Option<Arc<dyn LibraryRepository>>,
    media_refs: Option<Arc<dyn MediaReferencesRepository>>,
    media_files_read: Option<Arc<dyn MediaFilesReadPort>>,
    media_files_write: Option<Arc<dyn MediaFilesWritePort>>,
    images: Option<Arc<dyn ImageRepository>>,
    query: Option<Arc<dyn QueryRepository>>,

    users: Option<Arc<dyn UsersRepository>>,
    rbac: Option<Arc<dyn RbacRepository>>,

    watch_status: Option<Arc<dyn WatchStatusRepository>>,
    watch_metrics: Option<Arc<dyn WatchMetricsReadPort>>,

    sync_sessions: Option<Arc<dyn SyncSessionsRepository>>,

    folder_inventory: Option<Arc<dyn FolderInventoryRepository>>,
    processing_status: Option<Arc<dyn ProcessingStatusRepository>>,
}

impl Default for AppUnitOfWorkBuilder {
    fn default() -> Self {
        Self {
            libraries: None,
            media_refs: None,
            media_files_read: None,
            media_files_write: None,
            images: None,
            query: None,
            users: None,
            rbac: None,
            watch_status: None,
            watch_metrics: None,
            sync_sessions: None,
            folder_inventory: None,
            processing_status: None,
        }
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
    pub fn with_media_refs(mut self, repo: Arc<dyn MediaReferencesRepository>) -> Self {
        self.media_refs = Some(repo);
        self
    }
    pub fn with_media_files_read(mut self, repo: Arc<dyn MediaFilesReadPort>) -> Self {
        self.media_files_read = Some(repo);
        self
    }
    pub fn with_media_files_write(mut self, repo: Arc<dyn MediaFilesWritePort>) -> Self {
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
    pub fn with_watch_status(mut self, repo: Arc<dyn WatchStatusRepository>) -> Self {
        self.watch_status = Some(repo);
        self
    }
    pub fn with_watch_metrics(mut self, repo: Arc<dyn WatchMetricsReadPort>) -> Self {
        self.watch_metrics = Some(repo);
        self
    }
    pub fn with_sync_sessions(mut self, repo: Arc<dyn SyncSessionsRepository>) -> Self {
        self.sync_sessions = Some(repo);
        self
    }
    pub fn with_folder_inventory(mut self, repo: Arc<dyn FolderInventoryRepository>) -> Self {
        self.folder_inventory = Some(repo);
        self
    }
    pub fn with_processing_status(mut self, repo: Arc<dyn ProcessingStatusRepository>) -> Self {
        self.processing_status = Some(repo);
        self
    }

    /// Build a validated AppUnitOfWork. Returns a string error if any required
    /// repository is missing. Keep errors simple for ease of use at call sites.
    pub fn build(self) -> Result<AppUnitOfWork, String> {
        Ok(AppUnitOfWork {
            libraries: self
                .libraries
                .ok_or_else(|| "missing LibraryRepository".to_string())?,
            media_refs: self
                .media_refs
                .ok_or_else(|| "missing MediaReferencesRepository".to_string())?,
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
            watch_status: self
                .watch_status
                .ok_or_else(|| "missing WatchStatusRepository".to_string())?,
            watch_metrics: self
                .watch_metrics
                .ok_or_else(|| "missing WatchMetricsReadPort".to_string())?,
            sync_sessions: self
                .sync_sessions
                .ok_or_else(|| "missing SyncSessionsRepository".to_string())?,
            folder_inventory: self
                .folder_inventory
                .ok_or_else(|| "missing FolderInventoryRepository".to_string())?,
            processing_status: self
                .processing_status
                .ok_or_else(|| "missing ProcessingStatusRepository".to_string())?,
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
            Arc::new(PostgresMediaReferencesRepository::new(db.clone()));
        self.media_refs = Some(media_refs);

        let media_repo = Arc::new(PostgresMediaRepository::new(pool.clone()));
        self.media_files_read = Some(media_repo.clone());
        self.media_files_write = Some(media_repo.clone());

        let images: Arc<dyn ImageRepository> = Arc::new(PostgresImageRepository::new(db.clone()));
        self.images = Some(images);

        let query: Arc<dyn QueryRepository> = Arc::new(PostgresQueryRepository::new(db.clone()));
        self.query = Some(query);

        let users: Arc<dyn UsersRepository> = Arc::new(PostgresUsersRepository::new(pool.clone()));
        self.users = Some(users);

        let rbac: Arc<dyn RbacRepository> = Arc::new(PostgresRbacRepository::new(pool.clone()));
        self.rbac = Some(rbac);

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

        let processing_status: Arc<dyn ProcessingStatusRepository> =
            Arc::new(PostgresProcessingStatusRepository::new(pool));
        self.processing_status = Some(processing_status);

        self
    }
}
