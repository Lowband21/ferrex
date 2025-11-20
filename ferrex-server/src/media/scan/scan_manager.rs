use crate::{media::{metadata_service::MetadataService, prep::thumbnail_service::ThumbnailService}, MediaDatabase};
use axum::response::sse::{Event, KeepAlive, Sse};
use ferrex_core::{
    providers::TmdbApiProvider, LibraryReference, LibraryType, MediaEvent, MediaID, MediaIDLike,
    MediaLike, MediaOps, ScanOutput, ScanProgress, ScanStatus, StreamingScannerConfig,
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
use tracing::{debug, error, info, warn};
use uuid::Uuid;


// Progress update channel
type ProgressSender = mpsc::UnboundedSender<ScanProgress>;
type ProgressReceiver = mpsc::UnboundedReceiver<ScanProgress>;

// Media event channel
type MediaEventSender = mpsc::UnboundedSender<MediaEvent>;
type MediaEventReceiver = mpsc::UnboundedReceiver<MediaEvent>;

pub struct ScanManager {
    active_scans: Arc<RwLock<HashMap<Uuid, ScanProgress>>>,
    scan_history: Arc<RwLock<Vec<ScanProgress>>>,
    progress_channels: Arc<Mutex<HashMap<Uuid, Vec<ProgressSender>>>>,
    media_event_channels: Arc<Mutex<Vec<MediaEventSender>>>,
    db: Arc<MediaDatabase>,
    metadata_service: Arc<MetadataService>,
    thumbnail_service: Arc<ThumbnailService>,
    tmdb_provider: Arc<TmdbApiProvider>,
}

impl ScanManager {
    pub fn new(
        db: Arc<MediaDatabase>,
        metadata_service: Arc<MetadataService>,
        thumbnail_service: Arc<ThumbnailService>,
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
        libraries: Arc<Vec<ferrex_core::Library>>,
        _force_rescan: bool,
    ) -> Result<Uuid, anyhow::Error> {
        let scan_id = Uuid::now_v7();

        debug!(
            "Starting scan {} for libraries: [{}]\nuuids:[{}]",
            scan_id, libraries.iter().map(|library| library.name.clone()).collect::<Vec<String>>().join(", "), libraries.iter().map(|library| library.id.to_string()).collect::<Vec<String>>().join(", ")
        );

        let paths = libraries.iter().flat_map(|library| library.paths.clone()).collect();
        let library_names = libraries.iter().map(|library| library.name.clone()).collect();
        let library_ids = libraries.iter().map(|library| library.id.to_string()).collect();

        // Create initial progress
        let progress = ScanProgress {
            scan_id: scan_id,
            status: ScanStatus::Pending,
            paths,
            library_names,
            library_ids,
            folders_to_scan: 0,
            folders_scanned: 0,
            movies_scanned: 0,
            series_scanned: 0,
            seasons_scanned: 0,
            episodes_scanned: 0,
            skipped_samples: 0,
            errors: vec![],
            current_media: None,
            current_library: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
            estimated_time_remaining: None,
        };

        // Store in active scans
        self.active_scans
            .write()
            .await
            .insert(scan_id, progress.clone());

        // Send media event for scan started
        self.send_media_event(MediaEvent::ScanStarted {
            scan_id,
        })
        .await;

        // Create streaming scanner v2
        let config = StreamingScannerConfig {
            folder_workers: 4,
            batch_size: 100,
            force_refresh: false,
            tmdb_rate_limit_ms: 250,
            fuzzy_match_threshold: 60,
            cache_dir: Some(self.metadata_service.cache_dir().clone()),
            max_error_retries: 3,
            folder_batch_limit: 50, // Folders are processed in batches of 50 to avoid memory issues
                                    // The scanner will loop to process all batches until complete
        };

        let streaming_scanner = Arc::new(StreamingScannerV2::with_config(
            config,
            self.db.clone(),
            self.tmdb_provider.clone(),
        ));

        // Create output channel for scan results
        let (output_tx, mut output_rx) = mpsc::channel(1000);

        // Start the scan
        let scan_manager_in = Arc::new(self.clone());
        let scan_id_clone = scan_id.clone();
        let scanner_clone = streaming_scanner.clone();
        let libraries_arc = libraries.clone();

        tokio::spawn(async move {
            for library in libraries_arc.iter() {
                let library_ref = LibraryReference {
                    id: library.id,
                    name: library.name.clone(),
                    library_type: library.library_type,
                    paths: library.paths.clone(),
                };

                // Update status to scanning
                scan_manager_in
                    .update_progress(&scan_id, |p| {
                        p.status = ScanStatus::Scanning;
                    })
                    .await;

                // Run the scan
                if let Err(e) = scanner_clone.clone().scan_library(library_ref, output_tx.clone()).await {
                    error!("Scan failed: {}", e);
                    scan_manager_in
                        .update_progress(&scan_id, |p| {
                            p.status = ScanStatus::Failed;
                            p.errors.push(e.to_string());
                            p.completed_at = Some(chrono::Utc::now());
                        })
                        .await;
                }
            }
        });




        // Process scan output events
        let scan_manager_out = Arc::new(self.clone());
        let db_out = self.db.clone();

        tokio::spawn(async move {

            while let Some(output) = output_rx.recv().await {
                match output {
                    ScanOutput::MovieFound(movie) => {
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.folders_scanned += 1;
                                p.movies_scanned += 1;
                                p.current_media = Some(movie.title.to_string());
                            })
                            .await;
                        scan_manager_out
                            .send_media_event(MediaEvent::MovieAdded { movie })
                            .await;
                    }
                    ScanOutput::SeriesFound(series) => {
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.folders_scanned += 1;
                                p.series_scanned += 1;
                                p.current_media = Some(series.title.to_string());
                            })
                            .await;
                        scan_manager_out
                            .send_media_event(MediaEvent::SeriesAdded { series })
                            .await;
                    }
                    ScanOutput::SeasonFound(season) => {
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.folders_scanned += 1;
                                p.seasons_scanned += 1;
                            })
                            .await;
                        // Season is already stored in database by streaming_scanner_v2
                        let mut buff1 = Uuid::encode_buffer();
                        let mut buff2 = Uuid::encode_buffer();
                        info!(
                            "Season found: {} S{} for series {}",
                            season.id.as_str(&mut buff1),
                            season.season_number.value(),
                            season.series_id.as_str(&mut buff2)
                        );

                        scan_manager_out
                            .send_media_event(MediaEvent::SeasonAdded { season })
                            .await;
                    }
                    ScanOutput::EpisodeFound(episode) => {
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.episodes_scanned += 1;
                            })
                            .await;

                        let mut buff1 = Uuid::encode_buffer();
                        let mut buff2 = Uuid::encode_buffer();
                        let episode_id_str = episode.id.as_str(&mut buff1);
                        let series_id_str = episode.series_id.as_str(&mut buff2);

                        // Store episode in database
                        match db_out.backend().store_episode_reference(&episode).await { Err(e) => {
                            error!("Failed to store episode reference: {}. Episode: {} S{}E{} for series {}",
                                  e,
                                  episode_id_str,
                                  episode.season_number.value(),
                                  episode.episode_number.value(),
                                  series_id_str
                            );
                        } _ => {
                            info!(
                                "Stored episode {} S{}E{} for series {}",
                                episode_id_str,
                                episode.season_number.value(),
                                episode.episode_number.value(),
                                series_id_str
                            );
                        }}

                        scan_manager_out
                            .send_media_event(MediaEvent::EpisodeAdded { episode })
                            .await;
                    }
                    ScanOutput::ScanProgress {
                        folders_processed,
                        total_folders,
                        ..
                    } => {
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                //p.
                                if total_folders > 0 {
                                    let percent =
                                        (folders_processed as f64 / total_folders as f64) * 100.0;
                                }
                            })
                            .await;
                    }
                    ScanOutput::ScanComplete {
                        duration_secs,
                        ..
                    } => {
                        info!(
                            "Scan {} completed in {}s",
                            scan_id, duration_secs
                        );

                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.status = ScanStatus::Completed;
                                p.completed_at = Some(chrono::Utc::now());
                                p.estimated_time_remaining = None;
                            })
                            .await;

                        // Send scan completed event
                        scan_manager_out
                            .send_media_event(MediaEvent::ScanCompleted {
                                scan_id: scan_id.clone(),
                            })
                            .await;

                        // Move to history
                        if let Some(progress) = scan_manager_out
                            .active_scans
                            .write()
                            .await
                            .remove(&scan_id)
                        {
                            scan_manager_out.scan_history.write().await.push(progress);
                        }
                    }
                    ScanOutput::Error { path, error } => {
                        let error_msg = if let Some(p) = path {
                            format!("{}: {}", p, error)
                        } else {
                            error
                        };
                        scan_manager_out
                            .update_progress(&scan_id, |p| {
                                p.errors.push(error_msg);
                            })
                            .await;
                    }
                }
            }

            info!("Scan {} output processing completed", scan_id);
        });

        Ok(scan_id)
    }

    /// Clean up all files that no longer exist on disk
    async fn cleanup_all_deleted_files(&self, scan_id: &Uuid) -> Result<usize, anyhow::Error> {
        info!("Checking for deleted files across entire library");

        let all_media = self.db.backend().get_all_media().await?;
        let mut deleted_count = 0;

        for media_file in all_media {
            if !media_file.path.exists() {
                info!(
                    "File no longer exists, removing from database: {:?}",
                    media_file.path
                );

                // Determine the MediaID based on library type before deletion
                let media_id = match self.db.backend().get_library(&media_file.library_id).await
                { Ok(Some(library)) => {
                    match library.library_type {
                        ferrex_core::LibraryType::Movies => {
                            // Query for movie reference with this file_id
                            match self
                                .db
                                .backend()
                                .get_library_media_references(
                                    media_file.library_id,
                                    LibraryType::Movies,
                                )
                                .await
                            { Ok(movies) => {
                                movies
                                    .iter()
                                    .find(|m| (*m).as_movie().unwrap().file.id == media_file.id)
                                    .map(|m| m.media_id())
                            } _ => {
                                None
                            }}
                        }
                        ferrex_core::LibraryType::Series => {
                            // For TV shows, find the episode with this file
                            let mut found_episode = None;
                            if let Ok(series_list) = self
                                .db
                                .backend()
                                .get_library_media_references(
                                    media_file.library_id,
                                    LibraryType::Series,
                                )
                                .await
                            {
                                for series in series_list {
                                    if let Ok(seasons) = self
                                        .db
                                        .backend()
                                        .get_series_seasons(&series.as_series().unwrap().id)
                                        .await
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
                                                    .as_ref()
                                                {
                                                    found_episode =
                                                        Some(MediaID::Episode(episode.id));
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
                } _ => {
                    None
                }};

                match self
                    .db
                    .backend()
                    .delete_media(&media_file.id.to_string())
                    .await
                { Err(e) => {
                    warn!("Failed to delete media {}: {}", media_file.id, e);
                } _ => {
                    deleted_count += 1;
                    // Send media deleted event if we found the MediaID
                    if let Some(id) = media_id {
                        self.send_media_event(MediaEvent::MediaDeleted { id: id })
                            .await;
                    } else {
                        warn!(
                            "Could not determine MediaID for deleted file: {:?}",
                            media_file.path
                        );
                    }
                }}
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
    async fn update_progress<F>(&self, scan_id: &Uuid, updater: F)
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
    async fn is_scan_cancelled(&self, scan_id: &Uuid) -> bool {
        let scans = self.active_scans.read().await;
        scans
            .get(scan_id)
            .map(|p| p.status == ScanStatus::Cancelled)
            .unwrap_or(false)
    }

    /// Get current progress for a scan
    pub async fn get_scan_progress(&self, scan_id: &Uuid) -> Option<ScanProgress> {
        self.active_scans.read().await.get(scan_id).cloned()
    }

    /// Subscribe to progress updates for a specific scan
    pub async fn subscribe_to_progress(&self, scan_id: Uuid) -> ProgressReceiver {
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
    pub async fn cancel_scan(&self, scan_id: &Uuid) -> Result<(), anyhow::Error> {
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
    _scan_id: Uuid,
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
