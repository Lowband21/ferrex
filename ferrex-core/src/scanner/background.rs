use crate::{
    LibraryLike, LibraryReference, MediaDatabase, Result, ScanOutput,
    database::traits::{FileWatchEventType, ScanType},
    providers::TmdbApiProvider,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{
    file_watcher::FileWatcher,
    incremental::IncrementalScanner,
    orchestrator::{ScanOptions, ScanOrchestrator},
};
use std::path::{Path, PathBuf};

/// Background scanner that runs continuously
pub struct BackgroundScanner {
    db: Arc<MediaDatabase>,
    orchestrator: Arc<ScanOrchestrator>,
    file_watcher: Arc<FileWatcher>,
    tmdb_provider: Arc<TmdbApiProvider>,
    output_tx: mpsc::UnboundedSender<ScanOutput>,
    shutdown_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<()>>>,
}

impl BackgroundScanner {
    pub fn new(
        db: Arc<MediaDatabase>,
        orchestrator: Arc<ScanOrchestrator>,
        file_watcher: Arc<FileWatcher>,
        tmdb_provider: Arc<TmdbApiProvider>,
        output_tx: mpsc::UnboundedSender<ScanOutput>,
        shutdown_rx: mpsc::Receiver<()>,
    ) -> Self {
        Self {
            db,
            orchestrator,
            file_watcher,
            tmdb_provider,
            output_tx,
            shutdown_rx: Arc::new(tokio::sync::Mutex::new(shutdown_rx)),
        }
    }

    /// Start the background scanner
    pub async fn run(self: Arc<Self>) -> Result<()> {
        info!("Starting background scanner");

        // Recover any interrupted scans
        let recovered_scans = self.orchestrator.recover_interrupted_scans().await?;
        for scan in recovered_scans {
            info!(
                "Recovered interrupted scan {} for library {}",
                scan.id, scan.library_id
            );
            // These are now marked as paused and can be resumed manually
        }

        // Start file watchers for all enabled libraries
        let libraries = self.db.backend().list_library_references().await?;
        for library in &libraries {
            if let Ok(lib) = self.db.backend().get_library(&library.id).await {
                if let Some(lib) = lib {
                    if lib.enabled && lib.watch_for_changes {
                        if let Err(e) = self.file_watcher.watch_library(library).await {
                            error!(
                                "Failed to start file watcher for library {}: {}",
                                library.name, e
                            );
                        }
                    }
                }
            }
        }

        // Spawn background tasks
        let scanner = self.clone();
        let file_watch_task = tokio::spawn(async move {
            scanner.process_file_watch_events().await;
        });

        let scanner = self.clone();
        let periodic_scan_task = tokio::spawn(async move {
            scanner.run_periodic_scans().await;
        });

        let scanner = self.clone();
        let cleanup_task = tokio::spawn(async move {
            scanner.run_cleanup_tasks().await;
        });

        // Wait for shutdown signal
        {
            let mut shutdown_rx = self.shutdown_rx.lock().await;
            let _ = shutdown_rx.recv().await;
        }

        info!("Shutting down background scanner");

        // Cancel all tasks
        file_watch_task.abort();
        periodic_scan_task.abort();
        cleanup_task.abort();

        Ok(())
    }

    /// Process file watch events
    async fn process_file_watch_events(&self) {
        let mut interval = interval(Duration::from_secs(5));

        loop {
            interval.tick().await;

            // Get all libraries
            match self.db.backend().list_library_references().await {
                Ok(libraries) => {
                    for library in libraries {
                        // Process events for this library
                        match self
                            .file_watcher
                            .get_unprocessed_events(library.id, 100)
                            .await
                        {
                            Ok(events) => {
                                // Group events into unique target folders to avoid redundant scans
                                use std::collections::HashSet;
                                let mut targets: HashSet<std::path::PathBuf> = HashSet::new();

                                for event in &events {
                                    debug!("Processing file watch event: {:?}", event);

                                    match event.event_type {
                                        FileWatchEventType::Created
                                        | FileWatchEventType::Modified
                                        | FileWatchEventType::Moved => {
                                            let path = std::path::Path::new(&event.file_path);
                                            let folder = match library.library_type {
                                                crate::LibraryType::Movies => {
                                                    if path.is_file() {
                                                        path.parent().map(|p| p.to_path_buf())
                                                    } else {
                                                        Some(path.to_path_buf())
                                                    }
                                                }
                                                crate::LibraryType::Series => {
                                                    self.find_series_root_folder(path)
                                                }
                                            };
                                            if let Some(f) = folder {
                                                targets.insert(f);
                                            }

                                            // If moved, handle deletion of old path
                                            if let FileWatchEventType::Moved = event.event_type {
                                                if let Some(old_path) = &event.old_path {
                                                    if let Err(e) =
                                                        self.handle_file_deletion(old_path).await
                                                    {
                                                        error!(
                                                            "Failed to handle old path deletion {}: {}",
                                                            old_path, e
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        FileWatchEventType::Deleted => {
                                            // Remove from database
                                            if let Err(e) =
                                                self.handle_file_deletion(&event.file_path).await
                                            {
                                                error!(
                                                    "Failed to handle file deletion {}: {}",
                                                    event.file_path, e
                                                );
                                            }
                                        }
                                        FileWatchEventType::Moved => {
                                            // Handle move (delete old, add new)
                                            if let Some(old_path) = &event.old_path {
                                                if let Err(e) =
                                                    self.handle_file_deletion(old_path).await
                                                {
                                                    error!(
                                                        "Failed to handle old path deletion {}: {}",
                                                        old_path, e
                                                    );
                                                }
                                            }
                                            if let Err(e) = self
                                                .scan_media_folder(&library, &event.file_path)
                                                .await
                                            {
                                                error!(
                                                    "Failed to scan media folder for moved file {}: {}",
                                                    event.file_path, e
                                                );
                                            }
                                        }
                                    }
                                }

                                // Execute scans per unique folder
                                for folder in targets {
                                    if let Err(e) = self
                                        .scan_media_folder(&library, &folder.to_string_lossy())
                                        .await
                                    {
                                        error!("Failed to scan media folder {:?}: {}", folder, e);
                                    }
                                }

                                // Mark events as processed
                                for event in events {
                                    if let Err(e) =
                                        self.file_watcher.mark_event_processed(event.id).await
                                    {
                                        error!("Failed to mark event as processed: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Failed to get unprocessed events for library {}: {}",
                                    library.id, e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to list libraries: {}", e);
                }
            }
        }
    }

    /// Run periodic scans based on library settings
    async fn run_periodic_scans(&self) {
        let mut interval = interval(Duration::from_secs(60)); // Check every minute

        loop {
            interval.tick().await;

            match self.db.backend().list_libraries().await {
                Ok(libraries) => {
                    for library in libraries {
                        if !library.enabled || !library.auto_scan {
                            continue;
                        }

                        if library.needs_scan() {
                            info!("Library {} needs periodic scan", library.name);

                            // Create library reference
                            let library_ref = LibraryReference {
                                id: library.id,
                                name: library.name.clone(),
                                library_type: library.library_type,
                                paths: library.paths.clone(),
                            };

                            // Start an incremental scan
                            let scan_options = ScanOptions {
                                force_refresh: false,
                                skip_file_metadata: false,
                                skip_tmdb: false,
                                analyze_files: library.analyze_on_scan,
                                retry_failed: true,
                                max_retries: library.max_retry_attempts,
                                batch_size: 100,
                                concurrent_workers: 4,
                            };

                            match self
                                .orchestrator
                                .create_scan(
                                    &library_ref,
                                    ScanType::Incremental,
                                    scan_options.clone(),
                                )
                                .await
                            {
                                Ok(scan_state) => {
                                    let scanner = IncrementalScanner::new(
                                        self.db.clone(),
                                        self.tmdb_provider.clone(),
                                        self.orchestrator.clone(),
                                        scan_options,
                                    );

                                    let (tx, mut rx) = mpsc::channel(1000);
                                    let output_tx = self.output_tx.clone();

                                    // Forward scan outputs
                                    tokio::spawn(async move {
                                        while let Some(output) = rx.recv().await {
                                            let _ = output_tx.send(output);
                                        }
                                    });

                                    // Run the scan
                                    tokio::spawn(async move {
                                        if let Err(e) = scanner
                                            .scan_incremental(library_ref, &scan_state, tx)
                                            .await
                                        {
                                            error!("Incremental scan failed: {}", e);
                                        }
                                    });

                                    // Update last scan time
                                    if let Err(e) = self
                                        .db
                                        .backend()
                                        .update_library_last_scan(&library.id)
                                        .await
                                    {
                                        error!("Failed to update last scan time: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to create scan for library {}: {}",
                                        library.name, e
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to list libraries for periodic scan: {}", e);
                }
            }
        }
    }

    /// Run cleanup tasks
    async fn run_cleanup_tasks(&self) {
        let mut interval = interval(Duration::from_secs(24 * 60 * 60)); // Daily cleanup

        loop {
            interval.tick().await;

            info!("Running cleanup tasks");

            // Cleanup old file watch events (keep 7 days)
            match self.file_watcher.cleanup_old_events(7).await {
                Ok(count) => {
                    if count > 0 {
                        info!("Cleaned up {} old file watch events", count);
                    }
                }
                Err(e) => {
                    error!("Failed to cleanup old events: {}", e);
                }
            }

            // Cleanup orphaned images
            match self.db.backend().cleanup_orphaned_images().await {
                Ok(count) => {
                    if count > 0 {
                        info!("Cleaned up {} orphaned images", count);
                    }
                }
                Err(e) => {
                    error!("Failed to cleanup orphaned images: {}", e);
                }
            }
        }
    }

    /// Scan a media folder based on file events
    async fn scan_media_folder(&self, library: &LibraryReference, file_path: &str) -> Result<()> {
        use std::path::{Path, PathBuf};

        let path = Path::new(file_path);

        // Determine the media folder based on library type
        let media_folder = match library.library_type {
            crate::LibraryType::Movies => {
                // For movies, the parent folder is the movie folder (e.g., "The Matrix (1999)/")
                if path.is_file() {
                    path.parent().map(|p| p.to_path_buf())
                } else {
                    Some(path.to_path_buf())
                }
            }
            crate::LibraryType::Series => {
                // For TV shows, we need to find the series root folder
                // Structure: Series Name/Season XX/episode.mkv
                self.find_series_root_folder(path)
            }
        };

        let Some(folder) = media_folder else {
            warn!("Could not determine media folder for path: {}", file_path);
            return Ok(());
        };

        info!(
            "Scanning media folder: {} (library type: {:?})",
            folder.display(),
            library.library_type
        );

        // Create a one-time output channel for this scan
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        // Forward events to main output channel
        let output_tx = self.output_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let _ = output_tx.send(event);
            }
        });

        // Use the streaming scanner directly to process the folder
        let config = crate::StreamingScannerConfig {
            folder_workers: 4,
            batch_size: 100,
            tmdb_rate_limit_ms: 250,
            fuzzy_match_threshold: 60,
            cache_dir: None,
            max_error_retries: 3,
            folder_batch_limit: 50,
            force_refresh: false,
        };

        let scanner = crate::StreamingScannerV2::with_config(
            config,
            self.db.clone(),
            self.tmdb_provider.clone(),
        );

        match library.library_type {
            crate::LibraryType::Movies => {
                match scanner.process_movie_folder(folder, library.id).await {
                    Ok(movie_ref) => {
                        let _ = tx.send(ScanOutput::MovieFound(movie_ref)).await;
                    }
                    Err(e) => {
                        error!("Failed to process movie folder: {}", e);
                        let _ = tx
                            .send(ScanOutput::Error {
                                path: Some(file_path.to_string()),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }
            crate::LibraryType::Series => {
                match scanner.process_series_folder(folder, library.id, &tx).await {
                    Ok(_) => {
                        info!("Successfully processed series folder");
                    }
                    Err(e) => {
                        error!("Failed to process series folder: {}", e);
                        let _ = tx
                            .send(ScanOutput::Error {
                                path: Some(file_path.to_string()),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Find the series root folder from a file path
    fn find_series_root_folder(&self, path: &Path) -> Option<PathBuf> {
        // For TV shows, we expect structure like:
        // /library/Series Name/Season 01/episode.mkv
        // We need to go up to "Series Name" folder

        if path.is_file() {
            // Go up two levels: episode file -> season folder -> series folder
            path.parent()?.parent().map(|p| p.to_path_buf())
        } else {
            // If it's a folder, check if it's a season folder (contains "Season" in name)
            let folder_name = path.file_name()?.to_str()?;
            if folder_name.contains("Season") || folder_name.starts_with("S") {
                // It's a season folder, go up one level
                path.parent().map(|p| p.to_path_buf())
            } else {
                // Assume it's already the series folder
                Some(path.to_path_buf())
            }
        }
    }

    /// Handle file deletion
    async fn handle_file_deletion(&self, file_path: &str) -> Result<()> {
        // Find the media file by path
        if let Some(media_file) = self.db.backend().get_media_by_path(file_path).await? {
            // Delete from database
            self.db
                .backend()
                .delete_media(&media_file.id.to_string())
                .await?;

            // Send deletion event
            let _ = self.output_tx.send(ScanOutput::Error {
                path: Some(file_path.to_string()),
                error: "File deleted".to_string(),
            });

            info!("Removed deleted file from database: {}", file_path);
        }

        Ok(())
    }
}
