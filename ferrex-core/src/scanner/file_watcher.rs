use crate::{
    LibraryID, LibraryReference, MediaDatabase, Result,
    database::traits::{FileWatchEvent, FileWatchEventType},
};
use chrono::Utc;
use notify::{Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, NoCache, new_debouncer,
};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Watches filesystem changes for libraries
pub struct FileWatcher {
    db: Arc<MediaDatabase>,
    watchers: Arc<RwLock<HashMap<LibraryID, LibraryWatcher>>>,
    event_tx: mpsc::UnboundedSender<FileWatchEvent>,
    event_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<FileWatchEvent>>>,
}

impl fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let watcher_count = self.watchers.try_read().map(|map| map.len()).unwrap_or(0);
        let receiver_locked = self.event_rx.try_lock().is_err();

        f.debug_struct("FileWatcher")
            .field("db", &self.db)
            .field("watcher_count", &watcher_count)
            .field("receiver_locked", &receiver_locked)
            .finish()
    }
}

/// Internal watcher variants per library
enum LibraryWatcher {
    Debounced(Debouncer<RecommendedWatcher, NoCache>),
    Poll(PollWatcher),
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

            // Determine if any path is on a network filesystem
            let use_poll = library.paths.iter().any(|p| is_network_filesystem(p));

            if use_poll {
                warn!(
                    "Using polling watcher for library {} due to network filesystem",
                    library.name
                );

                let mut watcher = PollWatcher::new(
                    {
                        let event_tx = event_tx.clone();
                        let db = db.clone();
                        move |res: std::result::Result<Event, notify::Error>| match res {
                            Ok(event) => {
                                if let Some(watch_event) =
                                    Self::convert_notify_event(event, library_id)
                                {
                                    debug!("[poll] File watch event: {:?}", watch_event);

                                    if let Err(e) = event_tx.send(watch_event.clone()) {
                                        error!("Failed to send file watch event: {}", e);
                                    } else {
                                        let db_clone = db.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = db_clone
                                                .backend()
                                                .create_file_watch_event(&watch_event)
                                                .await
                                            {
                                                error!("Failed to persist file watch event: {}", e);
                                            }
                                        });
                                    }
                                }
                            }
                            Err(e) => error!("Poll watch error: {:?}", e),
                        }
                    },
                    Config::default().with_poll_interval(Duration::from_secs(600)),
                )
                .map_err(|e| {
                    crate::MediaError::Internal(format!("Failed to create poll watcher: {}", e))
                })?;

                for path in &library.paths {
                    match watcher.watch(path, RecursiveMode::Recursive) {
                        Ok(_) => info!("Polling path: {}", path.display()),
                        Err(e) => {
                            error!("Failed to watch path {}: {}", path.display(), e);
                            return Err(crate::MediaError::Internal(format!(
                                "Failed to watch path: {}",
                                e
                            )));
                        }
                    }
                }

                self.watchers
                    .write()
                    .await
                    .insert(library.id, LibraryWatcher::Poll(watcher));
            } else {
                // Debounced watcher for local filesystems (inotify/FSEvents/etc.)
                let event_tx_cb = event_tx.clone();
                let db_cb = db.clone();
                let mut debouncer = new_debouncer(
                    Duration::from_millis(200), // debounce window: 200ms
                    None,
                    move |result: DebounceEventResult| {
                        match result {
                            Ok(events) => {
                                for de in events {
                                    // Try to map debounced event to our model via the underlying notify::Event
                                    if let Some(watch_event) =
                                        Self::convert_debounced_event(&de, library_id)
                                    {
                                        debug!("[debounced] File watch event: {:?}", watch_event);

                                        if let Err(e) = event_tx_cb.send(watch_event.clone()) {
                                            error!("Failed to send file watch event: {}", e);
                                        } else {
                                            let db_clone = db_cb.clone();
                                            tokio::spawn(async move {
                                                if let Err(e) = db_clone
                                                    .backend()
                                                    .create_file_watch_event(&watch_event)
                                                    .await
                                                {
                                                    error!(
                                                        "Failed to persist file watch event: {}",
                                                        e
                                                    );
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            Err(errors) => {
                                for e in errors {
                                    error!("Debouncer error: {}", e);
                                }
                            }
                        }
                    },
                )
                .map_err(|e| {
                    crate::MediaError::Internal(format!("Failed to create debouncer: {}", e))
                })?;

                for path in &library.paths {
                    match debouncer.watch(path, RecursiveMode::Recursive) {
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

                self.watchers
                    .write()
                    .await
                    .insert(library.id, LibraryWatcher::Debounced(debouncer));
            }
        }

        Ok(())
    }

    /// Stop watching a library
    pub async fn unwatch_library(&self, library_id: LibraryID) -> Result<()> {
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
                    ));
                }
            }
        }

        Ok(events)
    }

    /// Get unprocessed events from database
    pub async fn get_unprocessed_events(
        &self,
        library_id: LibraryID,
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
    fn convert_notify_event(event: Event, library_id: LibraryID) -> Option<FileWatchEvent> {
        // Get the primary path
        let path = event.paths.first()?.clone();

        // Early path filtering
        if should_ignore_path(&path) {
            return None;
        }

        // Determine event type and handle renames if two paths are present
        let mut old_path: Option<String> = None;
        let mut event_type = match event.kind {
            EventKind::Create(_) => FileWatchEventType::Created,
            EventKind::Modify(_) => FileWatchEventType::Modified,
            EventKind::Remove(_) => FileWatchEventType::Deleted,
            EventKind::Any => return None,       // Skip generic events
            EventKind::Access(_) => return None, // Skip access events
            EventKind::Other => return None,     // Skip other events
        };

        if event.paths.len() == 2 {
            // Likely a rename/move
            event_type = FileWatchEventType::Moved;
            old_path = event.paths.first().map(|p| p.to_string_lossy().to_string());
        }

        // Video file filtering (allow deletions regardless)
        if event_type != FileWatchEventType::Deleted && !Self::is_video_file(&path) {
            return None;
        }

        let path_str = path.to_string_lossy().to_string();

        // Get file size if file exists for create/modify/move
        let file_size = match event_type {
            FileWatchEventType::Deleted => None,
            _ => fs::metadata(&path).ok().map(|m| m.len() as i64),
        };

        Some(FileWatchEvent {
            id: Uuid::new_v4(),
            library_id,
            event_type,
            file_path: path_str,
            old_path,
            file_size,
            detected_at: Utc::now(),
            processed: false,
            processed_at: None,
            processing_attempts: 0,
            last_error: None,
        })
    }

    /// Convert a debounced event (from notify-debouncer-full) to our FileWatchEvent
    fn convert_debounced_event(
        event: &DebouncedEvent,
        library_id: LibraryID,
    ) -> Option<FileWatchEvent> {
        // Prefer to use the underlying notify::Event when available
        #[allow(deprecated)]
        let notify_event = &event.event;
        Self::convert_notify_event(notify_event.clone(), library_id)
    }

    /// Check if a file is a video file
    fn is_video_file(path: &Path) -> bool {
        let video_extensions = [
            "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg",
        ];

        if let Some(extension) = path.extension()
            && let Some(ext_str) = extension.to_str()
        {
            return video_extensions.contains(&ext_str.to_lowercase().as_str());
        }
        false
    }

    /// Fast path filtering to exclude common noise
    fn is_ignored_component(component: &str) -> bool {
        matches!(
            component,
            "target" | "node_modules" | ".git" | ".hg" | ".svn" | ".DS_Store"
        )
    }

    fn should_ignore_extension(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("tmp" | "swp" | "bak" | "part" | "crdownload")
        )
    }

    fn path_contains_ignored_dir(path: &Path) -> bool {
        path.components().any(|c| match c {
            std::path::Component::Normal(os) => {
                if let Some(s) = os.to_str() {
                    Self::is_ignored_component(s)
                } else {
                    false
                }
            }
            _ => false,
        })
    }

    /// Cleanup old processed events
    pub async fn cleanup_old_events(&self, days_to_keep: i32) -> Result<u32> {
        self.db.backend().cleanup_old_events(days_to_keep).await
    }
}

/// Determine if a path resides on a network filesystem (Linux)
fn is_network_filesystem(path: &Path) -> bool {
    // Attempt to canonicalize to match mountpoints
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    let file = match fs::File::open("/proc/mounts") {
        Ok(f) => f,
        Err(_) => return false,
    };
    let reader = BufReader::new(file);

    let mut best_match: Option<(PathBuf, String)> = None; // (mountpoint, fstype)
    for line in reader.lines().flatten() {
        // /proc/mounts format: src mountpoint fstype options 0 0
        let mut parts = line.split_whitespace();
        let _src = parts.next();
        let mountpoint = parts.next();
        let fstype = parts.next();
        if let (Some(mnt), Some(fs_type)) = (mountpoint, fstype) {
            let mnt_path = PathBuf::from(mnt);
            if canonical.starts_with(&mnt_path) {
                let take = match &best_match {
                    None => true,
                    Some((best, _)) => mnt_path.as_os_str().len() > best.as_os_str().len(),
                };
                if take {
                    best_match = Some((mnt_path, fs_type.to_string()));
                }
            }
        }
    }

    if let Some((_mnt, fstype)) = best_match {
        // Common network filesystems
        let net_fs = [
            "nfs",
            "nfs4",
            "cifs",
            "smbfs",
            "smb3",
            "smbfs",
            "afs",
            "sshfs",
            "fuse.sshfs",
        ];
        return net_fs.iter().any(|t| &fstype == t);
    }
    false
}

fn should_ignore_path(path: &Path) -> bool {
    if FileWatcher::path_contains_ignored_dir(path) || FileWatcher::should_ignore_extension(path) {
        return true;
    }
    false
}
