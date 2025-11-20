use crate::MediaDatabase;
use axum::response::sse::{Event, KeepAlive, Sse};
use ferrex_core::{
    providers::TmdbApiProvider,
    LibraryReference,
    MediaEvent,
    MediaId,
    ScanOutput,
    // Import types from api_types
    ScanProgress,
    ScanStatus,
    StreamingScannerConfig,
    StreamingScannerV2,
};
use futures::stream::{self};
use futures_util::stream::Stream;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{
    mpsc::{self},
    Mutex, RwLock,
};
use tracing::{error, info, warn};
use uuid::Uuid;

// These types are now in ferrex_core::api_types
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct ScanProgress {
//     pub scan_id: String,
//     pub status: ScanStatus,
//     pub path: String,
//     pub library_name: Option<String>, // Name of the library being scanned
//     pub library_id: Option<String>,   // ID of the library being scanned
//     pub total_files: usize,
//     pub scanned_files: usize,
//     pub stored_files: usize,
//     pub metadata_fetched: usize,
//     pub skipped_samples: usize,
//     pub errors: Vec<String>,
//     pub current_file: Option<String>,
//     pub started_at: chrono::DateTime<chrono::Utc>,
//     pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
//     pub estimated_time_remaining: Option<Duration>,
// }

// #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "lowercase")]
// pub enum ScanStatus {
//     Pending,
//     Scanning,
//     Processing,
//     Completed,
//     Failed,
//     Cancelled,
// }

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct ScanRequest {
//     pub path: Option<String>,
//     pub max_depth: Option<usize>,
//     pub follow_links: bool,
//     pub extract_metadata: bool,
//     pub force_rescan: bool,
//     pub paths: Option<Vec<String>>, // Multiple paths support
//     pub library_id: Option<Uuid>,
//     pub library_type: Option<ferrex_core::LibraryType>,
// }

// // Media event types for SSE
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(tag = "type", rename_all = "snake_case")]
// pub enum MediaEvent {
//     MediaAdded { media: MediaFile },
//     MediaUpdated { media: MediaFile },
//     MediaDeleted { id: String },
//     MetadataUpdated { id: String },
//     TvShowAdded { show: ferrex_core::TvShowDetails },
//     TvShowUpdated { show: ferrex_core::TvShowDetails },
//     ScanStarted { scan_id: String },
//     ScanCompleted { scan_id: String },
// }

// Progress update channel
type ProgressSender = mpsc::UnboundedSender<ScanProgress>;
type ProgressReceiver = mpsc::UnboundedReceiver<ScanProgress>;

// Media event channel
type MediaEventSender = mpsc::UnboundedSender<MediaEvent>;
type MediaEventReceiver = mpsc::UnboundedReceiver<MediaEvent>;

pub struct ScanManager {
    active_scans: Arc<RwLock<HashMap<String, ScanProgress>>>,
    scan_history: Arc<RwLock<Vec<ScanProgress>>>,
    progress_channels: Arc<Mutex<HashMap<String, Vec<ProgressSender>>>>,
    media_event_channels: Arc<Mutex<Vec<MediaEventSender>>>,
    db: Arc<MediaDatabase>,
    metadata_service: Arc<crate::metadata_service::MetadataService>,
    thumbnail_service: Arc<crate::thumbnail_service::ThumbnailService>,
    tmdb_provider: Arc<TmdbApiProvider>,
}

impl ScanManager {
    pub fn new(
        db: Arc<MediaDatabase>,
        metadata_service: Arc<crate::metadata_service::MetadataService>,
        thumbnail_service: Arc<crate::thumbnail_service::ThumbnailService>,
    ) -> Self {
        // Create TmdbApiProvider
        let tmdb_provider = Arc::new(TmdbApiProvider::new());

        Self {
            active_scans: Arc::new(RwLock::new(HashMap::new())),
            scan_history: Arc::new(RwLock::new(Vec::new())),
            progress_channels: Arc::new(Mutex::new(HashMap::new())),
            media_event_channels: Arc::new(Mutex::new(Vec::new())),
            db,
            metadata_service,
            thumbnail_service,
            tmdb_provider,
        }
    }

    /// Start a streaming scan for a library
    pub async fn start_library_scan(
        &self,
        library: Arc<ferrex_core::Library>,
        _force_rescan: bool,
    ) -> Result<String, anyhow::Error> {
        let scan_id = Uuid::new_v4().to_string();

        info!(
            "Starting streaming scan {} for library: {} ({})",
            scan_id, library.name, library.id
        );

        // Verify the library exists in the database before starting scan
        match self.db.backend().get_library(&library.id.to_string()).await {
            Ok(Some(_)) => {
                info!("Library {} verified in database", library.id);
            }
            Ok(None) => {
                error!(
                    "Library {} does not exist in database! Cannot start scan.",
                    library.id
                );
                return Err(anyhow::anyhow!(
                    "Library {} does not exist in database. Please create the library first.",
                    library.id
                ));
            }
            Err(e) => {
                error!("Failed to verify library {}: {}", library.id, e);
                return Err(anyhow::anyhow!("Failed to verify library existence: {}", e));
            }
        }

        // Create initial progress
        let progress = ScanProgress {
            scan_id: scan_id.clone(),
            status: ScanStatus::Pending,
            path: library
                .paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            library_name: Some(library.name.clone()),
            library_id: Some(library.id.to_string()),
            total_files: 0,
            scanned_files: 0,
            stored_files: 0,
            metadata_fetched: 0,
            skipped_samples: 0,
            errors: vec![],
            current_file: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
            estimated_time_remaining: None,
        };

        // Store in active scans
        self.active_scans
            .write()
            .await
            .insert(scan_id.clone(), progress.clone());

        // Send media event for scan started
        self.send_media_event(MediaEvent::ScanStarted {
            scan_id: scan_id.clone(),
        })
        .await;

        // Create streaming scanner v2
        let config = StreamingScannerConfig {
            folder_workers: 4,
            batch_size: 100,
            tmdb_rate_limit_ms: 250,
            fuzzy_match_threshold: 60,
            cache_dir: Some(self.metadata_service.cache_dir().clone()),
            max_error_retries: 3,
            folder_batch_limit: 50, // Folders are processed in batches of 50 to avoid memory issues
                                    // The scanner will loop to process all batches until complete
        };

        // Convert Library to LibraryReference
        info!(
            "Converting Library to LibraryReference with ID: {}",
            library.id
        );
        let library_ref = LibraryReference {
            id: library.id,
            name: library.name.clone(),
            library_type: library.library_type,
            paths: library.paths.clone(),
        };
        info!("LibraryReference created with ID: {}", library_ref.id);

        let streaming_scanner = Arc::new(StreamingScannerV2::with_config(
            config,
            self.db.clone(),
            self.tmdb_provider.clone(),
        ));

        // Create output channel for scan results
        let (output_tx, mut output_rx) = mpsc::channel(1000);

        // Start the scan
        let scan_manager = Arc::new(self.clone());
        let scan_id_clone = scan_id.clone();
        let scanner_clone = streaming_scanner.clone();

        tokio::spawn(async move {
            // Update status to scanning
            scan_manager
                .update_progress(&scan_id_clone, |p| {
                    p.status = ScanStatus::Scanning;
                })
                .await;

            // Run the scan
            if let Err(e) = scanner_clone.scan_library(library_ref, output_tx).await {
                error!("Scan failed: {}", e);
                scan_manager
                    .update_progress(&scan_id_clone, |p| {
                        p.status = ScanStatus::Failed;
                        p.errors.push(e.to_string());
                        p.completed_at = Some(chrono::Utc::now());
                    })
                    .await;
            }
        });

        // Process scan output events
        let scan_manager_2 = Arc::new(self.clone());
        let scan_id_clone_2 = scan_id.clone();
        let db_2 = self.db.clone();

        tokio::spawn(async move {
            let mut movies_found = 0;
            let mut series_found = 0;
            let mut episodes_found = 0;

            while let Some(output) = output_rx.recv().await {
                match output {
                    ScanOutput::MovieFound(movie) => {
                        movies_found += 1;
                        scan_manager_2
                            .update_progress(&scan_id_clone_2, |p| {
                                p.scanned_files += 1;
                                p.stored_files += 1;
                            })
                            .await;
                        scan_manager_2
                            .send_media_event(MediaEvent::MovieAdded { movie })
                            .await;
                    }
                    ScanOutput::SeriesFound(series) => {
                        series_found += 1;
                        scan_manager_2
                            .send_media_event(MediaEvent::SeriesAdded { series })
                            .await;
                    }
                    ScanOutput::SeasonFound(season) => {
                        // Season is already stored in database by streaming_scanner_v2
                        info!(
                            "Season found: {} S{} for series {}",
                            season.id.as_str(),
                            season.season_number.value(),
                            season.series_id.as_str()
                        );

                        scan_manager_2
                            .send_media_event(MediaEvent::SeasonAdded { season })
                            .await;
                    }
                    ScanOutput::EpisodeFound(episode) => {
                        episodes_found += 1;

                        // Store episode in database
                        if let Err(e) = db_2.backend().store_episode_reference(&episode).await {
                            error!("Failed to store episode reference: {}. Episode: {} S{}E{} for series {}",
                                  e,
                                  episode.id.as_str(),
                                  episode.season_number.value(),
                                  episode.episode_number.value(),
                                  episode.series_id.as_str());
                        } else {
                            info!(
                                "Stored episode {} S{}E{} for series {}",
                                episode.id.as_str(),
                                episode.season_number.value(),
                                episode.episode_number.value(),
                                episode.series_id.as_str()
                            );
                        }

                        scan_manager_2
                            .update_progress(&scan_id_clone_2, |p| {
                                p.scanned_files += 1;
                                p.stored_files += 1;
                            })
                            .await;
                        scan_manager_2
                            .send_media_event(MediaEvent::EpisodeAdded { episode })
                            .await;
                    }
                    ScanOutput::ScanProgress {
                        folders_processed,
                        total_folders,
                        ..
                    } => {
                        scan_manager_2
                            .update_progress(&scan_id_clone_2, |p| {
                                if total_folders > 0 {
                                    let percent =
                                        (folders_processed as f64 / total_folders as f64) * 100.0;
                                    p.current_file = Some(format!(
                                        "Processing folders: {}/{} ({:.1}%)",
                                        folders_processed, total_folders, percent
                                    ));
                                }
                            })
                            .await;
                    }
                    ScanOutput::ScanComplete {
                        movies_found: m,
                        series_found: s,
                        episodes_found: e,
                        duration_secs,
                        ..
                    } => {
                        info!(
                            "Scan {} completed: {} movies, {} series, {} episodes in {}s",
                            scan_id_clone_2, m, s, e, duration_secs
                        );

                        scan_manager_2
                            .update_progress(&scan_id_clone_2, |p| {
                                p.status = ScanStatus::Completed;
                                p.completed_at = Some(chrono::Utc::now());
                                p.total_files = m + e; // Movies + Episodes have files
                                p.current_file = None;
                                p.estimated_time_remaining = None;
                            })
                            .await;

                        // Send scan completed event
                        scan_manager_2
                            .send_media_event(MediaEvent::ScanCompleted {
                                scan_id: scan_id_clone_2.clone(),
                            })
                            .await;

                        // Move to history
                        if let Some(progress) = scan_manager_2
                            .active_scans
                            .write()
                            .await
                            .remove(&scan_id_clone_2)
                        {
                            scan_manager_2.scan_history.write().await.push(progress);
                        }
                    }
                    ScanOutput::Error { path, error } => {
                        let error_msg = if let Some(p) = path {
                            format!("{}: {}", p, error)
                        } else {
                            error
                        };
                        scan_manager_2
                            .update_progress(&scan_id_clone_2, |p| {
                                p.errors.push(error_msg);
                            })
                            .await;
                    }
                }
            }

            info!("Scan {} output processing completed", scan_id_clone_2);
        });

        Ok(scan_id)
    }

    /// Clean up all files that no longer exist on disk
    async fn cleanup_all_deleted_files(&self, scan_id: &str) -> Result<usize, anyhow::Error> {
        info!("Checking for deleted files across entire library");

        let all_media = self.db.backend().get_all_media().await?;
        let mut deleted_count = 0;

        for media_file in all_media {
            if !media_file.path.exists() {
                info!(
                    "File no longer exists, removing from database: {:?}",
                    media_file.path
                );

                // Determine the MediaId based on library type before deletion
                let media_id = if let Ok(Some(library)) = self
                    .db
                    .backend()
                    .get_library(&media_file.library_id.to_string())
                    .await
                {
                    match library.library_type {
                        ferrex_core::LibraryType::Movies => {
                            // Query for movie reference with this file_id
                            if let Ok(movies) = self
                                .db
                                .backend()
                                .get_library_movies(media_file.library_id)
                                .await
                            {
                                movies
                                    .iter()
                                    .find(|m| m.file.id == media_file.id)
                                    .map(|m| MediaId::Movie(m.id.clone()))
                            } else {
                                None
                            }
                        }
                        ferrex_core::LibraryType::TvShows => {
                            // For TV shows, find the episode with this file
                            let mut found_episode = None;
                            if let Ok(series_list) = self
                                .db
                                .backend()
                                .get_library_series(media_file.library_id)
                                .await
                            {
                                for series in series_list {
                                    if let Ok(seasons) =
                                        self.db.backend().get_series_seasons(&series.id).await
                                    {
                                        for season in seasons {
                                            if let Ok(episodes) = self
                                                .db
                                                .backend()
                                                .get_season_episodes(&season.id)
                                                .await
                                            {
                                                if let Some(episode) = episodes
                                                    .iter()
                                                    .find(|e| e.file.id == media_file.id)
                                                {
                                                    found_episode =
                                                        Some(MediaId::Episode(episode.id.clone()));
                                                    break;
                                                }
                                            }
                                        }
                                        if found_episode.is_some() {
                                            break;
                                        }
                                    }
                                }
                            }
                            found_episode
                        }
                    }
                } else {
                    None
                };

                if let Err(e) = self
                    .db
                    .backend()
                    .delete_media(&media_file.id.to_string())
                    .await
                {
                    warn!("Failed to delete media {}: {}", media_file.id, e);
                } else {
                    deleted_count += 1;
                    // Send media deleted event if we found the MediaId
                    if let Some(id) = media_id {
                        self.send_media_event(MediaEvent::MediaDeleted { id }).await;
                    } else {
                        warn!(
                            "Could not determine MediaId for deleted file: {:?}",
                            media_file.path
                        );
                    }
                }
            }
        }

        if deleted_count > 0 {
            info!("Removed {} deleted files from database", deleted_count);
            self.update_progress(scan_id, |p| {
                p.errors.push(format!(
                    "Removed {} deleted files from database",
                    deleted_count
                ));
            })
            .await;
        }

        Ok(deleted_count)
    }

    /// Update scan progress
    async fn update_progress<F>(&self, scan_id: &str, updater: F)
    where
        F: FnOnce(&mut ScanProgress),
    {
        let mut scans = self.active_scans.write().await;
        if let Some(progress) = scans.get_mut(scan_id) {
            updater(progress);

            // Send to all subscribers
            let channels = self.progress_channels.lock().await;
            if let Some(senders) = channels.get(scan_id) {
                let progress_clone = progress.clone();
                for sender in senders {
                    let _ = sender.send(progress_clone.clone());
                }
            }
        }
    }

    /// Check if a scan has been cancelled
    async fn is_scan_cancelled(&self, scan_id: &str) -> bool {
        let scans = self.active_scans.read().await;
        scans
            .get(scan_id)
            .map(|p| p.status == ScanStatus::Cancelled)
            .unwrap_or(false)
    }

    /// Get current progress for a scan
    pub async fn get_scan_progress(&self, scan_id: &str) -> Option<ScanProgress> {
        self.active_scans.read().await.get(scan_id).cloned()
    }

    /// Subscribe to progress updates for a specific scan
    pub async fn subscribe_to_progress(&self, scan_id: String) -> ProgressReceiver {
        let (tx, rx) = mpsc::unbounded_channel();

        // Send current progress if it exists
        if let Some(progress) = self.active_scans.read().await.get(&scan_id).cloned() {
            let _ = tx.send(progress);
        }

        let mut channels = self.progress_channels.lock().await;
        channels.entry(scan_id).or_insert_with(Vec::new).push(tx);

        rx
    }

    /// Subscribe to media events
    pub async fn subscribe_to_media_events(&self) -> MediaEventReceiver {
        let (tx, rx) = mpsc::unbounded_channel();

        let mut channels = self.media_event_channels.lock().await;
        channels.push(tx);

        rx
    }

    /// Send a media event to all subscribers
    pub async fn send_media_event(&self, event: MediaEvent) {
        let mut channels = self.media_event_channels.lock().await;

        // Remove any closed channels
        channels.retain(|sender| sender.send(event.clone()).is_ok());
    }

    /// Get active scans
    pub async fn get_active_scans(&self) -> Vec<ScanProgress> {
        self.active_scans.read().await.values().cloned().collect()
    }

    /// Get scan history
    pub async fn get_scan_history(&self, limit: usize) -> Vec<ScanProgress> {
        let history = self.scan_history.read().await;
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Cancel a scan
    pub async fn cancel_scan(&self, scan_id: &str) -> Result<(), anyhow::Error> {
        self.update_progress(scan_id, |p| {
            p.status = ScanStatus::Cancelled;
            p.completed_at = Some(chrono::Utc::now());
        })
        .await;

        Ok(())
    }
}

// Clone implementation for ScanManager
impl Clone for ScanManager {
    fn clone(&self) -> Self {
        Self {
            active_scans: self.active_scans.clone(),
            scan_history: self.scan_history.clone(),
            progress_channels: self.progress_channels.clone(),
            media_event_channels: self.media_event_channels.clone(),
            db: self.db.clone(),
            metadata_service: self.metadata_service.clone(),
            thumbnail_service: self.thumbnail_service.clone(),
            tmdb_provider: self.tmdb_provider.clone(),
        }
    }
}

/// Create SSE stream for scan progress
pub fn scan_progress_sse(
    _scan_id: String,
    receiver: ProgressReceiver,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>>> {
    let stream = stream::unfold(receiver, move |mut receiver| async move {
        match receiver.recv().await {
            Some(progress) => {
                let event = Event::default()
                    .event("progress")
                    .json_data(&progress)
                    .map_err(Into::into);
                Some((event, receiver))
            }
            None => None,
        }
    });

    // Add keepalive to prevent connection timeout
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keepalive"),
    )
}

/// Create SSE stream for media events
pub fn media_events_sse(
    receiver: MediaEventReceiver,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>>> {
    let stream = stream::unfold(receiver, move |mut receiver| async move {
        match receiver.recv().await {
            Some(event) => {
                let sse_event = Event::default()
                    .event("media_event")
                    .json_data(&event)
                    .map_err(Into::into);
                Some((sse_event, receiver))
            }
            None => None,
        }
    });

    // Add keepalive to prevent connection timeout
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keepalive"),
    )
}
