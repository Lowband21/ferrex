use crate::{
    database::traits::{FileWatchEvent, FileWatchEventType},
    LibraryReference, MediaDatabase, Result,
};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};
use uuid::Uuid;
use chrono::Utc;

/// Watches filesystem changes for libraries
pub struct FileWatcher {
    db: Arc<MediaDatabase>,
    watchers: Arc<RwLock<HashMap<Uuid, RecommendedWatcher>>>,
    event_tx: mpsc::UnboundedSender<FileWatchEvent>,
    event_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<FileWatchEvent>>>,
}

impl FileWatcher {
    pub fn new(db: Arc<MediaDatabase>) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            db,
            watchers: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(tokio::sync::Mutex::new(event_rx)),
        }
    }

    /// Start watching a library's paths
    pub async fn watch_library(&self, library: &LibraryReference) -> Result<()> {
        if !library.paths.is_empty() {
            info!("Starting file watcher for library: {}", library.name);

            let event_tx = self.event_tx.clone();
            let library_id = library.id;
            let db = self.db.clone();

            // Create a watcher for this library
            let mut watcher = RecommendedWatcher::new(
                move |res: std::result::Result<Event, notify::Error>| {
                    match res {
                        Ok(event) => {
                            if let Some(watch_event) = Self::convert_notify_event(event, library_id) {
                                debug!("File watch event: {:?}", watch_event);
                                
                                // Send to channel for processing
                                if let Err(e) = event_tx.send(watch_event.clone()) {
                                    error!("Failed to send file watch event: {}", e);
                                } else {
                                    // Also persist to database
                                    let db_clone = db.clone();
                                    tokio::spawn(async move {
                                        if let Err(e) = db_clone.backend().create_file_watch_event(&watch_event).await {
                                            error!("Failed to persist file watch event: {}", e);
                                        }
                                    });
                                }
                            }
                        }
                        Err(e) => error!("Watch error: {:?}", e),
                    }
                },
                Config::default(),
            ).map_err(|e| crate::MediaError::Internal(format!("Failed to create watcher: {}", e)))?;

            // Watch all paths in the library
            for path in &library.paths {
                match watcher.watch(path, RecursiveMode::Recursive) {
                    Ok(_) => info!("Watching path: {}", path.display()),
                    Err(e) => {
                        error!("Failed to watch path {}: {}", path.display(), e);
                        return Err(crate::MediaError::Internal(format!(
                            "Failed to watch path: {}",
                            e
                        )));
                    }
                }
            }

            // Store the watcher
            self.watchers.write().await.insert(library.id, watcher);
        }

        Ok(())
    }

    /// Stop watching a library
    pub async fn unwatch_library(&self, library_id: Uuid) -> Result<()> {
        if self.watchers.write().await.remove(&library_id).is_some() {
            info!("Stopped watching library: {}", library_id);
        }
        Ok(())
    }

    /// Process pending file watch events
    pub async fn process_events(&self, batch_size: usize) -> Result<Vec<FileWatchEvent>> {
        let mut events = Vec::new();
        let mut rx = self.event_rx.lock().await;

        // Collect up to batch_size events
        for _ in 0..batch_size {
            match rx.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(crate::MediaError::Internal(
                        "File watcher channel disconnected".to_string(),
                    ))
                }
            }
        }

        Ok(events)
    }

    /// Get unprocessed events from database
    pub async fn get_unprocessed_events(
        &self,
        library_id: Uuid,
        limit: i32,
    ) -> Result<Vec<FileWatchEvent>> {
        self.db
            .backend()
            .get_unprocessed_events(library_id, limit)
            .await
    }

    /// Mark an event as processed
    pub async fn mark_event_processed(&self, event_id: Uuid) -> Result<()> {
        self.db.backend().mark_event_processed(event_id).await
    }

    /// Convert notify event to our FileWatchEvent
    fn convert_notify_event(event: Event, library_id: Uuid) -> Option<FileWatchEvent> {
        // Get the first path (most events only have one)
        let path = event.paths.first()?.clone();
        let path_str = path.to_string_lossy().to_string();

        // Determine event type
        let event_type = match event.kind {
            EventKind::Create(_) => FileWatchEventType::Created,
            EventKind::Modify(_) => FileWatchEventType::Modified,
            EventKind::Remove(_) => FileWatchEventType::Deleted,
            EventKind::Any => return None, // Skip generic events
            EventKind::Access(_) => return None, // Skip access events
            EventKind::Other => return None, // Skip other events
        };

        // Check if it's a video file
        if !Self::is_video_file(&path) && event_type != FileWatchEventType::Deleted {
            return None;
        }

        // Get file size if file exists
        let file_size = if event_type != FileWatchEventType::Deleted {
            std::fs::metadata(&path).ok().map(|m| m.len() as i64)
        } else {
            None
        };

        Some(FileWatchEvent {
            id: Uuid::new_v4(),
            library_id,
            event_type,
            file_path: path_str,
            old_path: None, // Moves are complex in notify, would need to track separately
            file_size,
            detected_at: Utc::now(),
            processed: false,
            processed_at: None,
            processing_attempts: 0,
            last_error: None,
        })
    }

    /// Check if a file is a video file
    fn is_video_file(path: &Path) -> bool {
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

    /// Cleanup old processed events
    pub async fn cleanup_old_events(&self, days_to_keep: i32) -> Result<u32> {
        self.db.backend().cleanup_old_events(days_to_keep).await
    }
}