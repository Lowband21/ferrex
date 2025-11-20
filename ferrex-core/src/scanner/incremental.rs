use crate::{
    database::traits::{MediaProcessingStatus, ScanState},
    providers::TmdbApiProvider,
    LibraryReference, MediaDatabase, Result, StreamingScannerConfig, StreamingScannerV2,
};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use super::orchestrator::{ScanOptions, ScanOrchestrator};

/// Incremental scanner that extends StreamingScannerV2 with state tracking
pub struct IncrementalScanner {
    scanner: Arc<StreamingScannerV2>,
    orchestrator: Arc<ScanOrchestrator>,
    db: Arc<MediaDatabase>,
    scan_options: ScanOptions,
}

impl IncrementalScanner {
    pub fn new(
        db: Arc<MediaDatabase>,
        tmdb_provider: Arc<TmdbApiProvider>,
        orchestrator: Arc<ScanOrchestrator>,
        scan_options: ScanOptions,
    ) -> Self {
        let config = StreamingScannerConfig {
            folder_workers: scan_options.concurrent_workers,
            batch_size: scan_options.batch_size,
            tmdb_rate_limit_ms: 250,
            fuzzy_match_threshold: 60,
            cache_dir: None,
            max_error_retries: 3,
            folder_batch_limit: 50,
        };

        let scanner = Arc::new(StreamingScannerV2::with_config(
            config,
            db.clone(),
            tmdb_provider,
        ));

        Self {
            scanner,
            orchestrator,
            db,
            scan_options,
        }
    }

    /// Perform an incremental scan
    pub async fn scan_incremental(
        &self,
        library: LibraryReference,
        scan_state: &ScanState,
        output_tx: mpsc::Sender<crate::ScanOutput>,
    ) -> Result<()> {
        info!(
            "Starting incremental scan {} for library {}",
            scan_state.id, library.name
        );

        // Get unprocessed files
        let unprocessed_metadata = self
            .db
            .backend()
            .get_unprocessed_files(library.id, "metadata", 1000)
            .await?;

        let unprocessed_tmdb = if !self.scan_options.skip_tmdb {
            self.db
                .backend()
                .get_unprocessed_files(library.id, "tmdb", 1000)
                .await?
        } else {
            vec![]
        };

        let unprocessed_images = if !self.scan_options.skip_tmdb {
            self.db
                .backend()
                .get_unprocessed_files(library.id, "images", 1000)
                .await?
        } else {
            vec![]
        };

        let failed_files = if self.scan_options.retry_failed {
            self.db
                .backend()
                .get_failed_files(library.id, self.scan_options.max_retries as i32)
                .await?
        } else {
            vec![]
        };

        info!(
            "Found {} unprocessed metadata, {} unprocessed TMDB, {} unprocessed images, {} failed files",
            unprocessed_metadata.len(),
            unprocessed_tmdb.len(),
            unprocessed_images.len(),
            failed_files.len()
        );

        // Update scan state with totals
        self.orchestrator
            .update_scan_progress(
                scan_state.id,
                None,
                Some(
                    (unprocessed_metadata.len()
                        + unprocessed_tmdb.len()
                        + unprocessed_images.len()
                        + failed_files.len()) as i32,
                ),
                None,
            )
            .await?;

        // Group files by their parent folders
        let mut folders_to_process =
            std::collections::HashMap::<std::path::PathBuf, Vec<crate::MediaFile>>::new();

        // Add unprocessed metadata files
        for file in unprocessed_metadata {
            let folder = self.get_media_folder(&file.path, &library);
            if let Some(folder_path) = folder {
                folders_to_process
                    .entry(folder_path)
                    .or_insert_with(Vec::new)
                    .push(file);
            }
        }

        // Add unprocessed TMDB files
        for file in unprocessed_tmdb {
            let folder = self.get_media_folder(&file.path, &library);
            if let Some(folder_path) = folder {
                folders_to_process
                    .entry(folder_path)
                    .or_insert_with(Vec::new)
                    .push(file);
            }
        }

        // Add failed files
        for file in failed_files {
            let folder = self.get_media_folder(&file.path, &library);
            if let Some(folder_path) = folder {
                folders_to_process
                    .entry(folder_path)
                    .or_insert_with(Vec::new)
                    .push(file);
            }
        }

        // Process each folder
        let mut processed_count = 0;
        for (folder_path, files) in folders_to_process {
            if !self.orchestrator.is_scan_active(scan_state.id).await {
                info!("Scan {} is no longer active, stopping", scan_state.id);
                break;
            }

            info!(
                "Processing folder: {} with {} unprocessed files",
                folder_path.display(),
                files.len()
            );

            // Process the entire folder using the scanner
            match self
                .process_media_folder(&folder_path, &library, output_tx.clone())
                .await
            {
                Ok(_) => {
                    processed_count += files.len();
                    self.orchestrator
                        .update_scan_progress(
                            scan_state.id,
                            None,
                            Some(processed_count as i32),
                            Some(folder_path.display().to_string()),
                        )
                        .await?;
                }
                Err(e) => {
                    error!("Failed to process folder {:?}: {}", folder_path, e);
                    self.orchestrator
                        .add_scan_error(
                            scan_state.id,
                            format!("Folder processing failed for {:?}: {}", folder_path, e),
                        )
                        .await?;

                    // Update processing status for all files in this folder
                    for file in files {
                        let mut status = self.get_or_create_processing_status(file.id).await?;
                        status.last_error = Some(format!("Folder processing failed: {}", e));
                        status.retry_count += 1;
                        status.next_retry_at = Some(Utc::now() + chrono::Duration::hours(1));
                        self.db
                            .backend()
                            .create_or_update_processing_status(&status)
                            .await?;
                    }
                }
            }
        }

        // For TV shows, we might have individual episodes that need processing
        // Process remaining unprocessed images (these weren't included in folder processing)
        for _media_file in unprocessed_images {
            if !self.orchestrator.is_scan_active(scan_state.id).await {
                info!("Scan {} is no longer active, stopping", scan_state.id);
                break;
            }

            // TODO: Implement image caching for already-matched files
            warn!("Image caching for existing files not yet implemented");
        }

        // Perform regular scan for new files
        info!("Checking for new files in library {}", library.name);
        self.scanner
            .clone()
            .scan_library(library, output_tx)
            .await?;

        Ok(())
    }

    /// Get the media folder for a file path based on library type
    fn get_media_folder(
        &self,
        file_path: &std::path::Path,
        library: &LibraryReference,
    ) -> Option<std::path::PathBuf> {
        match library.library_type {
            crate::LibraryType::Movies => {
                // For movies, the parent folder is the movie folder
                file_path.parent().map(|p| p.to_path_buf())
            }
            crate::LibraryType::Series => {
                // For TV shows, we need the series root folder
                // Structure: Series Name/Season XX/episode.mkv
                if file_path.is_file() {
                    // Go up two levels: episode file -> season folder -> series folder
                    file_path.parent()?.parent().map(|p| p.to_path_buf())
                } else if let Some(folder_name) = file_path.file_name()?.to_str() {
                    if folder_name.contains("Season") || folder_name.starts_with("S") {
                        // It's a season folder, go up one level
                        file_path.parent().map(|p| p.to_path_buf())
                    } else {
                        // Assume it's already the series folder
                        Some(file_path.to_path_buf())
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Process a media folder using the streaming scanner
    async fn process_media_folder(
        &self,
        folder_path: &std::path::Path,
        library: &LibraryReference,
        output_tx: mpsc::Sender<crate::ScanOutput>,
    ) -> Result<()> {
        match library.library_type {
            crate::LibraryType::Movies => {
                // Process as a movie folder
                match self
                    .scanner
                    .process_movie_folder(folder_path.to_path_buf(), library.id)
                    .await
                {
                    Ok(movie_ref) => {
                        let _ = output_tx
                            .send(crate::ScanOutput::MovieFound(movie_ref))
                            .await;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            crate::LibraryType::Series => {
                // Process as a series folder
                self.scanner
                    .process_series_folder(folder_path.to_path_buf(), library.id, &output_tx)
                    .await
            }
        }
    }

    /// Get or create processing status for a media file
    async fn get_or_create_processing_status(
        &self,
        media_file_id: Uuid,
    ) -> Result<MediaProcessingStatus> {
        if let Some(status) = self
            .db
            .backend()
            .get_processing_status(media_file_id)
            .await?
        {
            Ok(status)
        } else {
            Ok(MediaProcessingStatus {
                media_file_id,
                metadata_extracted: false,
                metadata_extracted_at: None,
                tmdb_matched: false,
                tmdb_matched_at: None,
                images_cached: false,
                images_cached_at: None,
                file_analyzed: false,
                file_analyzed_at: None,
                last_error: None,
                error_details: None,
                retry_count: 0,
                next_retry_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        }
    }

    /// Perform a metadata refresh scan
    pub async fn scan_refresh_metadata(
        &self,
        library: LibraryReference,
        scan_state: &ScanState,
        output_tx: mpsc::Sender<crate::ScanOutput>,
    ) -> Result<()> {
        info!(
            "Starting metadata refresh scan {} for library {}",
            scan_state.id, library.name
        );

        // For metadata refresh, we scan the library normally
        // The scanner will check existing references and update TMDB data
        self.scanner.clone().scan_library(library, output_tx).await
    }

    /// Perform an analyze scan
    pub async fn scan_analyze(
        &self,
        library: LibraryReference,
        scan_state: &ScanState,
        _output_tx: mpsc::Sender<crate::ScanOutput>,
    ) -> Result<()> {
        info!(
            "Starting analyze scan {} for library {}",
            scan_state.id, library.name
        );

        // Get files that haven't been analyzed
        let unanalyzed = self
            .db
            .backend()
            .get_unprocessed_files(library.id, "analyze", 1000)
            .await?;

        info!("Found {} files to analyze", unanalyzed.len());

        for media_file in unanalyzed {
            if !self.orchestrator.is_scan_active(scan_state.id).await {
                info!("Scan {} is no longer active, stopping", scan_state.id);
                break;
            }

            // TODO: Implement analysis (thumbnail generation, etc.)
            warn!(
                "File analysis not yet implemented for {:?}",
                media_file.path
            );

            // Update processing status
            let mut status = self.get_or_create_processing_status(media_file.id).await?;
            status.file_analyzed = true;
            status.file_analyzed_at = Some(Utc::now());

            self.db
                .backend()
                .create_or_update_processing_status(&status)
                .await?;
        }

        Ok(())
    }
}
