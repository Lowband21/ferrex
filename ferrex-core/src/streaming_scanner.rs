use crate::{MediaError, Result, Library, MediaScanner, MediaFile, MediaDatabase, MetadataExtractor};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;
use walkdir::WalkDir;
use futures::stream::Stream;

/// Progress events emitted during streaming scan
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanProgress {
    /// Scan has started
    ScanStarted {
        scan_id: Uuid,
        library_id: Uuid,
        library_name: String,
        paths: Vec<String>,
    },
    /// A new folder has been discovered
    FolderDiscovered {
        path: String,
        estimated_files: usize,
    },
    /// A file has been scanned and added to database
    FileScanned {
        file_id: Uuid,
        filename: String,
        path: String,
        current: usize,
        total_estimate: usize,
    },
    /// Metadata has been extracted for a file
    MetadataExtracted {
        file_id: Uuid,
        has_metadata: bool,
    },
    /// External metadata has been fetched
    ExternalMetadataFetched {
        file_id: Uuid,
        source: String,
        success: bool,
    },
    /// Thumbnail has been generated
    ThumbnailGenerated {
        file_id: Uuid,
        thumbnail_path: String,
    },
    /// A batch of files has been processed
    BatchCompleted {
        processed: usize,
        remaining: usize,
        batch_duration_ms: u64,
    },
    /// Scan has completed
    ScanCompleted {
        scan_id: Uuid,
        total_files: usize,
        new_files: usize,
        updated_files: usize,
        errors: usize,
        duration_secs: u64,
    },
    /// An error occurred
    Error {
        path: Option<String>,
        error: String,
        recoverable: bool,
    },
}

/// Configuration for the streaming scanner
#[derive(Debug, Clone)]
pub struct StreamingScannerConfig {
    /// Number of concurrent folder scanning workers
    pub folder_workers: usize,
    /// Number of concurrent file processing workers
    pub file_workers: usize,
    /// Maximum files to process in a single batch
    pub batch_size: usize,
    /// Progress channel buffer size
    pub progress_buffer: usize,
    /// Whether to extract metadata during scan
    pub extract_metadata: bool,
    /// Whether to fetch external metadata during scan
    pub fetch_external_metadata: bool,
    /// Whether to generate thumbnails during scan
    pub generate_thumbnails: bool,
}

impl Default for StreamingScannerConfig {
    fn default() -> Self {
        Self {
            folder_workers: 4,
            file_workers: 8,
            batch_size: 100,
            progress_buffer: 1000,
            extract_metadata: true,
            fetch_external_metadata: true,
            generate_thumbnails: true,
        }
    }
}

/// Streaming scanner that processes files incrementally
pub struct StreamingScanner {
    config: StreamingScannerConfig,
    base_scanner: MediaScanner,
    db: Arc<MediaDatabase>,
    metadata_extractor: Option<Arc<tokio::sync::Mutex<MetadataExtractor>>>,
}

/// Handle to a running scan
pub struct ScanHandle {
    pub scan_id: Uuid,
    pub progress_rx: mpsc::Receiver<ScanProgress>,
    pub cancel_tx: mpsc::Sender<()>,
    join_handle: JoinHandle<Result<ScanSummary>>,
}

/// Summary of a completed scan
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanSummary {
    pub scan_id: Uuid,
    pub total_files: usize,
    pub new_files: usize,
    pub updated_files: usize,
    pub errors: Vec<String>,
    pub duration: Duration,
}

/// Internal message for coordinating scan workers
#[derive(Debug)]
enum WorkerMessage {
    FolderBatch(Vec<PathBuf>),
    FileBatch(Vec<PathBuf>),
}

impl StreamingScanner {
    /// Create a new streaming scanner with default configuration
    pub fn new(db: Arc<MediaDatabase>) -> Self {
        Self {
            config: StreamingScannerConfig::default(),
            base_scanner: MediaScanner::new(),
            db,
            metadata_extractor: None,
        }
    }

    /// Create a new streaming scanner with custom configuration
    pub fn with_config(config: StreamingScannerConfig, db: Arc<MediaDatabase>) -> Self {
        let metadata_extractor = if config.extract_metadata {
            Some(Arc::new(tokio::sync::Mutex::new(MetadataExtractor::new())))
        } else {
            None
        };
        
        Self {
            config,
            base_scanner: MediaScanner::new(),
            db,
            metadata_extractor,
        }
    }

    /// Start a streaming scan of a library
    pub fn scan_library(
        self: Arc<Self>,
        library: Arc<Library>,
        _force: bool,
    ) -> ScanHandle {
        let scan_id = Uuid::new_v4();
        let (progress_tx, progress_rx) = mpsc::channel(self.config.progress_buffer);
        let (cancel_tx, mut cancel_rx) = mpsc::channel(1);
        
        let scanner = self; // Move self into the task
        let config = scanner.config.clone();
        let base_scanner = scanner.base_scanner.clone()
            .with_library(library.id, library.library_type.clone());

        let join_handle = tokio::spawn(async move {
            let start_time = Instant::now();
            
            // Send scan started event
            let _ = progress_tx.send(ScanProgress::ScanStarted {
                scan_id,
                library_id: library.id,
                library_name: library.name.clone(),
                paths: library.paths.iter().map(|p| p.display().to_string()).collect(),
            }).await;

            // Create work channels
            let (folder_tx, folder_rx) = mpsc::channel::<WorkerMessage>(100);
            let (file_tx, file_rx) = mpsc::channel::<WorkerMessage>(1000);

            // Statistics tracking
            let stats = Arc::new(tokio::sync::Mutex::new(ScanStatistics::default()));

            // Start folder discovery task
            let library_paths = library.paths.clone();
            let folder_tx_clone = folder_tx.clone();
            let progress_tx_clone = progress_tx.clone();
            let stats_clone = stats.clone();
            
            let discovery_task = tokio::spawn(async move {
                Self::discover_folders(
                    library_paths,
                    folder_tx_clone,
                    progress_tx_clone,
                    config.batch_size,
                    stats_clone,
                ).await
            });

            // Start folder scanning workers
            let folder_rx = Arc::new(tokio::sync::Mutex::new(folder_rx));
            let mut folder_workers = Vec::new();
            for worker_id in 0..config.folder_workers {
                let folder_rx_clone = folder_rx.clone();
                let file_tx_clone = file_tx.clone();
                let progress_tx_clone = progress_tx.clone();
                let base_scanner_clone = base_scanner.clone();
                let stats_clone = stats.clone();
                
                let worker = tokio::spawn(async move {
                    Self::folder_scan_worker(
                        worker_id,
                        folder_rx_clone,
                        file_tx_clone,
                        progress_tx_clone,
                        base_scanner_clone,
                        stats_clone,
                    ).await
                });
                
                folder_workers.push(worker);
            }

            // Start file processing workers
            let file_rx = Arc::new(tokio::sync::Mutex::new(file_rx));
            let mut file_workers = Vec::new();
            for worker_id in 0..config.file_workers {
                let file_rx_clone = file_rx.clone();
                let progress_tx_clone = progress_tx.clone();
                let config_clone = config.clone();
                let stats_clone = stats.clone();
                let db_clone = scanner.db.clone();
                let metadata_extractor_clone = scanner.metadata_extractor.clone();
                let library_clone = library.clone();
                
                let worker = tokio::spawn(async move {
                    Self::file_process_worker(
                        worker_id,
                        file_rx_clone,
                        progress_tx_clone,
                        config_clone,
                        stats_clone,
                        db_clone,
                        metadata_extractor_clone,
                        library_clone,
                    ).await
                });
                
                file_workers.push(worker);
            }

            // Monitor for cancellation
            tokio::select! {
                _ = cancel_rx.recv() => {
                    info!("Scan {} cancelled", scan_id);
                    // TODO: Implement graceful shutdown
                }
                _ = discovery_task => {
                    debug!("Folder discovery completed");
                    drop(folder_tx); // Signal folder workers to complete
                }
            }

            // Wait for all workers to complete
            for worker in folder_workers {
                let _ = worker.await;
            }
            drop(file_tx); // Signal file workers to complete
            
            for worker in file_workers {
                let _ = worker.await;
            }

            // Calculate final statistics
            let final_stats = stats.lock().await;
            let duration = start_time.elapsed();
            
            // Send completion event
            let _ = progress_tx.send(ScanProgress::ScanCompleted {
                scan_id,
                total_files: final_stats.total_files,
                new_files: final_stats.new_files,
                updated_files: final_stats.updated_files,
                errors: final_stats.errors,
                duration_secs: duration.as_secs(),
            }).await;

            Ok(ScanSummary {
                scan_id,
                total_files: final_stats.total_files,
                new_files: final_stats.new_files,
                updated_files: final_stats.updated_files,
                errors: final_stats.error_messages.clone(),
                duration,
            })
        });

        ScanHandle {
            scan_id,
            progress_rx,
            cancel_tx,
            join_handle,
        }
    }

    /// Discover folders and send them to workers in batches
    async fn discover_folders(
        paths: Vec<PathBuf>,
        folder_tx: mpsc::Sender<WorkerMessage>,
        progress_tx: mpsc::Sender<ScanProgress>,
        batch_size: usize,
        _stats: Arc<tokio::sync::Mutex<ScanStatistics>>,
    ) -> Result<()> {
        let mut folder_batch = Vec::with_capacity(batch_size);
        
        for root_path in paths {
            if !root_path.exists() {
                let _ = progress_tx.send(ScanProgress::Error {
                    path: Some(root_path.display().to_string()),
                    error: "Path does not exist".to_string(),
                    recoverable: true,
                }).await;
                continue;
            }

            let walker = WalkDir::new(&root_path)
                .follow_links(false)
                .max_depth(10);

            for entry in walker {
                match entry {
                    Ok(entry) if entry.file_type().is_dir() => {
                        let path = entry.path().to_path_buf();
                        
                        // Estimate files in directory
                        let file_count = Self::estimate_files_in_dir(&path);
                        if file_count > 0 {
                            let _ = progress_tx.send(ScanProgress::FolderDiscovered {
                                path: path.display().to_string(),
                                estimated_files: file_count,
                            }).await;

                            folder_batch.push(path);
                            
                            // Send batch when full
                            if folder_batch.len() >= batch_size {
                                let batch = std::mem::replace(&mut folder_batch, Vec::with_capacity(batch_size));
                                if folder_tx.send(WorkerMessage::FolderBatch(batch)).await.is_err() {
                                    return Err(MediaError::Cancelled("Scan cancelled".to_string()));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = progress_tx.send(ScanProgress::Error {
                            path: None,
                            error: format!("Directory walk error: {}", e),
                            recoverable: true,
                        }).await;
                    }
                    _ => {} // Skip non-directories
                }
            }
        }

        // Send remaining folders
        if !folder_batch.is_empty() {
            let _ = folder_tx.send(WorkerMessage::FolderBatch(folder_batch)).await;
        }

        Ok(())
    }

    /// Estimate number of files in a directory (non-recursive)
    fn estimate_files_in_dir(path: &PathBuf) -> usize {
        std::fs::read_dir(path)
            .map(|entries| entries.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    }

    /// Worker that scans folders for media files
    async fn folder_scan_worker(
        worker_id: usize,
        folder_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<WorkerMessage>>>,
        file_tx: mpsc::Sender<WorkerMessage>,
        progress_tx: mpsc::Sender<ScanProgress>,
        scanner: MediaScanner,
        _stats: Arc<tokio::sync::Mutex<ScanStatistics>>,
    ) {
        debug!("Folder scan worker {} started", worker_id);
        
        loop {
            let message = {
                let mut rx = folder_rx.lock().await;
                rx.recv().await
            };

            match message {
                Some(WorkerMessage::FolderBatch(folders)) => {
                    let mut file_batch = Vec::new();
                    
                    for folder in folders {
                        match std::fs::read_dir(&folder) {
                            Ok(entries) => {
                                for entry in entries.filter_map(|e| e.ok()) {
                                    let path = entry.path();
                                    if path.is_file() && scanner.is_video_file(&path) {
                                        file_batch.push(path);
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = progress_tx.send(ScanProgress::Error {
                                    path: Some(folder.display().to_string()),
                                    error: format!("Failed to read directory: {}", e),
                                    recoverable: true,
                                }).await;
                            }
                        }
                    }

                    if !file_batch.is_empty() {
                        if file_tx.send(WorkerMessage::FileBatch(file_batch)).await.is_err() {
                            break;
                        }
                    }
                }
                None => break,
                _ => {}
            }
        }
        
        debug!("Folder scan worker {} completed", worker_id);
    }

    /// Worker that processes individual media files
    async fn file_process_worker(
        worker_id: usize,
        file_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<WorkerMessage>>>,
        progress_tx: mpsc::Sender<ScanProgress>,
        config: StreamingScannerConfig,
        stats: Arc<tokio::sync::Mutex<ScanStatistics>>,
        db: Arc<MediaDatabase>,
        metadata_extractor: Option<Arc<tokio::sync::Mutex<MetadataExtractor>>>,
        library: Arc<Library>,
    ) {
        debug!("File process worker {} started", worker_id);
        
        loop {
            let message = {
                let mut rx = file_rx.lock().await;
                rx.recv().await
            };

            match message {
                Some(WorkerMessage::FileBatch(files)) => {
                    let batch_start = Instant::now();
                    let batch_size = files.len();
                    
                    for file_path in files {
                        // Process file through full pipeline
                        match Self::process_single_file(
                            &file_path,
                            &library,
                            &db,
                            &metadata_extractor,
                            &config,
                            &progress_tx,
                            &stats,
                        ).await {
                            Ok(file_id) => {
                                debug!("Successfully processed file: {} ({})", file_path.display(), file_id);
                            }
                            Err(e) => {
                                warn!("Failed to process file {}: {}", file_path.display(), e);
                                let _ = progress_tx.send(ScanProgress::Error {
                                    path: Some(file_path.display().to_string()),
                                    error: e.to_string(),
                                    recoverable: true,
                                }).await;
                                
                                let mut stats_guard = stats.lock().await;
                                stats_guard.errors += 1;
                                stats_guard.error_messages.push(format!("{}: {}", file_path.display(), e));
                            }
                        }
                    }

                    let _ = progress_tx.send(ScanProgress::BatchCompleted {
                        processed: batch_size,
                        remaining: 0, // TODO: Calculate actual remaining
                        batch_duration_ms: batch_start.elapsed().as_millis() as u64,
                    }).await;
                }
                None => break,
                _ => {}
            }
        }
        
        debug!("File process worker {} completed", worker_id);
    }

    /// Process a single file through the complete pipeline
    async fn process_single_file(
        file_path: &PathBuf,
        library: &Library,
        db: &Arc<MediaDatabase>,
        metadata_extractor: &Option<Arc<tokio::sync::Mutex<MetadataExtractor>>>,
        config: &StreamingScannerConfig,
        progress_tx: &mpsc::Sender<ScanProgress>,
        stats: &Arc<tokio::sync::Mutex<ScanStatistics>>,
    ) -> Result<Uuid> {
        // Step 1: Create MediaFile
        let mut media_file = MediaFile::new_with_library(file_path.clone(), library.id)?;
        
        // Update stats
        {
            let mut stats_guard = stats.lock().await;
            stats_guard.total_files += 1;
        }
        
        // Step 2: Extract metadata if enabled
        if config.extract_metadata {
            if let Some(extractor) = metadata_extractor {
                let mut extractor_guard = extractor.lock().await;
                // Set library type context for better parsing
                extractor_guard.set_library_type(Some(library.library_type.clone()));
                
                match extractor_guard.extract_metadata(&media_file.path) {
                    Ok(metadata) => {
                        // Check if this is a sample file
                        if extractor_guard.is_sample(&metadata) {
                            info!("Skipping sample file: {}", media_file.filename);
                            let mut stats_guard = stats.lock().await;
                            stats_guard.total_files -= 1; // Don't count samples
                            return Err(MediaError::InvalidMedia("Sample file".to_string()));
                        }
                        media_file.metadata = Some(metadata);
                        
                        // Send metadata extracted event
                        let _ = progress_tx.send(ScanProgress::MetadataExtracted {
                            file_id: media_file.id,
                            has_metadata: true,
                        }).await;
                    }
                    Err(e) => {
                        warn!("Metadata extraction failed for {}: {}", media_file.filename, e);
                        // Continue without metadata
                    }
                }
            }
        }
        
        // Step 3: Store in database
        let _stored_id = db.backend().store_media(media_file.clone()).await?;
        
        // Update stats and send progress
        {
            let mut stats_guard = stats.lock().await;
            stats_guard.new_files += 1;
            
            let _ = progress_tx.send(ScanProgress::FileScanned {
                file_id: media_file.id,
                filename: media_file.filename.clone(),
                path: file_path.display().to_string(),
                current: stats_guard.new_files,
                total_estimate: stats_guard.total_files,
            }).await;
        }
        
        // Step 4: External metadata fetching would happen here
        // (Typically done in a separate pass to respect API rate limits)
        if config.fetch_external_metadata {
            let _ = progress_tx.send(ScanProgress::ExternalMetadataFetched {
                file_id: media_file.id,
                source: "deferred".to_string(),
                success: false, // Deferred for later
            }).await;
        }
        
        // Step 5: Thumbnail generation would happen here
        // (Typically done for TV episodes in a separate pass)
        if config.generate_thumbnails {
            if let Some(metadata) = &media_file.metadata {
                if let Some(parsed) = &metadata.parsed_info {
                    if parsed.media_type == crate::MediaType::TvEpisode {
                        // Thumbnail generation would be queued here
                        let _ = progress_tx.send(ScanProgress::ThumbnailGenerated {
                            file_id: media_file.id,
                            thumbnail_path: "pending".to_string(),
                        }).await;
                    }
                }
            }
        }
        
        Ok(media_file.id)
    }
}

/// Internal statistics tracking
#[derive(Debug, Default)]
struct ScanStatistics {
    total_files: usize,
    new_files: usize,
    updated_files: usize,
    errors: usize,
    error_messages: Vec<String>,
}

impl ScanHandle {
    /// Get the scan ID
    pub fn scan_id(&self) -> Uuid {
        self.scan_id
    }

    /// Wait for the scan to complete and return the summary
    pub async fn wait(self) -> Result<ScanSummary> {
        self.join_handle.await
            .map_err(|e| MediaError::Internal(format!("Scan task failed: {}", e)))?
    }

    /// Cancel the scan
    pub async fn cancel(self) -> Result<()> {
        self.cancel_tx.send(()).await
            .map_err(|_| MediaError::Internal("Failed to send cancel signal".to_string()))?;
        Ok(())
    }

    /// Convert progress receiver into a Stream
    pub fn progress_stream(self) -> impl Stream<Item = ScanProgress> {
        tokio_stream::wrappers::ReceiverStream::new(self.progress_rx)
    }
}