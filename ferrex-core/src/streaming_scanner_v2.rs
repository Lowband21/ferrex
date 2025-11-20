use crate::media::*;
use crate::providers::TmdbApiProvider;
use crate::{
    media, ImageService, LibraryType, MediaDatabase, MediaError, MediaFile, MetadataExtractor,
    Result, TvParser,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::warn;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Configuration for the streaming scanner v2
#[derive(Debug, Clone)]
pub struct StreamingScannerConfig {
    /// Number of concurrent folder scanning workers
    pub folder_workers: usize,
    /// Number of files to process in a batch
    pub batch_size: usize,
    /// Milliseconds to wait between TMDB API calls
    pub tmdb_rate_limit_ms: u64,
    /// Minimum fuzzy match score for series matching (0-100)
    pub fuzzy_match_threshold: i64,
    /// Path to cache directory for images
    pub cache_dir: Option<std::path::PathBuf>,
    /// Maximum number of error retries for failed folders
    pub max_error_retries: i32,
    /// Maximum number of folders to process in a single batch
    pub folder_batch_limit: usize,
}

impl Default for StreamingScannerConfig {
    fn default() -> Self {
        Self {
            folder_workers: 4,
            batch_size: 100,
            tmdb_rate_limit_ms: 250,   // 4 requests per second
            fuzzy_match_threshold: 60, // 60% match minimum
            cache_dir: None,
            max_error_retries: 3,      // 3 retry attempts
            folder_batch_limit: 50,     // Process up to 50 folders per batch
        }
    }
}

/// Scanner output events sent to player during scanning
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanOutput {
    MovieFound(MovieReference),
    SeriesFound(SeriesReference),
    SeasonFound(SeasonReference),
    EpisodeFound(EpisodeReference),
    ScanProgress {
        scan_id: Uuid,
        folders_processed: usize,
        total_folders: usize,
    },
    ScanComplete {
        scan_id: Uuid,
        movies_found: usize,
        series_found: usize,
        episodes_found: usize,
        duration_secs: u64,
    },
    Error {
        path: Option<String>,
        error: String,
    },
}

/// Streaming scanner v2 - library-type aware scanning with TMDB integration
pub struct StreamingScannerV2 {
    config: StreamingScannerConfig,
    db: Arc<MediaDatabase>,
    metadata_extractor: Arc<tokio::sync::Mutex<MetadataExtractor>>,
    tmdb_provider: Arc<TmdbApiProvider>,
    fuzzy_matcher: SkimMatcherV2,
    last_tmdb_request: Arc<Mutex<Instant>>,
    image_service: Arc<ImageService>,
}

impl StreamingScannerV2 {
    /// Create a new streaming scanner v2
    pub fn new(db: Arc<MediaDatabase>, tmdb_provider: Arc<TmdbApiProvider>) -> Self {
        Self::with_config(StreamingScannerConfig::default(), db, tmdb_provider)
    }

    /// Create a new streaming scanner v2 with custom configuration
    pub fn with_config(
        config: StreamingScannerConfig,
        db: Arc<MediaDatabase>,
        tmdb_provider: Arc<TmdbApiProvider>,
    ) -> Self {
        let metadata_extractor = Arc::new(tokio::sync::Mutex::new(MetadataExtractor::new()));

        // Create image service with cache directory
        let cache_dir = config
            .cache_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("/tmp/ferrex_cache"));
        let image_service = Arc::new(ImageService::new(db.clone(), cache_dir));

        Self {
            config,
            db,
            metadata_extractor,
            tmdb_provider,
            fuzzy_matcher: SkimMatcherV2::default(),
            last_tmdb_request: Arc::new(Mutex::new(Instant::now())),
            image_service,
        }
    }

    /// Scan a library and stream results to the output channel
    pub async fn scan_library(
        self: Arc<Self>,
        library: LibraryReference,
        output_tx: mpsc::Sender<ScanOutput>,
    ) -> Result<()> {
        let scan_id = Uuid::new_v4();
        let start_time = Instant::now();

        info!(
            "Starting scan {} for library: {} (type: {:?})",
            scan_id, library.name, library.library_type
        );

        match library.library_type {
            LibraryType::Movies => {
                self.scan_movie_library(library, output_tx.clone(), scan_id)
                    .await?
            }
            LibraryType::TvShows => {
                self.scan_tv_library(library, output_tx.clone(), scan_id)
                    .await?
            }
        }

        // Send completion event
        let duration_secs = start_time.elapsed().as_secs();
        let _ = output_tx
            .send(ScanOutput::ScanComplete {
                scan_id,
                movies_found: 0, // TODO: Track these
                series_found: 0,
                episodes_found: 0,
                duration_secs,
            })
            .await;

        Ok(())
    }

    /// Scan a movie library using folder inventory
    async fn scan_movie_library(
        self: Arc<Self>,
        library: LibraryReference,
        output_tx: mpsc::Sender<ScanOutput>,
        scan_id: Uuid,
    ) -> Result<()> {
        info!("Scanning movie library: {} using folder inventory", library.name);

        let mut total_folders_processed = 0;
        let mut batch_number = 0;
        
        // Loop to process all folders in batches
        loop {
            batch_number += 1;
            
            // Query folder inventory for folders needing scan
            // Prioritize unscanned folders first
            let filters = crate::database::traits::FolderScanFilters {
                library_id: Some(library.id),
                processing_status: Some(crate::database::traits::FolderProcessingStatus::Pending),
                folder_type: Some(crate::database::traits::FolderType::Movie),
                max_attempts: Some(3),
                stale_after_hours: None,
                limit: None,
                priority: None,
                max_batch_size: Some(self.config.folder_batch_limit as i32),
                error_retry_threshold: Some(self.config.max_error_retries),
            };

            // Get pending folders first
            let mut folders_to_scan = self.db.backend().get_folders_needing_scan(&filters).await?;
            
            // Also get failed folders that haven't exceeded max attempts
            let mut failed_filters = filters.clone();
            failed_filters.processing_status = Some(crate::database::traits::FolderProcessingStatus::Failed);
            let failed_folders = self.db.backend().get_folders_needing_scan(&failed_filters).await?;
            folders_to_scan.extend(failed_folders);

            let batch_size = folders_to_scan.len();
            
            // If no more folders to scan, we're done
            if batch_size == 0 {
                if total_folders_processed == 0 {
                    info!("No movie folders need scanning for library: {}", library.name);
                } else {
                    info!("Completed scanning all {} movie folders for library: {}", 
                          total_folders_processed, library.name);
                }
                break;
            }

            info!("Processing batch {} with {} movie folders for library: {} (total processed so far: {})", 
                  batch_number, batch_size, library.name, total_folders_processed);

            // Create work channel
            let (folder_tx, folder_rx) = mpsc::channel(100);
            let folder_rx = Arc::new(Mutex::new(folder_rx));

            // Send folders to channel
            let folder_tx_clone = folder_tx.clone();
            let db = self.db.clone();
            tokio::spawn(async move {
                for folder in folders_to_scan {
                    // Update folder status to 'scanning' before processing
                    if let Err(e) = db.backend().update_folder_status(
                        folder.id,
                        crate::database::traits::FolderProcessingStatus::Processing,
                        None
                    ).await {
                        error!("Failed to update folder {} status to scanning: {}", folder.folder_path, e);
                        continue;
                    }
                    
                    // Send folder info to worker
                    if folder_tx_clone.send((folder.id, PathBuf::from(folder.folder_path))).await.is_err() {
                        break;
                    }
                }
            });
            drop(folder_tx); // Close sender when done

            // Spawn workers
            let mut workers = Vec::new();
            for worker_id in 0..self.config.folder_workers {
                let worker = self.clone().spawn_movie_worker_with_inventory(
                    worker_id,
                    folder_rx.clone(),
                    output_tx.clone(),
                    library.id,
                    scan_id,
                );
                workers.push(worker);
            }

            // Wait for all workers to complete
            for worker in workers {
                if let Err(e) = worker.await {
                    error!("Movie worker failed: {}", e);
                }
            }
            
            total_folders_processed += batch_size;
            info!("Completed batch {} ({} folders). Total processed: {}", 
                  batch_number, batch_size, total_folders_processed);
        }

        // Update library's last_scan timestamp after successful scan
        if let Err(e) = self.db.backend().update_library_last_scan(&library.id.to_string()).await {
            error!("Failed to update library last_scan timestamp: {}", e);
        } else {
            info!("Updated last_scan timestamp for library: {}", library.name);
        }

        Ok(())
    }

    /// Scan a TV library using folder inventory
    async fn scan_tv_library(
        self: Arc<Self>,
        library: LibraryReference,
        output_tx: mpsc::Sender<ScanOutput>,
        scan_id: Uuid,
    ) -> Result<()> {
        info!("Scanning TV library: {} using folder inventory", library.name);

        let mut total_folders_processed = 0;
        let mut batch_number = 0;
        
        // Loop to process all folders in batches
        loop {
            batch_number += 1;
            
            // Query folder inventory for series folders needing scan
            let filters = crate::database::traits::FolderScanFilters {
                library_id: Some(library.id),
                processing_status: Some(crate::database::traits::FolderProcessingStatus::Pending),
                folder_type: Some(crate::database::traits::FolderType::TvShow),
                max_attempts: Some(3),
                stale_after_hours: None,
                limit: None,
                priority: None,
                max_batch_size: Some(self.config.folder_batch_limit as i32),
                error_retry_threshold: Some(self.config.max_error_retries),
            };

            // Get pending folders first
            let mut folders_to_scan = self.db.backend().get_folders_needing_scan(&filters).await?;
            
            // Also get failed folders that haven't exceeded max attempts
            let mut failed_filters = filters.clone();
            failed_filters.processing_status = Some(crate::database::traits::FolderProcessingStatus::Failed);
            let failed_folders = self.db.backend().get_folders_needing_scan(&failed_filters).await?;
            folders_to_scan.extend(failed_folders);

            let batch_size = folders_to_scan.len();
            
            // If no more folders to scan, we're done
            if batch_size == 0 {
                if total_folders_processed == 0 {
                    info!("No TV series folders need scanning for library: {}", library.name);
                } else {
                    info!("Completed scanning all {} TV series folders for library: {}", 
                          total_folders_processed, library.name);
                }
                break;
            }

            info!("Processing batch {} with {} TV series folders for library: {} (total processed so far: {})", 
                  batch_number, batch_size, library.name, total_folders_processed);

            // Process each series folder in this batch
            for (idx, folder_info) in folders_to_scan.into_iter().enumerate() {
                // Send progress update with cumulative count
                let _ = output_tx
                    .send(ScanOutput::ScanProgress {
                        scan_id,
                        folders_processed: total_folders_processed + idx,
                        total_folders: total_folders_processed + batch_size, // Estimate based on current knowledge
                    })
                    .await;

                // Update folder status to 'scanning' before processing
                if let Err(e) = self.db.backend().update_folder_status(
                    folder_info.id,
                    crate::database::traits::FolderProcessingStatus::Processing,
                    None
                ).await {
                    error!("Failed to update folder {} status to scanning: {}", folder_info.folder_path, e);
                    continue;
                }

                let scanner = self.clone();
                let folder_path = PathBuf::from(&folder_info.folder_path);
                let folder_id = folder_info.id;
                
                match scanner.process_series_folder(folder_path.clone(), library.id, &output_tx).await {
                    Ok(_) => {
                        // Update folder status to 'completed' after successful processing
                        if let Err(e) = self.db.backend().update_folder_status(
                            folder_id,
                            crate::database::traits::FolderProcessingStatus::Completed,
                            None
                        ).await {
                            error!("Failed to update folder {} status to completed: {}", folder_info.folder_path, e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to process series folder {}: {}", folder_info.folder_path, e);
                        
                        // Update folder status to 'failed' with error message
                        if let Err(update_err) = self.db.backend().update_folder_status(
                            folder_id,
                            crate::database::traits::FolderProcessingStatus::Failed,
                            Some(e.to_string())
                        ).await {
                            error!("Failed to update folder {} status to failed: {}", folder_info.folder_path, update_err);
                        }
                        
                        let _ = output_tx
                            .send(ScanOutput::Error {
                                path: Some(folder_info.folder_path),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }
            
            total_folders_processed += batch_size;
            info!("Completed batch {} ({} folders). Total processed: {}", 
                  batch_number, batch_size, total_folders_processed);
        }

        // Update library's last_scan timestamp after successful scan
        if let Err(e) = self.db.backend().update_library_last_scan(&library.id.to_string()).await {
            error!("Failed to update library last_scan timestamp: {}", e);
        } else {
            info!("Updated last_scan timestamp for library: {}", library.name);
        }

        Ok(())
    }

    /// Cache image and return endpoint URL using ImageService
    /// Returns (endpoint, optional_theme_color)
    async fn cache_image(
        &self,
        media_type: &str,
        media_id: &str,
        category: &str,
        index: usize,
        tmdb_path: &str,
    ) -> Option<(String, Option<String>)> {
        debug!("cache_image called: type={}, id={}, category={}, index={}, path={}", 
               media_type, media_id, category, index, tmdb_path);
        
        // Parse media_id to UUID
        let media_uuid = match Uuid::parse_str(media_id) {
            Ok(uuid) => uuid,
            Err(e) => {
                warn!("Invalid media ID {}: {}", media_id, e);
                return None;
            }
        };

        // Download images immediately for better quality control
        let theme_color = if category == "poster" && index == 0 {
            info!("Downloading poster for theme color extraction: {} {} {}", media_type, media_id, tmdb_path);
            // Download the poster variant to extract theme color
            match self
                .image_service
                .download_variant(tmdb_path, crate::image_service::TmdbImageSize::PosterW342)
                .await
            {
                Ok((_, _, extracted_color)) => {
                    info!("Poster downloaded, extracted theme color: {:?}", extracted_color);
                    extracted_color
                },
                Err(e) => {
                    warn!("Failed to download poster for theme color extraction: {}", e);
                    None
                }
            }
        } else if category == "backdrop" {
            info!("Downloading backdrop at original resolution: {} {} {}", media_type, media_id, tmdb_path);
            // Download backdrop at original resolution for best quality
            match self
                .image_service
                .download_variant(tmdb_path, crate::image_service::TmdbImageSize::Original)
                .await
            {
                Ok((_, _, _)) => {
                    info!("Backdrop downloaded at original resolution");
                    None
                },
                Err(e) => {
                    warn!("Failed to download backdrop: {}", e);
                    None
                }
            }
        } else if category == "still" {
            info!("Downloading episode still: {} {} {}", media_type, media_id, tmdb_path);
            // Download episode stills at multiple sizes for different use cases
            
            // First download the smaller size for thumbnails
            match self
                .image_service
                .download_variant(tmdb_path, crate::image_service::TmdbImageSize::StillW300)
                .await
            {
                Ok((_, _, _)) => {
                    info!("Episode still w300 downloaded successfully");
                },
                Err(e) => {
                    warn!("Failed to download episode still w300: {}", e);
                }
            }
            
            // Then download larger size for detailed views
            match self
                .image_service
                .download_variant(tmdb_path, crate::image_service::TmdbImageSize::StillW500)
                .await
            {
                Ok((_, _, _)) => {
                    info!("Episode still w500 downloaded successfully");
                    None
                },
                Err(e) => {
                    warn!("Failed to download episode still w500: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Link the image to the media item in database
        debug!("Linking image to media: type={}, uuid={}, path={}, category={}", 
               media_type, media_uuid, tmdb_path, category);
        
        if let Err(e) = self
            .image_service
            .link_to_media(
                media_type,
                media_uuid,
                tmdb_path,
                category,
                index as i32,
                index == 0, // First image is primary
            )
            .await
        {
            warn!("Failed to link image to media: type={}, uuid={}, error={}", media_type, media_uuid, e);
            return None;
        }

        debug!("Successfully linked image to media: type={}, uuid={}", media_type, media_uuid);

        // Return the endpoint URL that the image handler will use
        let endpoint = format!(
            "/images/{}/{}/{}/{}",
            media_type, media_id, category, index
        );
        
        debug!("Returning endpoint: {}", endpoint);
        
        Some((endpoint, theme_color))
    }




    /// Check if a file is a video file
    fn is_video_file(&self, path: &Path) -> bool {
        let video_extensions = [
            "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg",
        ];

        if let Some(extension) = path.extension() {
            if let Some(ext_str) = extension.to_str() {
                return video_extensions.contains(&ext_str.to_lowercase().as_str());
            }
        }
        false
    }

    /// Spawn a worker to process movie folders
    fn spawn_movie_worker(
        self: Arc<Self>,
        worker_id: usize,
        folder_rx: Arc<Mutex<mpsc::Receiver<PathBuf>>>,
        output_tx: mpsc::Sender<ScanOutput>,
        library_id: Uuid,
        _scan_id: Uuid,
    ) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            info!("Movie worker {} started", worker_id);

            loop {
                let folder = {
                    let mut rx = folder_rx.lock().await;
                    rx.recv().await
                };

                let Some(folder) = folder else {
                    info!("Movie worker {} completed", worker_id);
                    break;
                };

                match self.process_movie_folder(folder.clone(), library_id).await {
                    Ok(movie_ref) => {
                        info!(
                            "Movie worker {} processed: {}",
                            worker_id,
                            movie_ref.title.as_str()
                        );
                        let _ = output_tx.send(ScanOutput::MovieFound(movie_ref)).await;
                    }
                    Err(e) => {
                        error!(
                            "Movie worker {} failed to process {:?}: {}",
                            worker_id, folder, e
                        );
                        let _ = output_tx
                            .send(ScanOutput::Error {
                                path: Some(folder.display().to_string()),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            Ok(())
        })
    }

    /// Spawn a worker to process movie folders with inventory tracking
    fn spawn_movie_worker_with_inventory(
        self: Arc<Self>,
        worker_id: usize,
        folder_rx: Arc<Mutex<mpsc::Receiver<(Uuid, PathBuf)>>>,
        output_tx: mpsc::Sender<ScanOutput>,
        library_id: Uuid,
        _scan_id: Uuid,
    ) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            info!("Movie worker {} started (with inventory tracking)", worker_id);

            loop {
                let folder_data = {
                    let mut rx = folder_rx.lock().await;
                    rx.recv().await
                };

                let Some((folder_id, folder)) = folder_data else {
                    info!("Movie worker {} completed", worker_id);
                    break;
                };

                match self.process_movie_folder(folder.clone(), library_id).await {
                    Ok(movie_ref) => {
                        info!(
                            "Movie worker {} processed: {}",
                            worker_id,
                            movie_ref.title.as_str()
                        );
                        
                        // Update folder status to 'completed' after successful processing
                        if let Err(e) = self.db.backend().update_folder_status(
                            folder_id,
                            crate::database::traits::FolderProcessingStatus::Completed,
                            None
                        ).await {
                            error!("Failed to update folder {} status to completed: {}", folder.display(), e);
                        }
                        
                        let _ = output_tx.send(ScanOutput::MovieFound(movie_ref)).await;
                    }
                    Err(e) => {
                        error!(
                            "Movie worker {} failed to process {:?}: {}",
                            worker_id, folder, e
                        );
                        
                        // Update folder status to 'failed' with error message
                        if let Err(update_err) = self.db.backend().update_folder_status(
                            folder_id,
                            crate::database::traits::FolderProcessingStatus::Failed,
                            Some(e.to_string())
                        ).await {
                            error!("Failed to update folder {} status to failed: {}", folder.display(), update_err);
                        }
                        
                        let _ = output_tx
                            .send(ScanOutput::Error {
                                path: Some(folder.display().to_string()),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            Ok(())
        })
    }

    /// Process a movie folder and return a complete MovieReference
    pub async fn process_movie_folder(
        &self,
        folder: PathBuf,
        library_id: Uuid,
    ) -> Result<MovieReference> {
        debug!("Processing movie folder: {:?}", folder);

        let (parsed_info, video_file) = if folder.is_file() {
            // Handle loose video file
            let file_stem = folder
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Try parsing the filename
            let parsed = self.parse_movie_filename(&file_stem)?;

            (parsed, folder.clone())
        } else {
            // Handle proper folder structure
            let folder_name = folder
                .file_name()
                .ok_or_else(|| MediaError::InvalidMedia("Invalid folder path".to_string()))?
                .to_string_lossy()
                .to_string();

            let parsed_info = self.parse_movie_folder_name(&folder_name)?;
            let video_file = self.find_main_video_file(&folder).await?;

            (parsed_info, video_file)
        };

        // Create MediaFile
        info!(
            "Creating MediaFile with library_id: {} for file: {:?}",
            library_id, video_file
        );
        let mut media_file = MediaFile::new(video_file.clone(), library_id)?;
        info!(
            "MediaFile created with ID: {} and library_id: {}",
            media_file.id, media_file.library_id
        );

        // Extract technical metadata
        {
            let mut extractor = self.metadata_extractor.lock().await;
            extractor.set_library_type(Some(LibraryType::Movies));
            match extractor.extract_metadata(&media_file.path) {
                Ok(metadata) => {
                    media_file.media_file_metadata = Some(metadata);
                }
                Err(e) => {
                    warn!("Failed to extract metadata from {:?}: {}", video_file, e);
                }
            }
        }

        // Rate limit TMDB requests
        self.rate_limit_tmdb().await;

        // Clean the title
        let folder_name = folder
            .file_name()
            .ok_or_else(|| MediaError::InvalidMedia("Invalid folder path".to_string()))?
            .to_string_lossy()
            .to_string();
        //e for TMDB search - normalize unicode and remove problematic characters
        let clean_title = self.clean_movie_title(&parsed_info.title);
        info!(
            "Searching TMDB for: '{}' (cleaned from: '{}')",
            clean_title, folder_name
        );

        // Search TMDB
        let tmdb_results = self
            .tmdb_provider
            .search_movies(&clean_title, parsed_info.year)
            .await
            .map_err(|e| MediaError::Internal(format!("TMDB search failed: {}", e)))?;

        // Find best match using fuzzy matching and year if available
        let best_match = tmdb_results.first();
        /*
            .into_iter()
            .map(|result| {
                let mut score = self
                    .fuzzy_matcher
                    .fuzzy_match(&result.title.as_str(), &clean_title)
                    .unwrap_or(0);

                // Boost score if year matches
                if let (Some(movie_year), Some(parsed_year)) =
                    (result.details.get_release_year(), parsed_info.year)
                {
                    if movie_year == parsed_year {
                        score += 100; // Significant boost for year match
                    } else if (movie_year as i32 - parsed_year as i32).abs() <= 1 {
                        score += 10; // Small boost for close year
                    }
                }

                (result, score)
            })
            .max_by_key(|(_, score)| *score);
        let tmdb_match = if let Some((result, score)) = best_match {
            if score >= self.config.fuzzy_match_threshold {
                info!(
                    "Found TMDB match for '{}': {} (score: {}, ID: {})",
                    parsed_info.title,
                    result.title.as_str(),
                    score,
                    result.tmdb_id
                );
                result
            } else {
                warn!(
                    "Best TMDB match for '{}' scored too low: {} (score: {})",
                    parsed_info.title,
                    result.title.as_str(),
                    score
                );
                // Store without TMDB data instead of failing
                return self.store_movie_without_tmdb(media_file, parsed_info).await;
            }
        } else {
            warn!(
                "No TMDB matches found for: '{}', storing without metadata",
                parsed_info.title
            );
            return self.store_movie_without_tmdb(media_file, parsed_info).await;
        };
        */
        let tmdb_match = if let Some(result) = best_match {
            result
        } else {
            warn!(
                "No TMDB matches found for: '{}', storing without metadata",
                parsed_info.title
            );
            return self.store_movie_without_tmdb(media_file, parsed_info).await;
        };

        // Fetch full TMDB details
        self.rate_limit_tmdb().await;
        let tmdb_details = self
            .tmdb_provider
            .get_movie(tmdb_match.tmdb_id)
            .await
            .map_err(|e| MediaError::Internal(format!("Failed to fetch movie details: {}", e)))?;

        // Fetch additional metadata
        let images = {
            self.rate_limit_tmdb().await;
            self.tmdb_provider
                .get_movie_images(tmdb_match.tmdb_id)
                .await
                .ok()
        };

        let credits = {
            self.rate_limit_tmdb().await;
            match self.tmdb_provider
                .get_movie_credits(tmdb_match.tmdb_id)
                .await {
                Ok(credits) => {
                    info!("Successfully fetched movie credits for TMDB ID {}", tmdb_match.tmdb_id);
                    Some(credits)
                }
                Err(e) => {
                    warn!("Failed to fetch movie credits for TMDB ID {}: {}", tmdb_match.tmdb_id, e);
                    None
                }
            }
        };

        // Generate movie ID early for image caching
        let movie_id = MovieID::new(Uuid::new_v4().to_string())?;
        let movie_id_str = movie_id.as_str();

        // Cache images and build MediaImages with metadata
        let mut cached_posters = vec![];
        let mut cached_backdrops = vec![];
        let mut cached_logos = vec![];

        // Handle main poster from movie details and extract theme color
        let mut theme_color = None;
        if let Some(poster_path) = tmdb_details.inner.poster_path.as_ref() {
            info!("Movie {} - Caching poster from path: {}", movie_id_str, poster_path);
            if let Some((endpoint, extracted_color)) = self
                .cache_image("movie", &movie_id_str, "poster", 0, poster_path)
                .await
            {
                theme_color = extracted_color;
                info!(
                    "Movie {} - Poster cached with endpoint: {}, theme_color: {:?}",
                    movie_id_str, endpoint, theme_color
                );
                cached_posters.push(media::ImageWithMetadata {
                    endpoint,
                    metadata: media::ImageMetadata {
                        file_path: poster_path.clone(),
                        width: 500,  // Default width for main poster
                        height: 750, // Default height for main poster
                        aspect_ratio: 0.667,
                        iso_639_1: None,
                        vote_average: 0.0,
                        vote_count: 0,
                    },
                });
            }
        }

        // Handle main backdrop from movie details
        if let Some(backdrop_path) = tmdb_details.inner.backdrop_path.as_ref() {
            if let Some((endpoint, _)) = self
                .cache_image("movie", &movie_id_str, "backdrop", 0, backdrop_path)
                .await
            {
                cached_backdrops.push(media::ImageWithMetadata {
                    endpoint,
                    metadata: media::ImageMetadata {
                        file_path: backdrop_path.clone(),
                        width: 1920,  // Default width for backdrop
                        height: 1080, // Default height for backdrop
                        aspect_ratio: 1.778,
                        iso_639_1: None,
                        vote_average: 0.0,
                        vote_count: 0,
                    },
                });
            }
        }

        // Cache additional images from MovieImagesResult
        if let Some(img_result) = &images {
            // Cache posters (skip index 0 if we already have main poster)
            let start_idx = if cached_posters.is_empty() { 0 } else { 1 };
            for (idx, poster) in img_result.posters.iter().enumerate() {
                if !poster.file_path.is_empty() {
                    if let Some((endpoint, extracted_color)) = self
                        .cache_image(
                            "movie",
                            &movie_id_str,
                            "poster",
                            start_idx + idx,
                            &poster.file_path,
                        )
                        .await
                    {
                        // If this is the first poster and we don't have a theme color yet, use it
                        if theme_color.is_none() && start_idx + idx == 0 {
                            theme_color = extracted_color;
                        }
                        cached_posters.push(media::ImageWithMetadata {
                            endpoint,
                            metadata: media::ImageMetadata {
                                file_path: poster.file_path.clone(),
                                width: poster.width,
                                height: poster.height,
                                aspect_ratio: poster.aspect_ratio,
                                iso_639_1: poster.iso_639_1.clone(),
                                vote_average: poster.vote_average,
                                vote_count: poster.vote_count,
                            },
                        });
                    }
                }
            }

            // Cache backdrops (skip index 0 if we already have main backdrop)
            let start_idx = if cached_backdrops.is_empty() { 0 } else { 1 };
            for (idx, backdrop) in img_result.backdrops.iter().enumerate() {
                if !backdrop.file_path.is_empty() {
                    if let Some((endpoint, _)) = self
                        .cache_image(
                            "movie",
                            &movie_id_str,
                            "backdrop",
                            start_idx + idx,
                            &backdrop.file_path,
                        )
                        .await
                    {
                        cached_backdrops.push(media::ImageWithMetadata {
                            endpoint,
                            metadata: media::ImageMetadata {
                                file_path: backdrop.file_path.clone(),
                                width: backdrop.width,
                                height: backdrop.height,
                                aspect_ratio: backdrop.aspect_ratio,
                                iso_639_1: backdrop.iso_639_1.clone(),
                                vote_average: backdrop.vote_average,
                                vote_count: backdrop.vote_count,
                            },
                        });
                    }
                }
            }

            // Cache logos
            for (idx, logo) in img_result.logos.iter().enumerate() {
                if !logo.file_path.is_empty() {
                    if let Some((endpoint, _)) = self
                        .cache_image("movie", &movie_id_str, "logo", idx, &logo.file_path)
                        .await
                    {
                        cached_logos.push(media::ImageWithMetadata {
                            endpoint,
                            metadata: media::ImageMetadata {
                                file_path: logo.file_path.clone(),
                                width: logo.width,
                                height: logo.height,
                                aspect_ratio: logo.aspect_ratio,
                                iso_639_1: logo.iso_639_1.clone(),
                                vote_average: logo.vote_average,
                                vote_count: logo.vote_count,
                            },
                        });
                    }
                }
            }
        }

        // Extract cast and crew
        let mut cast = vec![];
        let mut crew = vec![];

        if let Some(credits_result) = &credits {
            warn!("Movie {} - Processing credits: {} cast, {} crew members", movie_id_str, credits_result.cast.len(), credits_result.crew.len());
            cast = credits_result
                .cast
                .iter()
                .take(20)
                .map(|c| {
                    debug!("TMDB cast member: id={}, name={}, profile_path={:?}", 
                           c.person.id, c.person.name, c.person.profile_path);
                    CastMember {
                        id: c.person.id as u64,
                        name: c.person.name.clone(),
                        character: c.character.clone(),
                        profile_path: c.person.profile_path.clone(),
                        order: c.order as u32,
                    }
                })
                .collect();

            crew = credits_result
                .crew
                .iter()
                .filter(|c| {
                    matches!(
                        c.job.as_str(),
                        "Director" | "Producer" | "Writer" | "Director of Photography"
                    )
                })
                .take(10)
                .map(|c| CrewMember {
                    id: c.person.id as u64,
                    name: c.person.name.clone(),
                    job: c.job.clone(),
                    department: c.department.clone(),
                    profile_path: c.person.profile_path.clone(),
                })
                .collect();
        }

        // Cache cast profile images
        warn!("Movie {} - Caching profile images for {} cast members", movie_id_str, cast.len());
        let mut cached_count = 0;
        for (idx, cast_member) in cast.iter().enumerate() {
            if let Some(profile_path) = &cast_member.profile_path {
                // Generate a deterministic UUID for the person based on their TMDB ID
                let person_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("person-{}", cast_member.id).as_bytes());
                let person_id_str = person_uuid.to_string();
                
                if let Some((endpoint, _)) = self
                    .cache_image("person", &person_id_str, "profile", 0, profile_path)
                    .await
                {
                    cached_count += 1;
                    info!("Cached profile image for cast member {} ({}): {}", cast_member.name, cast_member.id, endpoint);
                } else {
                    warn!("Failed to cache profile image for cast member {} ({}) with path: {}", cast_member.name, cast_member.id, profile_path);
                }
            } else {
                debug!("Cast member {} ({}) has no profile_path", cast_member.name, cast_member.id);
            }
        }
        warn!("Movie {} - Successfully cached {} cast profile images", movie_id_str, cached_count);

        // Cache crew profile images for important crew members
        let mut crew_cached_count = 0;
        for (idx, crew_member) in crew.iter().enumerate() {
            if let Some(profile_path) = &crew_member.profile_path {
                // Generate a deterministic UUID for the person based on their TMDB ID
                let person_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("person-{}", crew_member.id).as_bytes());
                let person_id_str = person_uuid.to_string();
                
                if let Some((endpoint, _)) = self
                    .cache_image("person", &person_id_str, "profile", 0, profile_path)
                    .await
                {
                    crew_cached_count += 1;
                    info!("Cached profile image for crew member {} ({}): {}", crew_member.name, crew_member.id, endpoint);
                } else {
                    warn!("Failed to cache profile image for crew member {} ({}) with path: {}", crew_member.name, crew_member.id, profile_path);
                }
            } else {
                debug!("Crew member {} ({}) has no profile_path", crew_member.name, crew_member.id);
            }
        }
        warn!("Movie {} - Successfully cached {} crew profile images", movie_id_str, crew_cached_count);

        // Extract genres from the movie details
        let genres = tmdb_details
            .genres
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<String>>();

        // Extract production companies
        let production_companies = tmdb_details
            .production_companies
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<String>>();

        // Create enhanced movie details
        let enhanced_details = media::EnhancedMovieDetails {
            id: tmdb_details.inner.id as u64,
            title: tmdb_details.inner.title.clone(),
            overview: Some(tmdb_details.inner.overview.clone()),
            release_date: tmdb_details
                .inner
                .release_date
                .as_ref()
                .map(|d| d.to_string()),
            runtime: tmdb_details.runtime.map(|r| r as u32),
            vote_average: Some(tmdb_details.inner.vote_average as f32),
            vote_count: Some(tmdb_details.inner.vote_count as u32),
            popularity: Some(tmdb_details.inner.popularity as f32),
            genres,
            production_companies,
            poster_path: cached_posters.first().map(|img| img.endpoint.clone()),
            backdrop_path: cached_backdrops.first().map(|img| img.endpoint.clone()),
            logo_path: cached_logos.first().map(|img| img.endpoint.clone()),
            images: MediaImages {
                posters: cached_posters,
                backdrops: cached_backdrops,
                logos: cached_logos,
                stills: vec![],
            },
            cast,
            crew,
            videos: vec![],                       // TODO: Add video fetching if needed
            keywords: vec![],                     // TODO: Add keywords fetching if needed
            external_ids: ExternalIds::default(), // TODO: Add external IDs fetching if needed
        };

        // Create MovieReference with all data (using pre-generated ID)
        let movie_ref = MovieReference {
            id: movie_id,
            tmdb_id: tmdb_match.tmdb_id,
            title: MovieTitle::new(tmdb_details.inner.title.clone())?,
            details: MediaDetailsOption::Details(TmdbDetails::Movie(enhanced_details)),
            endpoint: MovieURL::from_string(format!("/api/stream/{}", media_file.id)),
            file: media_file,
            theme_color, // Extracted from poster
        };

        // Store the movie reference (which also stores the media file)
        info!(
            "Storing movie reference for: {} (TMDB ID: {}, Library ID: {}, theme_color: {:?})",
            movie_ref.title.as_str(),
            movie_ref.tmdb_id,
            movie_ref.file.library_id,
            movie_ref.theme_color
        );

        self.db
            .backend()
            .store_movie_reference(&movie_ref)
            .await
            .map_err(|e| {
                error!(
                    "Failed to store movie reference: {}. Library ID: {}",
                    e, movie_ref.file.library_id
                );
                e
            })?;

        info!(
            "Successfully stored movie: {} with file: {:?}",
            movie_ref.title.as_str(),
            movie_ref.file.path
        );

        Ok(movie_ref)
    }


    /// Process a series folder
    pub async fn process_series_folder(
        &self,
        series_folder: PathBuf,
        library_id: Uuid,
        output_tx: &mpsc::Sender<ScanOutput>,
    ) -> Result<()> {
        let series_name = series_folder
            .file_name()
            .ok_or_else(|| MediaError::InvalidMedia("Invalid series folder".to_string()))?
            .to_string_lossy()
            .to_string();

        info!("Processing series: {}", series_name);

        // Clean series name for TMDB search
        let clean_name = self.clean_series_name(&series_name);

        // Search TMDB with fuzzy matching
        let series_ref = self.find_or_create_series(&clean_name, library_id, &series_folder).await?;

        // Send series found event immediately
        output_tx
            .send(ScanOutput::SeriesFound(series_ref.clone()))
            .await
            .map_err(|_| MediaError::Cancelled("Output channel closed".to_string()))?;

        // Get the series folder from inventory to find its ID
        let series_folder_str = series_folder.to_string_lossy().to_string();
        let series_folder_info = match self.db.backend().get_folder_by_path(library_id, &series_folder_str).await? {
            Some(folder) => folder,
            None => {
                warn!("Series folder not found in inventory: {}", series_folder_str);
                return Ok(());
            }
        };

        // Query for season folders under this series
        info!("Querying for season folders under series: {} (folder_id: {})", series_name, series_folder_info.id);
        let season_folders = self.db.backend().get_season_folders(series_folder_info.id).await?;
        
        if season_folders.is_empty() {
            info!("No season folders found for series: {}", series_name);
        } else {
            info!("Found {} season folders for series: {}", season_folders.len(), series_name);
            
            // Process each season folder
            for season_folder in season_folders {
                let season_path = PathBuf::from(&season_folder.folder_path);
                
                // Update season folder status to 'scanning'
                if let Err(e) = self.db.backend().update_folder_status(
                    season_folder.id,
                    crate::database::traits::FolderProcessingStatus::Processing,
                    None
                ).await {
                    error!("Failed to update season folder {} status to scanning: {}", season_folder.folder_path, e);
                    continue;
                }
                
                // Process the season folder
                match self.process_season_folder(&season_path, &series_ref, library_id, output_tx).await {
                    Ok(season_ref) => {
                        info!("Successfully processed season {} of {}", season_ref.season_number, series_name);
                        
                        // Update season folder status to 'completed'
                        if let Err(e) = self.db.backend().update_folder_status(
                            season_folder.id,
                            crate::database::traits::FolderProcessingStatus::Completed,
                            None
                        ).await {
                            error!("Failed to update season folder {} status to completed: {}", season_folder.folder_path, e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to process season folder {}: {}", season_folder.folder_path, e);
                        
                        // Update season folder status to 'failed'
                        if let Err(e) = self.db.backend().update_folder_status(
                            season_folder.id,
                            crate::database::traits::FolderProcessingStatus::Failed,
                            Some(e.to_string())
                        ).await {
                            error!("Failed to update season folder {} status to failed: {}", season_folder.folder_path, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Find or create a series reference with TMDB data
    pub async fn find_or_create_series(&self, series_name: &str, library_id: Uuid, series_folder: &Path) -> Result<SeriesReference> {
        info!(
            "SCAN: find_or_create_series called for '{}' in library {}",
            series_name, library_id
        );
        
        // First, check if series already exists by name in this library
        match self.db.backend().find_series_by_name(library_id, series_name).await {
            Ok(Some(existing_series)) => {
                info!(
                    "SCAN: Found existing series '{}' (ID: {}) in library {} with TMDB ID: {}, returning it",
                    existing_series.title.as_str(),
                    existing_series.id.as_str(),
                    library_id,
                    existing_series.tmdb_id
                );
                return Ok(existing_series);
            }
            Ok(None) => {
                info!("SCAN: No existing series found by name '{}' in library {}", series_name, library_id);
            }
            Err(e) => {
                warn!("SCAN: Error checking for existing series by name: {}", e);
            }
        }

        // Rate limit TMDB requests
        self.rate_limit_tmdb().await;

        // Search TMDB
        let tmdb_results = self
            .tmdb_provider
            .search_series(series_name)
            .await
            .map_err(|e| MediaError::Internal(format!("TMDB search failed: {}", e)))?;

        // Fuzzy match to find best result
        let best_match = tmdb_results.first();
        /*
        let best_match = tmdb_results
            .into_iter()
            .map(|result| {
                let score = self
                    .fuzzy_matcher
                    .fuzzy_match(&result.title.as_str(), series_name)
                    .unwrap_or(0);
                (result, score)
            })
            .max_by_key(|(_, score)| *score);

        let tmdb_match = if let Some((result, score)) = best_match {
            if score >= self.config.fuzzy_match_threshold {
                info!(
                    "Found TMDB match for '{}': {} (score: {}, ID: {})",
                    series_name,
                    result.title.as_str(),
                    score,
                    result.tmdb_id
                );
                Some(result)
            } else {
                warn!(
                    "Best TMDB match for '{}' scored too low: {} (score: {})",
                    series_name,
                    result.title.as_str(),
                    score
                );
                None
            }
        } else {
            warn!("No TMDB matches found for series: {}", series_name);
            None
        }; */

        let tmdb_match = best_match;

        // If we found a TMDB match, check if it already exists in the database
        if let Some(matched) = tmdb_match {
            info!("SCAN: Found TMDB match for '{}': TMDB ID {}", series_name, matched.tmdb_id);
            match self.db.backend().get_series_by_tmdb_id(library_id, matched.tmdb_id).await {
                Ok(Some(existing_series)) => {
                    info!(
                        "SCAN: Found existing series by TMDB ID {} (ID: {}) in library {}: '{}', returning it",
                        matched.tmdb_id,
                        existing_series.id.as_str(),
                        library_id,
                        existing_series.title.as_str()
                    );
                    return Ok(existing_series);
                }
                Ok(None) => {
                    info!("SCAN: No existing series found with TMDB ID {} in library {}", matched.tmdb_id, library_id);
                }
                Err(e) => {
                    warn!("SCAN: Error checking for existing series by TMDB ID: {}", e);
                }
            }
        } else {
            info!("SCAN: No TMDB match found for '{}'", series_name);
        }

        // Only generate a new series ID if we're actually creating a new series
        // This is critical - we should NEVER regenerate IDs for existing series
        let series_id = SeriesID::new(Uuid::new_v4().to_string())?;
        info!(
            "SCAN: Creating NEW series for '{}' with generated ID: {} (confirmed: no existing series found)",
            series_name, series_id.as_str()
        );

        // Use match or create placeholder
        let (tmdb_id, enhanced_details, theme_color) = if let Some(matched) = tmdb_match {
            // Fetch full details
            self.rate_limit_tmdb().await;
            let details = self
                .tmdb_provider
                .get_series(matched.tmdb_id)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("Failed to fetch series details: {}", e))
                })?;

            // Fetch additional metadata
            let images = {
                self.rate_limit_tmdb().await;
                self.tmdb_provider
                    .get_series_images(matched.tmdb_id)
                    .await
                    .ok()
            };

            let credits = {
                self.rate_limit_tmdb().await;
                match self.tmdb_provider
                    .get_series_credits(matched.tmdb_id)
                    .await {
                    Ok(credits) => {
                        info!("Successfully fetched series credits for TMDB ID {}", matched.tmdb_id);
                        Some(credits)
                    }
                    Err(e) => {
                        warn!("Failed to fetch series credits for TMDB ID {}: {}", matched.tmdb_id, e);
                        None
                    }
                }
            };

            // Use the pre-generated series ID for image caching
            let series_id_str = series_id.as_str();

            // Cache images and build MediaImages with metadata
            let mut cached_posters = vec![];
            let mut cached_backdrops = vec![];
            let mut cached_logos = vec![];

            // Handle main poster from series details and extract theme color
            let mut theme_color = None;
            if let Some(poster_path) = details.inner.poster_path.as_ref() {
                if let Some((endpoint, extracted_color)) = self
                    .cache_image("series", &series_id_str, "poster", 0, poster_path)
                    .await
                {
                    theme_color = extracted_color;
                    cached_posters.push(media::ImageWithMetadata {
                        endpoint,
                        metadata: media::ImageMetadata {
                            file_path: poster_path.clone(),
                            width: 500,  // Default width for main poster
                            height: 750, // Default height for main poster
                            aspect_ratio: 0.667,
                            iso_639_1: None,
                            vote_average: 0.0,
                            vote_count: 0,
                        },
                    });
                }
            }

            // Handle main backdrop from series details
            if let Some(backdrop_path) = details.inner.backdrop_path.as_ref() {
                if let Some((endpoint, _)) = self
                    .cache_image("series", &series_id_str, "backdrop", 0, backdrop_path)
                    .await
                {
                    cached_backdrops.push(media::ImageWithMetadata {
                        endpoint,
                        metadata: media::ImageMetadata {
                            file_path: backdrop_path.clone(),
                            width: 1920,  // Default width for backdrop
                            height: 1080, // Default height for backdrop
                            aspect_ratio: 1.778,
                            iso_639_1: None,
                            vote_average: 0.0,
                            vote_count: 0,
                        },
                    });
                }
            }

            // Cache additional images from TVShowImagesResult
            if let Some(img_result) = &images {
                // Cache posters (skip index 0 if we already have main poster)
                let start_idx = if cached_posters.is_empty() { 0 } else { 1 };
                for (idx, poster) in img_result.posters.iter().enumerate() {
                    if !poster.file_path.is_empty() {
                        if let Some((endpoint, extracted_color)) = self
                            .cache_image(
                                "series",
                                &series_id_str,
                                "poster",
                                start_idx + idx,
                                &poster.file_path,
                            )
                            .await
                        {
                            // If this is the first poster and we don't have a theme color yet, use it
                            if theme_color.is_none() && start_idx + idx == 0 {
                                theme_color = extracted_color;
                            }
                            cached_posters.push(media::ImageWithMetadata {
                                endpoint,
                                metadata: media::ImageMetadata {
                                    file_path: poster.file_path.clone(),
                                    width: poster.width,
                                    height: poster.height,
                                    aspect_ratio: poster.aspect_ratio,
                                    iso_639_1: poster.iso_639_1.clone(),
                                    vote_average: poster.vote_average,
                                    vote_count: poster.vote_count,
                                },
                            });
                        }
                    }
                }

                // Cache backdrops (skip index 0 if we already have main backdrop)
                let start_idx = if cached_backdrops.is_empty() { 0 } else { 1 };
                for (idx, backdrop) in img_result.backdrops.iter().enumerate() {
                    if !backdrop.file_path.is_empty() {
                        if let Some((endpoint, _)) = self
                            .cache_image(
                                "series",
                                &series_id_str,
                                "backdrop",
                                start_idx + idx,
                                &backdrop.file_path,
                            )
                            .await
                        {
                            cached_backdrops.push(media::ImageWithMetadata {
                                endpoint,
                                metadata: media::ImageMetadata {
                                    file_path: backdrop.file_path.clone(),
                                    width: backdrop.width,
                                    height: backdrop.height,
                                    aspect_ratio: backdrop.aspect_ratio,
                                    iso_639_1: backdrop.iso_639_1.clone(),
                                    vote_average: backdrop.vote_average,
                                    vote_count: backdrop.vote_count,
                                },
                            });
                        }
                    }
                }

                // Cache logos
                for (idx, logo) in img_result.logos.iter().enumerate() {
                    if !logo.file_path.is_empty() {
                        if let Some((endpoint, _)) = self
                            .cache_image("series", &series_id_str, "logo", idx, &logo.file_path)
                            .await
                        {
                            cached_logos.push(media::ImageWithMetadata {
                                endpoint,
                                metadata: media::ImageMetadata {
                                    file_path: logo.file_path.clone(),
                                    width: logo.width,
                                    height: logo.height,
                                    aspect_ratio: logo.aspect_ratio,
                                    iso_639_1: logo.iso_639_1.clone(),
                                    vote_average: logo.vote_average,
                                    vote_count: logo.vote_count,
                                },
                            });
                        }
                    }
                }
            }

            // Extract cast and crew
            let mut cast = vec![];
            let mut crew = vec![];

            if let Some(credits_result) = &credits {
                warn!("Series {} - Processing credits: {} cast, {} crew members", series_id_str, credits_result.cast.len(), credits_result.crew.len());
                cast = credits_result
                    .cast
                    .iter()
                    .take(20)
                    .map(|c| {
                        debug!("TMDB TV cast member: id={}, name={}, profile_path={:?}", 
                               c.inner.id, c.inner.name, c.inner.profile_path);
                        CastMember {
                            id: c.inner.id as u64,
                            name: c.inner.name.clone(),
                            character: c
                                .roles
                                .first()
                                .map(|r| r.character.clone())
                                .unwrap_or_default(),
                            profile_path: c.inner.profile_path.clone(),
                            order: c.order as u32,
                        }
                    })
                    .collect();

                crew = credits_result
                    .crew
                    .iter()
                    .filter(|c| {
                        c.jobs.iter().any(|j| {
                            matches!(
                                j.job.as_str(),
                                "Creator" | "Executive Producer" | "Showrunner"
                            )
                        })
                    })
                    .take(10)
                    .map(|c| CrewMember {
                        id: c.inner.id as u64,
                        name: c.inner.name.clone(),
                        job: c.jobs.first().map(|j| j.job.clone()).unwrap_or_default(),
                        department: c.department.clone(),
                        profile_path: c.inner.profile_path.clone(),
                    })
                    .collect();
            }

            // Cache cast profile images
            warn!("Series {} - Caching profile images for {} cast members", series_id_str, cast.len());
            let mut cached_count = 0;
            for (idx, cast_member) in cast.iter().enumerate() {
                if let Some(profile_path) = &cast_member.profile_path {
                    // Generate a deterministic UUID for the person based on their TMDB ID
                    let person_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("person-{}", cast_member.id).as_bytes());
                    let person_id_str = person_uuid.to_string();
                    
                    if let Some((endpoint, _)) = self
                        .cache_image("person", &person_id_str, "profile", 0, profile_path)
                        .await
                    {
                        cached_count += 1;
                        info!("Cached profile image for TV cast member {} ({}): {}", cast_member.name, cast_member.id, endpoint);
                    } else {
                        warn!("Failed to cache profile image for TV cast member {} ({}) with path: {}", cast_member.name, cast_member.id, profile_path);
                    }
                } else {
                    debug!("TV cast member {} ({}) has no profile_path", cast_member.name, cast_member.id);
                }
            }
            warn!("Series {} - Successfully cached {} cast profile images", series_id_str, cached_count);

            // Cache crew profile images for important crew members
            let mut crew_cached_count = 0;
            for (idx, crew_member) in crew.iter().enumerate() {
                if let Some(profile_path) = &crew_member.profile_path {
                    // Generate a deterministic UUID for the person based on their TMDB ID
                    let person_uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("person-{}", crew_member.id).as_bytes());
                    let person_id_str = person_uuid.to_string();
                    
                    if let Some((endpoint, _)) = self
                        .cache_image("person", &person_id_str, "profile", 0, profile_path)
                        .await
                    {
                        crew_cached_count += 1;
                        info!("Cached profile image for TV crew member {} ({}): {}", crew_member.name, crew_member.id, endpoint);
                    } else {
                        warn!("Failed to cache profile image for TV crew member {} ({}) with path: {}", crew_member.name, crew_member.id, profile_path);
                    }
                } else {
                    debug!("TV crew member {} ({}) has no profile_path", crew_member.name, crew_member.id);
                }
            }
            warn!("Series {} - Successfully cached {} crew profile images", series_id_str, crew_cached_count);

            // Extract genres and networks from TV show details
            let genres = details
                .genres
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<String>>();

            let networks = details
                .networks
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<String>>();

            // Create enhanced series details
            let enhanced = EnhancedSeriesDetails {
                id: details.inner.id as u64,
                name: details.inner.name.clone(),
                overview: details.inner.overview.clone(),
                first_air_date: details.inner.first_air_date.as_ref().map(|d| d.to_string()),
                last_air_date: details.last_air_date.as_ref().map(|d| d.to_string()),
                number_of_seasons: Some(details.number_of_seasons as u32),
                number_of_episodes: details.number_of_episodes.map(|e| e as u32),
                vote_average: Some(details.inner.vote_average as f32),
                vote_count: Some(details.inner.vote_count as u32),
                popularity: Some(details.inner.popularity as f32),
                genres,
                networks,
                poster_path: cached_posters.first().map(|img| img.endpoint.clone()),
                backdrop_path: cached_backdrops.first().map(|img| img.endpoint.clone()),
                logo_path: cached_logos.first().map(|img| img.endpoint.clone()),
                images: MediaImages {
                    posters: cached_posters,
                    backdrops: cached_backdrops,
                    logos: cached_logos,
                    stills: vec![],
                },
                cast,
                crew,
                videos: vec![],   // TODO: Add video fetching if needed
                keywords: vec![], // TODO: Add keywords fetching if needed
                external_ids: ExternalIds::default(), // TODO: Add external IDs fetching if needed
            };

            (matched.tmdb_id, Some(enhanced), theme_color)
        } else {
            // No good match - create placeholder
            (0, None, None)
        };

        // Get folder creation time
        let created_at = series_folder.metadata()
            .ok()
            .and_then(|metadata| {
                metadata.created()
                    .ok()
                    .and_then(|time| {
                        let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                        chrono::DateTime::<chrono::Utc>::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos()
                        )
                    })
                    .or_else(|| {
                        // Fallback to modified time if creation time not available
                        metadata.modified()
                            .ok()
                            .and_then(|time| {
                                let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                                chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    duration.as_secs() as i64,
                                    duration.subsec_nanos()
                                )
                            })
                    })
            })
            .unwrap_or_else(chrono::Utc::now);

        let series_ref = SeriesReference {
            id: series_id,
            library_id,
            tmdb_id,
            title: SeriesTitle::new(
                enhanced_details
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| series_name.to_string()),
            )?,
            details: if let Some(details) = enhanced_details {
                MediaDetailsOption::Details(TmdbDetails::Series(details))
            } else {
                MediaDetailsOption::Endpoint(format!(
                    "/api/series/lookup/{}",
                    series_name.replace(' ', "%20")
                ))
            },
            endpoint: SeriesURL::from_string(format!(
                "/api/series/{}",
                if tmdb_id > 0 {
                    tmdb_id.to_string()
                } else {
                    Uuid::new_v4().to_string()
                }
            )),
            created_at,
            theme_color, // Extracted from poster
        };

        // Store the series reference in the database
        info!(
            "Storing series reference for: {} (TMDB ID: {}, theme_color: {:?})",
            series_ref.title.as_str(),
            series_ref.tmdb_id,
            series_ref.theme_color
        );
        
        self.db
            .backend()
            .store_series_reference(&series_ref)
            .await
            .map_err(|e| {
                error!(
                    "Failed to store series reference: {}. Series: {}",
                    e, series_ref.title.as_str()
                );
                MediaError::Internal(format!("Failed to store series reference: {}", e))
            })?;

        info!(
            "Successfully stored series reference: {} (ID: {}, TMDB: {})",
            series_ref.title.as_str(),
            series_ref.id.as_str(),
            series_ref.tmdb_id
        );

        Ok(series_ref)
    }

    /// Process a season folder
    async fn process_season_folder(
        &self,
        season_folder: &Path,
        series_ref: &SeriesReference,
        library_id: Uuid,
        output_tx: &mpsc::Sender<ScanOutput>,
    ) -> Result<SeasonReference> {
        // Extract season number
        let season_num = self.extract_season_number(season_folder)?;

        info!(
            "Processing season {} of {} (series_id: {})",
            season_num,
            series_ref.title.as_str(),
            series_ref.id.as_str()
        );

        // Get season folder creation time
        let folder_created_at = match season_folder.metadata() {
            Ok(metadata) => {
                metadata.created()
                    .ok()
                    .and_then(|time| {
                        let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                        chrono::DateTime::<chrono::Utc>::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos()
                        )
                    })
                    .unwrap_or_else(|| {
                        // Fallback to modified time if creation time is not available
                        metadata.modified()
                            .ok()
                            .and_then(|time| {
                                let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
                                chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    duration.as_secs() as i64,
                                    duration.subsec_nanos()
                                )
                            })
                            .unwrap_or_else(chrono::Utc::now)
                    })
            }
            Err(e) => {
                warn!("Failed to get season folder metadata for {}: {}, using current time", season_folder.display(), e);
                chrono::Utc::now()
            }
        };

        // Get season details from TMDB if available
        let season_details = if series_ref.tmdb_id > 0 {
            self.rate_limit_tmdb().await;
            self.tmdb_provider
                .get_season(series_ref.tmdb_id, season_num)
                .await
                .ok()
        } else {
            None
        };

        // Generate season ID early for image caching
        let season_id = SeasonID::new(Uuid::new_v4().to_string())?;
        let season_id_str = season_id.as_str().to_string();

        // Process season details and cache poster
        let (enhanced_season, cached_poster_endpoint) = if let Some(details) = season_details.as_ref() {
            let mut cached_poster = None;
            
            // Cache season poster if available
            if let Some(poster_path) = &details.inner.poster_path {
                info!("Caching season {} poster for series {}", season_num, series_ref.title.as_str());
                if let Some((endpoint, _)) = self
                    .cache_image("season", &season_id_str, "poster", 0, poster_path)
                    .await
                {
                    info!("Successfully cached season poster: {}", endpoint);
                    cached_poster = Some(endpoint);
                } else {
                    warn!("Failed to cache season poster");
                }
            }

            let enhanced = SeasonDetails {
                id: details.inner.id as u64,
                season_number: details.inner.season_number as u8,
                name: if details.inner.name.is_empty() {
                    format!("Season {}", season_num)
                } else {
                    details.inner.name.clone()
                },
                overview: details.inner.overview.clone(),
                air_date: details.inner.air_date.as_ref().map(|d| d.to_string()),
                episode_count: details.episodes.len() as u32,
                poster_path: cached_poster.clone(),
            };

            (Some(enhanced), cached_poster)
        } else {
            (None, None)
        };

        let season_ref = SeasonReference {
            id: season_id,
            season_number: SeasonNumber::new(season_num),
            series_id: series_ref.id.clone(), // Link to parent series
            library_id, // Direct library reference (no runtime derivation needed)
            tmdb_series_id: series_ref.tmdb_id,
            details: if let Some(details) = enhanced_season {
                MediaDetailsOption::Details(TmdbDetails::Season(details))
            } else {
                MediaDetailsOption::Endpoint(format!(
                    "/api/series/{}/season/{}",
                    series_ref.tmdb_id, season_num
                ))
            },
            endpoint: SeasonURL::from_string(format!("/api/season/{}", season_id_str)),
            created_at: folder_created_at,
            theme_color: None, // Seasons don't have theme colors
        };

        // Store season in database BEFORE processing episodes to avoid foreign key constraint violation
        info!(
            "SCAN: Storing season reference: ID={} S{} for series '{}' (series_id={})",
            season_ref.id.as_str(),
            season_num,
            series_ref.title.as_str(),
            season_ref.series_id.as_str()
        );
        
        let actual_season_uuid = self.db
            .backend()
            .store_season_reference(&season_ref)
            .await
            .map_err(|e| {
                error!(
                    "Failed to store season reference: {}. Season: {} S{}",
                    e, season_ref.id.as_str(), season_num
                );
                MediaError::Internal(format!("Failed to store season reference: {}", e))
            })?;
        
        // Update season_ref with the actual ID from the database (in case it already existed)
        let mut season_ref = season_ref;
        season_ref.id = SeasonID::new(actual_season_uuid.to_string())?;

        // Send season found event AFTER storing it
        output_tx
            .send(ScanOutput::SeasonFound(season_ref.clone()))
            .await
            .map_err(|_| MediaError::Cancelled("Output channel closed".to_string()))?;

        // Process episodes - now safe because season exists in database
        let episode_files = self.find_episode_files(season_folder).await?;

        for episode_file in episode_files {
            match self
                .process_episode_file(episode_file.clone(), series_ref, &season_ref, library_id)
                .await
            {
                Ok(episode_ref) => {
                    // Send episode found event
                    let _ = output_tx.send(ScanOutput::EpisodeFound(episode_ref)).await;
                }
                Err(e) => {
                    error!("Failed to process episode {:?}: {}", episode_file, e);
                    let _ = output_tx
                        .send(ScanOutput::Error {
                            path: Some(episode_file.display().to_string()),
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        }

        Ok(season_ref)
    }

    /// Process an episode file
    async fn process_episode_file(
        &self,
        file_path: PathBuf,
        series_ref: &SeriesReference,
        season_ref: &SeasonReference,
        library_id: Uuid,
    ) -> Result<EpisodeReference> {
        debug!("Processing episode file: {:?}", file_path);

        // Create MediaFile
        let mut media_file = MediaFile::new(file_path.clone(), library_id)?;

        // Extract metadata
        {
            let mut extractor = self.metadata_extractor.lock().await;
            extractor.set_library_type(Some(LibraryType::TvShows));
            match extractor.extract_metadata(&media_file.path) {
                Ok(metadata) => {
                    media_file.media_file_metadata = Some(metadata);
                }
                Err(e) => {
                    warn!("Failed to extract metadata from {:?}: {}", file_path, e);
                }
            }
        }

        // Parse episode info
        let episode_info = TvParser::parse_episode_info(&file_path).ok_or_else(|| {
            MediaError::InvalidMedia(format!("Cannot parse episode info from: {:?}", file_path))
        })?;

        // Get episode details from TMDB if available
        let episode_details = if series_ref.tmdb_id > 0 {
            self.rate_limit_tmdb().await;
            self.tmdb_provider
                .get_episode(
                    series_ref.tmdb_id,
                    season_ref.season_number.value(),
                    episode_info.episode as u8,
                )
                .await
                .ok()
        } else {
            None
        };

        // Generate episode ID early for image caching
        let episode_id = EpisodeID::new(Uuid::new_v4().to_string())?;
        let episode_id_str = episode_id.as_str().to_string();

        // Process episode details and cache still
        let enhanced_episode = if let Some(details) = episode_details.as_ref() {
            let mut cached_still = None;
            
            // Cache episode still if available
            if let Some(still_path) = &details.inner.still_path {
                info!("Caching episode S{}E{} still for series {}", 
                      season_ref.season_number.value(), 
                      episode_info.episode,
                      series_ref.title.as_str());
                if let Some((endpoint, _)) = self
                    .cache_image("episode", &episode_id_str, "still", 0, still_path)
                    .await
                {
                    info!("Successfully cached episode still: {}", endpoint);
                    cached_still = Some(endpoint);
                } else {
                    warn!("Failed to cache episode still");
                }
            }

            Some(EpisodeDetails {
                id: details.inner.id as u64,
                episode_number: details.inner.episode_number as u8,
                season_number: details.inner.season_number as u8,
                name: details.inner.name.clone(),
                overview: details.inner.overview.clone(),
                air_date: details.inner.air_date.as_ref().map(|d| d.to_string()),
                runtime: None, // Not available in basic episode details
                still_path: cached_still,
                vote_average: Some(details.inner.vote_average as f32),
            })
        } else {
            None
        };

        let episode_ref = EpisodeReference {
            id: episode_id,
            episode_number: EpisodeNumber::new(episode_info.episode as u8),
            season_number: season_ref.season_number,
            season_id: season_ref.id.clone(), // Link to parent season
            series_id: series_ref.id.clone(), // Link to parent series
            tmdb_series_id: series_ref.tmdb_id,
            details: if let Some(details) = enhanced_episode {
                MediaDetailsOption::Details(TmdbDetails::Episode(details))
            } else {
                MediaDetailsOption::Endpoint(format!("/api/episode/lookup/{}", media_file.id))
            },
            endpoint: EpisodeURL::from_string(format!("/api/stream/{}", media_file.id)),
            file: media_file,
        };

        // Store in database and get actual file ID
        let actual_file_id = self.db
            .backend()
            .store_media(episode_ref.file.clone())
            .await?;
        
        // Update episode_ref with the actual file ID (in case it already existed)
        let mut episode_ref = episode_ref;
        episode_ref.file.id = Uuid::parse_str(&actual_file_id)
            .map_err(|e| MediaError::Internal(format!("Invalid file UUID: {}", e)))?;
        
        self.db
            .backend()
            .store_episode_reference(&episode_ref)
            .await?;

        Ok(episode_ref)
    }


    /// Check if a folder name looks like a season folder
    fn is_season_folder(&self, name: &str) -> bool {
        let patterns = [
            r"(?i)^season\s*\d+$",
            r"(?i)^s\d+$",
            r"(?i)^season\s*\d+\s*-",
        ];

        patterns
            .iter()
            .any(|pattern| Regex::new(pattern).unwrap().is_match(name))
    }

    /// Find episode files in a directory
    async fn find_episode_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut episode_files = Vec::new();

        let mut entries = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && self.is_video_file(&path) {
                episode_files.push(path);
            }
        }

        // Sort by filename to maintain episode order
        episode_files.sort();

        Ok(episode_files)
    }

    /// Parse movie folder name to extract title and year
    fn parse_movie_folder_name(&self, folder_name: &str) -> Result<ParsedMovieInfo> {
        // Try to match "Movie Name (Year)" pattern with optional extra info
        // Matches: "Movie Name (2023)" or "Movie Name (2023) (HDR 2160p)"
        let year_pattern = Regex::new(r"^(.+?)\s*\((\d{4})\)(?:\s.+)?\s*$").unwrap();

        if let Some(captures) = year_pattern.captures(folder_name) {
            let title = captures.get(1).unwrap().as_str().trim().to_string();
            let year = captures.get(2).unwrap().as_str().parse().ok();

            Ok(ParsedMovieInfo {
                title,
                year,
                resolution: None,
                source: None,
                release_group: None,
            })
        } else {
            info!("Failed to find year in folder: {}", folder_name);
            // No year in folder name
            Ok(ParsedMovieInfo {
                title: folder_name.trim().to_string(),
                year: None,
                resolution: None,
                source: None,
                release_group: None,
            })
        }
    }

    /// Parse movie filename to extract title and year
    fn parse_movie_filename(&self, filename: &str) -> Result<ParsedMovieInfo> {
        // Common patterns in movie filenames:
        // Movie.Name.2023.HDR.2160p.WEB.H265-GROUP
        // Movie Name (2023) 1080p BluRay
        // Movie.Name.2023.1080p.BluRay.x264-GROUP[tag]

        // First try year in parentheses
        let year_paren_pattern = Regex::new(r"^(.+?)\s*\((\d{4})\)").unwrap();
        if let Some(captures) = year_paren_pattern.captures(filename) {
            let title = captures
                .get(1)
                .unwrap()
                .as_str()
                .replace('.', " ")
                .replace('_', " ")
                .trim()
                .to_string();
            let year = captures.get(2).unwrap().as_str().parse().ok();

            return Ok(ParsedMovieInfo {
                title,
                year,
                resolution: None,
                source: None,
                release_group: None,
            });
        }

        // Try year with dots/spaces
        let year_dot_pattern = Regex::new(r"^(.+?)[\.\s]+(\d{4})[\.\s]").unwrap();
        if let Some(captures) = year_dot_pattern.captures(filename) {
            let title = captures
                .get(1)
                .unwrap()
                .as_str()
                .replace('.', " ")
                .replace('_', " ")
                .trim()
                .to_string();
            let year = captures.get(2).unwrap().as_str().parse().ok();

            return Ok(ParsedMovieInfo {
                title,
                year,
                resolution: None,
                source: None,
                release_group: None,
            });
        }

        // No year found - clean up the filename
        let title = filename
            .split(|c: char| c == '[' || c == '(' || c == '{')
            .next()
            .unwrap_or(filename)
            .replace('.', " ")
            .replace('_', " ")
            .replace('-', " ")
            .trim()
            .to_string();

        // Remove common quality indicators
        let quality_pattern = Regex::new(r"(?i)\s+(1080p|2160p|720p|4k|HDR|BluRay|WEB|HDTV|DVDRip|BRRip|x264|x265|H264|H265|HEVC).*$").unwrap();
        let title = quality_pattern.replace(&title, "").to_string();

        Ok(ParsedMovieInfo {
            title: title.trim().to_string(),
            year: None,
            resolution: None,
            source: None,
            release_group: None,
        })
    }

    /// Clean series name for TMDB search
    fn clean_series_name(&self, name: &str) -> String {
        // Remove year in parentheses
        let year_pattern = Regex::new(r"\s*\(\d{4}\)\s*$").unwrap();
        let cleaned = year_pattern.replace(name, "").to_string();

        // Remove special characters that might interfere with search
        cleaned.replace(['_', '.'], " ").trim().to_string()
    }

    /// Clean movie title for TMDB search - handles Unicode and special characters
    fn clean_movie_title(&self, title: &str) -> String {
        // First normalize unicode characters (NFD normalization)
        // This converts characters like "é" to "e" + combining accent
        let normalized = title
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();

        // Remove or replace problematic characters for search
        normalized
            .replace(['_', '.'], " ")
            .replace(":", " ")
            .replace("'", "")
            .replace("'", "")
            .replace("´", "")
            .replace("̀", "")
            .replace("́", "")
            .replace("̈", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Store a movie without TMDB metadata (fallback for search failures)
    async fn store_movie_without_tmdb(
        &self,
        media_file: MediaFile,
        parsed_info: ParsedMovieInfo,
    ) -> Result<MovieReference> {
        info!(
            "Storing movie '{}' without TMDB metadata (year: {:?})",
            parsed_info.title, parsed_info.year
        );

        // Create a basic movie reference without TMDB data
        let movie_ref = MovieReference {
            id: MovieID::new(Uuid::new_v4().to_string())?,
            tmdb_id: 0, // No TMDB ID
            title: MovieTitle::new(parsed_info.title.clone())?,
            details: MediaDetailsOption::Endpoint(format!("/api/movie/local/{}", media_file.id)),
            endpoint: MovieURL::from_string(format!("/api/stream/{}", media_file.id)),
            file: media_file,
            theme_color: None, // No theme color without poster
        };

        // Store the movie reference
        info!(
            "Storing movie reference without TMDB: {} (Library ID: {})",
            movie_ref.title.as_str(),
            movie_ref.file.library_id
        );

        self.db
            .backend()
            .store_movie_reference(&movie_ref)
            .await
            .map_err(|e| {
                error!(
                    "Failed to store movie reference: {}. Library ID: {}",
                    e, movie_ref.file.library_id
                );
                e
            })?;

        info!(
            "Successfully stored movie without TMDB: {} with file: {:?}",
            movie_ref.title.as_str(),
            movie_ref.file.path
        );

        Ok(movie_ref)
    }

    /// Extract season number from folder name
    fn extract_season_number(&self, path: &Path) -> Result<u8> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| MediaError::InvalidMedia("Invalid season folder".to_string()))?;

        // Match patterns like "Season 1", "Season 01", "S1", "S01"
        let patterns = [
            Regex::new(r"(?i)season\s*(\d+)").unwrap(),
            Regex::new(r"(?i)^s(\d+)").unwrap(),
        ];

        for pattern in &patterns {
            if let Some(captures) = pattern.captures(name) {
                let num: u8 =
                    captures.get(1).unwrap().as_str().parse().map_err(|_| {
                        MediaError::InvalidMedia("Invalid season number".to_string())
                    })?;
                return Ok(num);
            }
        }

        Err(MediaError::InvalidMedia(format!(
            "Cannot parse season number from: {}",
            name
        )))
    }

    /// Find the main video file in a directory (largest file)
    async fn find_main_video_file(&self, folder: &Path) -> Result<PathBuf> {
        let mut largest_file = None;
        let mut largest_size = 0u64;

        let mut entries = tokio::fs::read_dir(folder).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && self.is_video_file(&path) {
                let metadata = entry.metadata().await?;
                if metadata.len() > largest_size {
                    largest_size = metadata.len();
                    largest_file = Some(path);
                }
            }
        }

        largest_file
            .ok_or_else(|| MediaError::NotFound(format!("No video files found in: {:?}", folder)))
    }

    /// Rate limit TMDB API requests
    async fn rate_limit_tmdb(&self) {
        let mut last_request = self.last_tmdb_request.lock().await;
        let elapsed = last_request.elapsed();
        let required_delay = std::time::Duration::from_millis(self.config.tmdb_rate_limit_ms);

        if elapsed < required_delay {
            let sleep_duration = required_delay - elapsed;
            tokio::time::sleep(sleep_duration).await;
        }

        *last_request = Instant::now();
    }
}
