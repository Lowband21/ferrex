use crate::MediaDatabase;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, StreamExt};
use futures_util::stream::Stream;
use ferrex_core::{MediaFile, MediaScanner, MetadataExtractor, StreamingScanner, StreamingScannerConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_id: String,
    pub status: ScanStatus,
    pub path: String,
    pub total_files: usize,
    pub scanned_files: usize,
    pub stored_files: usize,
    pub metadata_fetched: usize,
    pub skipped_samples: usize,
    pub errors: Vec<String>,
    pub current_file: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub estimated_time_remaining: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Pending,
    Scanning,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    pub path: Option<String>,
    pub max_depth: Option<usize>,
    pub follow_links: bool,
    pub extract_metadata: bool,
    pub force_rescan: bool,
    pub paths: Option<Vec<String>>, // Multiple paths support
    pub library_id: Option<Uuid>,
    pub library_type: Option<ferrex_core::LibraryType>,
    pub use_streaming: bool, // Use new streaming scanner
}

// Media event types for SSE
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaEvent {
    MediaAdded { media: MediaFile },
    MediaUpdated { media: MediaFile },
    MediaDeleted { id: String },
    MetadataUpdated { id: String },
    ScanStarted { scan_id: String },
    ScanCompleted { scan_id: String },
}

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
}

impl ScanManager {
    pub fn new(
        db: Arc<MediaDatabase>,
        metadata_service: Arc<crate::metadata_service::MetadataService>,
        thumbnail_service: Arc<crate::thumbnail_service::ThumbnailService>,
    ) -> Self {
        Self {
            active_scans: Arc::new(RwLock::new(HashMap::new())),
            scan_history: Arc::new(RwLock::new(Vec::new())),
            progress_channels: Arc::new(Mutex::new(HashMap::new())),
            media_event_channels: Arc::new(Mutex::new(Vec::new())),
            db,
            metadata_service,
            thumbnail_service,
        }
    }

    /// Start a new scan in the background
    pub async fn start_scan(&self, request: ScanRequest) -> Result<String, anyhow::Error> {
        let scan_id = Uuid::new_v4().to_string();

        // Determine paths to scan
        let paths = if let Some(paths) = request.paths.clone() {
            paths
        } else if let Some(path) = request.path.clone() {
            vec![path]
        } else {
            // Use MEDIA_ROOT if no paths provided
            match std::env::var("MEDIA_ROOT") {
                Ok(path) => vec![path],
                Err(_) => return Err(anyhow::anyhow!("No paths provided and MEDIA_ROOT not set")),
            }
        };

        // Create initial progress
        let progress = ScanProgress {
            scan_id: scan_id.clone(),
            status: ScanStatus::Pending,
            path: paths.join(", "),
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

        // Spawn background scan task
        let scan_manager = Arc::new(self.clone());
        let scan_id_clone = scan_id.clone();

        tokio::spawn(async move {
            if let Err(e) = scan_manager
                .execute_scan(scan_id_clone.clone(), request, paths)
                .await
            {
                error!("Scan failed: {}", e);
                scan_manager
                    .update_progress(&scan_id_clone, |p| {
                        p.status = ScanStatus::Failed;
                        p.errors.push(format!("Scan failed: {}", e));
                        p.completed_at = Some(chrono::Utc::now());
                    })
                    .await;
            }
        });

        Ok(scan_id)
    }

    /// Start a streaming scan for a library
    pub async fn start_library_scan(&self, library: Arc<ferrex_core::Library>, force_rescan: bool) -> Result<String, anyhow::Error> {
        let scan_id = Uuid::new_v4().to_string();
        
        info!("Starting streaming scan {} for library: {} ({})", scan_id, library.name, library.id);
        
        // Create initial progress
        let progress = ScanProgress {
            scan_id: scan_id.clone(),
            status: ScanStatus::Pending,
            path: library.paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
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
        
        // Create streaming scanner
        let config = StreamingScannerConfig {
            folder_workers: 4,
            file_workers: 8,
            batch_size: 100,
            progress_buffer: 1000,
            extract_metadata: true,
            fetch_external_metadata: true,
            generate_thumbnails: true,
        };
        
        let streaming_scanner = Arc::new(StreamingScanner::with_config(config, self.db.clone()));
        let scan_handle = streaming_scanner.scan_library(library.clone(), force_rescan);
        
        // Convert streaming progress to our format and relay
        let scan_manager = Arc::new(self.clone());
        let scan_id_clone = scan_id.clone();
        let scan_handle_id = scan_handle.scan_id();
        
        tokio::spawn(async move {
            let mut progress_stream = scan_handle.progress_stream();
            
            while let Some(progress_event) = progress_stream.next().await {
                match progress_event {
                    ferrex_core::ScanProgress::ScanStarted { .. } => {
                        scan_manager.update_progress(&scan_id_clone, |p| {
                            p.status = ScanStatus::Scanning;
                        }).await;
                    }
                    ferrex_core::ScanProgress::FileScanned { filename, current, total_estimate, .. } => {
                        scan_manager.update_progress(&scan_id_clone, |p| {
                            p.current_file = Some(filename);
                            p.scanned_files = current;
                            p.total_files = total_estimate;
                            p.stored_files = current; // Assuming stored equals scanned
                        }).await;
                    }
                    ferrex_core::ScanProgress::MetadataExtracted { .. } => {
                        scan_manager.update_progress(&scan_id_clone, |p| {
                            p.metadata_fetched += 1;
                        }).await;
                    }
                    ferrex_core::ScanProgress::Error { error, .. } => {
                        scan_manager.update_progress(&scan_id_clone, |p| {
                            p.errors.push(error);
                        }).await;
                    }
                    ferrex_core::ScanProgress::ScanCompleted { total_files, duration_secs: _, .. } => {
                        scan_manager.update_progress(&scan_id_clone, |p| {
                            p.status = ScanStatus::Completed;
                            p.completed_at = Some(chrono::Utc::now());
                            p.total_files = total_files;
                            p.current_file = None;
                            p.estimated_time_remaining = None;
                        }).await;
                        
                        // Send scan completed event
                        scan_manager.send_media_event(MediaEvent::ScanCompleted {
                            scan_id: scan_id_clone.clone(),
                        }).await;
                        
                        // Move to history
                        if let Some(progress) = scan_manager.active_scans.write().await.remove(&scan_id_clone) {
                            scan_manager.scan_history.write().await.push(progress);
                        }
                    }
                    _ => {}
                }
            }
            
            info!("Streaming scan {} completed", scan_handle_id);
        });
        
        Ok(scan_id)
    }

    /// Execute the actual scan
    async fn execute_scan(
        &self,
        scan_id: String,
        request: ScanRequest,
        paths: Vec<String>,
    ) -> Result<(), anyhow::Error> {
        info!("Starting scan {} for paths: {:?}", scan_id, paths);

        self.update_progress(&scan_id, |p| {
            p.status = ScanStatus::Scanning;
        })
        .await;

        let start_time = std::time::Instant::now();
        let mut all_files = Vec::new();

        // Phase 1: Scan directories
        for path in &paths {
            let mut scanner = MediaScanner::new();
            if let Some(depth) = request.max_depth {
                scanner = scanner.with_max_depth(depth);
            }
            scanner = scanner.with_follow_links(request.follow_links);
            
            // Add library context if available
            if let (Some(library_id), Some(library_type)) = (request.library_id, request.library_type.clone()) {
                scanner = scanner.with_library(library_id, library_type);
            }

            match scanner.scan_directory(path) {
                Ok(result) => {
                    info!("Found {} video files in {}", result.video_files.len(), path);
                    all_files.extend(result.video_files);

                    if !result.errors.is_empty() {
                        self.update_progress(&scan_id, |p| {
                            p.errors.extend(result.errors);
                        })
                        .await;
                    }
                }
                Err(e) => {
                    warn!("Failed to scan {}: {}", path, e);
                    self.update_progress(&scan_id, |p| {
                        p.errors.push(format!("Failed to scan {}: {}", path, e));
                    })
                    .await;
                }
            }
        }

        let total_files = all_files.len();
        self.update_progress(&scan_id, |p| {
            p.total_files = total_files;
            p.status = ScanStatus::Processing;
        })
        .await;

        // Phase 2: Clean up deleted files if force rescan
        if request.force_rescan {
            info!("Force rescan enabled, cleaning up deleted files");
            if let Err(e) = self.cleanup_all_deleted_files(&scan_id).await {
                warn!("Failed to cleanup deleted files: {}", e);
            }
        }

        // Phase 3: Process files concurrently
        let extractor = if request.extract_metadata {
            if let Some(library_type) = request.library_type.clone() {
                Some(MetadataExtractor::with_library_type(library_type))
            } else {
                Some(MetadataExtractor::new())
            }
        } else {
            None
        };

        // Concurrency configuration for metadata fetching
        const MAX_CONCURRENT_METADATA: usize = 40; // TMDB allows 50 req/sec
        const MAX_CONCURRENT_DB_OPS: usize = 20;
        const MAX_CONCURRENT_THUMBNAILS: usize = 10;

        let metadata_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_METADATA));
        let db_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DB_OPS));
        let thumbnail_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_THUMBNAILS));

        // Shared state for concurrent processing
        let processed_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let self_arc = Arc::new(self.clone());
        let scan_id_arc = Arc::new(scan_id.clone());
        let force_rescan = request.force_rescan;
        let extractor_arc = Arc::new(Mutex::new(extractor));

        // Create futures for concurrent processing
        let futures = all_files
            .into_iter()
            .enumerate()
            .map(|(_index, mut media_file)| {
                let scan_id = scan_id_arc.clone();
                let scan_manager = self_arc.clone();
                let metadata_sem = metadata_semaphore.clone();
                let db_sem = db_semaphore.clone();
                let thumb_sem = thumbnail_semaphore.clone();
                let counter = processed_counter.clone();
                let extractor_mutex = extractor_arc.clone();

                async move {
                    // Check if scan was cancelled
                    if scan_manager.is_scan_cancelled(&scan_id).await {
                        return;
                    }

                    // Update progress
                    let current_count =
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    scan_manager
                        .update_progress(&scan_id, |p| {
                            p.current_file = Some(media_file.filename.clone());
                            p.scanned_files = current_count;

                            // Calculate ETA based on current processing rate
                            let elapsed = start_time.elapsed();
                            if elapsed.as_secs() > 0 {
                                let rate = current_count as f64 / elapsed.as_secs_f64();
                                let remaining = total_files - current_count;
                                if rate > 0.0 {
                                    p.estimated_time_remaining =
                                        Some(Duration::from_secs_f64(remaining as f64 / rate));
                                }
                            }
                        })
                        .await;

                    // Check if we should skip this file (for incremental scanning)
                    if !force_rescan {
                        if let Ok(Some(existing)) = scan_manager
                            .db
                            .backend()
                            .get_media_by_path(&media_file.path.to_string_lossy())
                            .await
                        {
                            // Skip if file hasn't been modified
                            if let Ok(metadata) = tokio::fs::metadata(&media_file.path).await {
                                if let Ok(modified) = metadata.modified() {
                                    let modified_time =
                                        chrono::DateTime::<chrono::Utc>::from(modified);
                                    if modified_time <= existing.created_at {
                                        return; // Skip unchanged file
                                    }
                                }
                            }
                        }
                    }

                    // Extract metadata (with mutex to avoid race conditions)
                    {
                        let mut extractor_guard = extractor_mutex.lock().await;
                        if let Some(ref mut metadata_extractor) = *extractor_guard {
                            match metadata_extractor.extract_metadata(&media_file.path) {
                                Ok(metadata) => {
                                    // Check if this is a sample file and skip it
                                    if metadata_extractor.is_sample(&metadata) {
                                        info!("Skipping sample file: {}", media_file.filename);
                                        scan_manager
                                            .update_progress(&scan_id, |p| {
                                                p.skipped_samples += 1;
                                            })
                                            .await;
                                        return; // Skip this file
                                    }
                                    media_file.metadata = Some(metadata);
                                }
                                Err(e) => {
                                    let error_msg = format!(
                                        "Metadata extraction failed for {}: {}",
                                        media_file.filename, e
                                    );
                                    warn!("{}", error_msg);
                                    scan_manager
                                        .update_progress(&scan_id, |p| {
                                            p.errors.push(error_msg);
                                        })
                                        .await;
                                }
                            }
                        }
                    }

                    // Store in database with concurrency control
                    let _db_permit = db_sem.acquire().await.unwrap();
                    match scan_manager
                        .db
                        .backend()
                        .store_media(media_file.clone())
                        .await
                    {
                        Ok(id) => {
                            scan_manager
                                .update_progress(&scan_id, |p| {
                                    p.stored_files += 1;
                                })
                                .await;
                            drop(_db_permit); // Release early

                            // Send media event
                            scan_manager
                                .send_media_event(MediaEvent::MediaAdded {
                                    media: media_file.clone(),
                                })
                                .await;

                            // Fetch external metadata concurrently
                            if media_file
                                .metadata
                                .as_ref()
                                .map(|m| m.external_info.is_none())
                                .unwrap_or(true)
                            {
                                let _metadata_permit = metadata_sem.acquire().await.unwrap();

                                match scan_manager
                                    .metadata_service
                                    .fetch_metadata(&media_file)
                                    .await
                                {
                                    Ok(detailed_info) => {
                                        // Update media with external info
                                        let mut updated_media = media_file.clone();
                                        if let Some(ref mut metadata) = updated_media.metadata {
                                            metadata.external_info =
                                                Some(detailed_info.external_info.clone());
                                        }

                                        // Store updated media
                                        if let Err(e) = scan_manager
                                            .db
                                            .backend()
                                            .store_media(updated_media.clone())
                                            .await
                                        {
                                            warn!(
                                                "Failed to update media with TMDB metadata: {}",
                                                e
                                            );
                                        } else {
                                            scan_manager
                                                .update_progress(&scan_id, |p| {
                                                    p.metadata_fetched += 1;
                                                })
                                                .await;

                                            // Cache poster if available
                                            if let Some(poster_path) =
                                                &detailed_info.external_info.poster_url
                                            {
                                                let media_id = id.split(':').last().unwrap_or(&id);
                                                if let Err(e) = scan_manager
                                                    .metadata_service
                                                    .cache_poster(poster_path, media_id)
                                                    .await
                                                {
                                                    warn!("Failed to cache poster: {}", e);
                                                }
                                            }

                                            // Send media updated event
                                            scan_manager
                                                .send_media_event(MediaEvent::MediaUpdated {
                                                    media: updated_media,
                                                })
                                                .await;
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to fetch TMDB metadata for {}: {}",
                                            media_file.filename, e
                                        );
                                    }
                                }
                            }

                            // Extract thumbnail for TV episodes concurrently
                            if let Some(metadata) = &media_file.metadata {
                                if let Some(parsed) = &metadata.parsed_info {
                                    if parsed.media_type == ferrex_core::MediaType::TvEpisode {
                                        let _thumb_permit = thumb_sem.acquire().await.unwrap();
                                        let media_id = id.split(':').last().unwrap_or(&id);
                                        if let Err(e) = scan_manager
                                            .thumbnail_service
                                            .extract_thumbnail(
                                                media_id,
                                                &media_file.path.to_string_lossy(),
                                            )
                                            .await
                                        {
                                            warn!(
                                                "Failed to extract thumbnail for {}: {}",
                                                media_file.filename, e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to store {}: {}", media_file.filename, e);
                            warn!("{}", error_msg);
                            scan_manager
                                .update_progress(&scan_id, |p| {
                                    p.errors.push(error_msg);
                                })
                                .await;
                        }
                    }
                }
            });

        // Process all files concurrently
        let mut stream = stream::iter(futures).buffer_unordered(MAX_CONCURRENT_DB_OPS);
        while let Some(_) = stream.next().await {
            // Progress is tracked in the futures above
        }

        // Mark scan as completed
        self.update_progress(&scan_id, |p| {
            p.status = ScanStatus::Completed;
            p.completed_at = Some(chrono::Utc::now());
            p.current_file = None;
            p.estimated_time_remaining = None;
        })
        .await;

        // Send scan completed event
        self.send_media_event(MediaEvent::ScanCompleted {
            scan_id: scan_id.clone(),
        })
        .await;

        // Move to history
        if let Some(progress) = self.active_scans.write().await.remove(&scan_id) {
            self.scan_history.write().await.push(progress);
        }

        Ok(())
    }

    /// Clean up all files that no longer exist on disk
    async fn cleanup_all_deleted_files(&self, scan_id: &str) -> Result<usize, anyhow::Error> {
        info!("Checking for deleted files across entire library");

        let all_media = self.db.backend().get_all_media().await?;
        let mut deleted_count = 0;

        for media in all_media {
            if !media.path.exists() {
                info!(
                    "File no longer exists, removing from database: {:?}",
                    media.path
                );
                if let Err(e) = self.db.backend().delete_media(&media.id.to_string()).await {
                    warn!("Failed to delete media {}: {}", media.id, e);
                } else {
                    deleted_count += 1;
                    // Send media deleted event
                    self.send_media_event(MediaEvent::MediaDeleted {
                        id: media.id.to_string(),
                    })
                    .await;
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
