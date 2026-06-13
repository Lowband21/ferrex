//! Filesystem watch provider for library actors.
//!
//! A thin wrapper around `notify` that debounces raw filesystem notifications
//! into batches and forwards them as `LibraryActorCommand::FsEvents` messages
//! through the scan orchestrator mailbox. Overflow conditions are surfaced
//! explicitly so the library actor can fall back to breadth-first rescans of the
//! affected subtree.

use crate::{
    database::traits::{FileWatchEvent, FileWatchEventType},
    domain::scan::orchestration::{
        FileSystemEvent, FileSystemEventKind, LibraryActorCommand,
        LibraryCommandExecutor, LibraryRootsId,
        config::{WatchConfig, WatchStrategy},
        scan_cursor::normalize_path,
    },
    error::{MediaError, Result},
    types::ids::LibraryId,
};

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use notify::event::{EventKind, ModifyKind, RemoveKind, RenameMode};
use notify::{
    Config as NotifyConfig, Event, PollWatcher, RecommendedWatcher,
    RecursiveMode, Watcher,
};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, mpsc};
use tokio::task::{JoinHandle, spawn_blocking};
use tokio::time::{Duration, timeout};
use tracing::{error, info, trace, warn};
use uuid::Uuid;

pub mod event_bus;
pub mod watcher;

pub use event_bus::{FileChangeEventBus, PostgresFileChangeEventBus};

// `watcher` currently holds the markdown design for the upcoming reimplementation that will unify
// realtime watchers, polling fallback, and the Postgres event bus. Once the design solidifies we
// will fold that module into this file and retire the legacy service below.

/// Version field stamped on emitted `FileSystemEvent`s.
pub const EVENT_VERSION: u16 = 1;

const FS_WATCH_CONSUMER_GROUP: &str = "fs-watch-service";

/// Configuration knobs for watch processing.
#[derive(Clone, Debug)]
pub struct FsWatchConfig {
    /// Debounce window for coalescing rapid event bursts per library root.
    pub debounce_window: Duration,
    /// Maximum number of filesystem events bundled into a single flush.
    pub max_batch_events: usize,
    /// Native/poll/auto backend strategy for filesystem watches.
    pub strategy: WatchStrategy,
    /// Polling cadence for backends that cannot deliver native filesystem events.
    pub poll_interval: Duration,
    /// Maximum backoff for poll-oriented recovery loops.
    pub poll_backoff_max: Duration,
}

impl Default for FsWatchConfig {
    fn default() -> Self {
        Self {
            debounce_window: Duration::from_millis(250),
            max_batch_events: 1024,
            strategy: WatchStrategy::Auto,
            poll_interval: Duration::from_secs(30),
            poll_backoff_max: Duration::from_secs(5 * 60),
        }
    }
}

impl From<WatchConfig> for FsWatchConfig {
    fn from(cfg: WatchConfig) -> Self {
        Self {
            debounce_window: Duration::from_millis(
                cfg.debounce_window_ms.max(1),
            ),
            max_batch_events: cfg.max_batch_events.max(1),
            strategy: cfg.strategy,
            poll_interval: Duration::from_millis(cfg.poll_interval_ms.max(1)),
            poll_backoff_max: Duration::from_millis(
                cfg.poll_backoff_max_ms.max(1),
            ),
        }
    }
}

/// Observer hook for surfacing watcher errors.
pub trait FsWatchObserver: Send + Sync {
    fn on_error(&self, library_id: LibraryId, error: &str);
}

/// No-op observer used when metrics instrumentation is not wired up.
pub struct NoopFsWatchObserver;

impl FsWatchObserver for NoopFsWatchObserver {
    fn on_error(&self, _library_id: LibraryId, _error: &str) {}
}

impl fmt::Debug for NoopFsWatchObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NoopFsWatchObserver")
    }
}

/// Dispatches debounced filesystem notifications through the scan orchestrator.
pub struct FsWatchService<O: FsWatchObserver = NoopFsWatchObserver> {
    config: FsWatchConfig,
    observer: Arc<O>,
    command_executor: Arc<dyn LibraryCommandExecutor>,
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    consumer_group: String,
    libraries: Arc<RwLock<HashMap<LibraryId, LibraryWatch>>>,
}

impl<O: FsWatchObserver + 'static> fmt::Debug for FsWatchService<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("FsWatchService");
        debug
            .field("config", &self.config)
            .field("observer_type", &std::any::type_name::<O>())
            .field("command_executor", &"LibraryCommandExecutor")
            .field("durable_event_bus", &self.event_bus.is_some())
            .field("consumer_group", &self.consumer_group);

        match self.libraries.try_read() {
            Ok(guard) => {
                let library_count = guard.len();
                let active_watchers = guard
                    .values()
                    .filter(|entry| entry.watchers.is_some())
                    .count();
                debug
                    .field("library_count", &library_count)
                    .field("active_watchers", &active_watchers);
            }
            Err(_) => {
                debug.field("libraries", &"<locked>");
            }
        }

        debug.finish()
    }
}

impl<O: FsWatchObserver + 'static> FsWatchService<O> {
    pub fn new(
        config: FsWatchConfig,
        observer: Arc<O>,
        command_executor: Arc<dyn LibraryCommandExecutor>,
    ) -> Self {
        Self {
            config,
            observer,
            command_executor,
            event_bus: None,
            consumer_group: FS_WATCH_CONSUMER_GROUP.to_owned(),
            libraries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_event_bus(
        config: FsWatchConfig,
        observer: Arc<O>,
        command_executor: Arc<dyn LibraryCommandExecutor>,
        event_bus: Arc<dyn FileChangeEventBus>,
    ) -> Self {
        Self::with_event_bus_and_group(
            config,
            observer,
            command_executor,
            event_bus,
            FS_WATCH_CONSUMER_GROUP,
        )
    }

    pub fn with_event_bus_and_group(
        config: FsWatchConfig,
        observer: Arc<O>,
        command_executor: Arc<dyn LibraryCommandExecutor>,
        event_bus: Arc<dyn FileChangeEventBus>,
        consumer_group: impl Into<String>,
    ) -> Self {
        Self {
            config,
            observer,
            command_executor,
            event_bus: Some(event_bus),
            consumer_group: consumer_group.into(),
            libraries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Attach notify watchers for the supplied library roots. Events are
    /// debounced and forwarded to the orchestrator command executor.
    pub async fn register_library(
        &self,
        library_id: LibraryId,
        roots: Vec<(LibraryRootsId, PathBuf)>,
    ) -> Result<()> {
        {
            let guard = self.libraries.read().await;
            if guard.contains_key(&library_id) {
                return Ok(());
            }
        }

        let resolved_roots = resolve_roots(roots);
        let capacity = self.config.max_batch_events.max(64) * 4;
        let (tx, rx) = mpsc::channel::<WatchMessage>(capacity);

        let flush_task = spawn_watch_loop(
            library_id,
            resolved_roots.clone(),
            Arc::clone(&self.observer),
            Arc::clone(&self.command_executor),
            self.event_bus.clone(),
            self.consumer_group.clone(),
            rx,
            self.config.clone(),
        );

        let mut guard = self.libraries.write().await;
        if guard.contains_key(&library_id) {
            flush_task.abort();
            return Ok(());
        }

        guard.insert(
            library_id,
            LibraryWatch {
                watchers: None,
                flush_task,
                tx: tx.clone(),
            },
        );
        drop(guard);

        if let Err(err) = replay_unacknowledged_events(
            library_id,
            &resolved_roots,
            Arc::clone(&self.observer),
            Arc::clone(&self.command_executor),
            self.event_bus.clone(),
            &self.consumer_group,
            self.config.max_batch_events,
        )
        .await
        {
            if let Some(watch) =
                self.libraries.write().await.remove(&library_id)
            {
                watch.shutdown();
            }
            return Err(err);
        }

        let libraries = Arc::clone(&self.libraries);
        let observer = Arc::clone(&self.observer);
        let watcher_roots = resolved_roots.clone();
        let watcher_tx = tx.clone();
        let watcher_config = self.config.clone();

        tokio::spawn(async move {
            let build_result = spawn_blocking(move || {
                init_watchers(watcher_config, watcher_roots, watcher_tx)
            })
            .await;

            match build_result {
                Ok(Ok(watchers)) => {
                    let mut guard = libraries.write().await;
                    if let Some(entry) = guard.get_mut(&library_id) {
                        entry.watchers = Some(watchers);
                        info!(%library_id, "fs_watch watchers initialized");
                    } else {
                        warn!(
                            %library_id,
                            "fs_watch init completed but library was already removed"
                        );
                    }
                }
                Ok(Err(err)) => {
                    let msg = err.to_string();
                    error!(
                        %library_id,
                        "fs_watch initialization failed: {}", msg
                    );
                    observer.on_error(library_id, &msg);
                    let mut guard = libraries.write().await;
                    if let Some(entry) = guard.remove(&library_id) {
                        entry.flush_task.abort();
                    }
                }
                Err(join_err) => {
                    let msg =
                        format!("watcher initialization panicked: {join_err}");
                    error!(
                        %library_id,
                        "fs_watch initialization panicked: {}", join_err
                    );
                    observer.on_error(library_id, &msg);
                    let mut guard = libraries.write().await;
                    if let Some(entry) = guard.remove(&library_id) {
                        entry.flush_task.abort();
                    }
                }
            }
        });

        drop(tx);

        Ok(())
    }

    /// Stop watching the specified library.
    pub async fn unregister_library(&self, library_id: LibraryId) {
        if let Some(watch) = self.libraries.write().await.remove(&library_id) {
            watch.shutdown();
        }
    }

    /// Tear down all registered watchers.
    pub async fn shutdown(&self) {
        let mut guard = self.libraries.write().await;
        let watches: Vec<_> = guard.drain().map(|(_, watch)| watch).collect();
        drop(guard);
        for watch in watches {
            watch.shutdown();
        }
    }

    #[cfg(test)]
    pub async fn watcher_count(&self) -> usize {
        self.libraries.read().await.len()
    }

    #[cfg(test)]
    async fn send_watch_message_for_test(
        &self,
        library_id: LibraryId,
        message: WatchMessage,
    ) -> Result<()> {
        let tx = {
            let guard = self.libraries.read().await;
            guard
                .get(&library_id)
                .map(|watch| watch.tx.clone())
                .ok_or_else(|| {
                    MediaError::Internal(format!(
                        "watcher not registered for library {library_id}"
                    ))
                })?
        };
        tx.send(message).await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to send test watch message: {err}"
            ))
        })
    }
}

struct LibraryWatch {
    watchers: Option<Vec<ActiveWatcher>>,
    flush_task: JoinHandle<()>,
    tx: mpsc::Sender<WatchMessage>,
}

impl LibraryWatch {
    fn shutdown(self) {
        let Self {
            watchers,
            flush_task,
            tx,
        } = self;
        // Drop watchers first — this stops the notify streams and
        // ensures all in-flight callbacks complete. Drop the retained sender
        // before aborting the flush task so no new test messages can enter.
        drop(watchers);
        drop(tx);
        flush_task.abort();
    }
}

impl fmt::Debug for LibraryWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let watcher_count =
            self.watchers.as_ref().map(|watchers| watchers.len());
        f.debug_struct("LibraryWatch")
            .field("watcher_count", &watcher_count)
            .field("flush_task_finished", &self.flush_task.is_finished())
            .field("tx_closed", &self.tx.is_closed())
            .finish()
    }
}

enum ActiveWatcher {
    Native { _watcher: RecommendedWatcher },
    Poll { _watcher: PollWatcher },
}

impl fmt::Debug for ActiveWatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            ActiveWatcher::Native { .. } => "Native",
            ActiveWatcher::Poll { .. } => "Poll",
        };
        f.debug_tuple("ActiveWatcher").field(&kind).finish()
    }
}

enum WatchMessage {
    Event(Event),
    Error(String),
}

struct NormalizedWatchEvent {
    root_id: LibraryRootsId,
    root_path: PathBuf,
    event: FileSystemEvent,
    file_size: Option<i64>,
    file_modified_at: Option<chrono::DateTime<chrono::Utc>>,
}

struct PendingWatchEvent {
    event_id: Option<Uuid>,
    event: FileSystemEvent,
}

enum PersistedWatchEvent {
    Dispatch { event_id: Option<Uuid> },
    Duplicate,
}

impl fmt::Debug for WatchMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WatchMessage::Event(event) => {
                let path_count = event.paths.len();
                f.debug_struct("WatchMessage::Event")
                    .field("kind", &event.kind)
                    .field("path_count", &path_count)
                    .finish()
            }
            WatchMessage::Error(message) => f
                .debug_struct("WatchMessage::Error")
                .field("message", message)
                .finish(),
        }
    }
}

fn spawn_watch_loop<O: FsWatchObserver + 'static>(
    library_id: LibraryId,
    roots: Vec<(LibraryRootsId, PathBuf)>,
    observer: Arc<O>,
    command_executor: Arc<dyn LibraryCommandExecutor>,
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    consumer_group: String,
    mut rx: mpsc::Receiver<WatchMessage>,
    config: FsWatchConfig,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut pending: HashMap<LibraryRootsId, Vec<PendingWatchEvent>> =
            HashMap::new();

        loop {
            let msg = if pending.is_empty() {
                rx.recv().await
            } else {
                match timeout(config.debounce_window, rx.recv()).await {
                    Ok(msg) => msg,
                    Err(_) => {
                        if let Err(err) = flush_pending(
                            Arc::clone(&observer),
                            library_id,
                            &mut pending,
                            &command_executor,
                            event_bus.clone(),
                            &consumer_group,
                        )
                        .await
                        {
                            observer.on_error(library_id, &err.to_string());
                        }
                        continue;
                    }
                }
            };

            let Some(msg) = msg else {
                if let Err(err) = flush_pending(
                    Arc::clone(&observer),
                    library_id,
                    &mut pending,
                    &command_executor,
                    event_bus.clone(),
                    &consumer_group,
                )
                .await
                {
                    observer.on_error(library_id, &err.to_string());
                }
                break;
            };

            match msg {
                WatchMessage::Event(event) => {
                    if let Some(normalized) =
                        convert_event(library_id, &roots, event)
                    {
                        let root_id = normalized.root_id;
                        match persist_normalized_event(
                            event_bus.clone(),
                            &normalized,
                        )
                        .await
                        {
                            Ok(PersistedWatchEvent::Dispatch { event_id }) => {
                                let pending_event = PendingWatchEvent {
                                    event_id,
                                    event: normalized.event,
                                };

                                if matches!(
                                    pending_event.event.kind,
                                    FileSystemEventKind::Overflow
                                ) {
                                    if let Err(err) = dispatch_events(
                                        Arc::clone(&observer),
                                        library_id,
                                        &command_executor,
                                        event_bus.clone(),
                                        &consumer_group,
                                        root_id,
                                        vec![pending_event],
                                    )
                                    .await
                                    {
                                        observer.on_error(
                                            library_id,
                                            &err.to_string(),
                                        );
                                    }
                                    continue;
                                }

                                let entry = pending.entry(root_id).or_default();
                                entry.push(pending_event);
                                if entry.len() >= config.max_batch_events {
                                    let events = std::mem::take(entry);
                                    if let Err(err) = dispatch_events(
                                        Arc::clone(&observer),
                                        library_id,
                                        &command_executor,
                                        event_bus.clone(),
                                        &consumer_group,
                                        root_id,
                                        events,
                                    )
                                    .await
                                    {
                                        observer.on_error(
                                            library_id,
                                            &err.to_string(),
                                        );
                                    }
                                }
                            }
                            Ok(PersistedWatchEvent::Duplicate) => {
                                trace!(
                                    %library_id,
                                    root_id = root_id.0,
                                    "skipping duplicate durable fs_watch event"
                                );
                            }
                            Err(err) => {
                                observer.on_error(library_id, &err.to_string());
                            }
                        }
                    }
                }
                WatchMessage::Error(error) => {
                    observer.on_error(library_id, &error);
                    if let Ok(overflow_events) =
                        overflow_for_roots(library_id, &roots)
                    {
                        for normalized in overflow_events {
                            let root_id = normalized.root_id;
                            match persist_normalized_event(
                                event_bus.clone(),
                                &normalized,
                            )
                            .await
                            {
                                Ok(PersistedWatchEvent::Dispatch {
                                    event_id,
                                }) => {
                                    let pending_event = PendingWatchEvent {
                                        event_id,
                                        event: normalized.event,
                                    };
                                    if let Err(err) = dispatch_events(
                                        Arc::clone(&observer),
                                        library_id,
                                        &command_executor,
                                        event_bus.clone(),
                                        &consumer_group,
                                        root_id,
                                        vec![pending_event],
                                    )
                                    .await
                                    {
                                        observer.on_error(
                                            library_id,
                                            &err.to_string(),
                                        );
                                    }
                                }
                                Ok(PersistedWatchEvent::Duplicate) => {
                                    trace!(
                                        %library_id,
                                        root_id = root_id.0,
                                        "skipping duplicate durable overflow event"
                                    );
                                }
                                Err(err) => {
                                    observer
                                        .on_error(library_id, &err.to_string());
                                }
                            }
                        }
                    };
                }
            }
        }
    })
}

async fn flush_pending<O: FsWatchObserver + 'static>(
    observer: Arc<O>,
    library_id: LibraryId,
    pending: &mut HashMap<LibraryRootsId, Vec<PendingWatchEvent>>,
    command_executor: &Arc<dyn LibraryCommandExecutor>,
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    consumer_group: &str,
) -> Result<()> {
    let mut batches = Vec::new();
    for (root_id, events) in pending.iter_mut() {
        if events.is_empty() {
            continue;
        }
        let drained = std::mem::take(events);
        batches.push((*root_id, drained));
    }

    for (root_id, events) in batches {
        dispatch_events(
            Arc::clone(&observer),
            library_id,
            command_executor,
            event_bus.clone(),
            consumer_group,
            root_id,
            events,
        )
        .await?;
    }
    pending.clear();
    Ok(())
}

async fn dispatch_events<O: FsWatchObserver + 'static>(
    observer: Arc<O>,
    library_id: LibraryId,
    command_executor: &Arc<dyn LibraryCommandExecutor>,
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    consumer_group: &str,
    root_id: LibraryRootsId,
    events: Vec<PendingWatchEvent>,
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    let mut pending_events = events;
    let mut actor_events: Vec<FileSystemEvent> = pending_events
        .iter()
        .map(|pending| pending.event.clone())
        .collect();

    let correlation_hint = actor_events
        .iter()
        .filter_map(|event| event.correlation_id)
        .next();

    if let Some(correlation) = correlation_hint {
        for event in actor_events.iter_mut() {
            if event.correlation_id.is_none() {
                event.correlation_id = Some(correlation);
            }
        }
    }

    let command = LibraryActorCommand::FsEvents {
        root: root_id,
        events: actor_events,
        correlation_id: correlation_hint,
    };

    if let Err(err) = command_executor
        .execute_library_command(library_id, command)
        .await
    {
        observer.on_error(library_id, &err.to_string());
        return Err(err);
    }

    if let Some(event_bus) = event_bus {
        for event_id in pending_events
            .drain(..)
            .filter_map(|pending| pending.event_id)
        {
            event_bus.ack(consumer_group, event_id).await?;
        }
    }

    Ok(())
}

async fn persist_normalized_event(
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    normalized: &NormalizedWatchEvent,
) -> Result<PersistedWatchEvent> {
    let Some(event_bus) = event_bus else {
        return Ok(PersistedWatchEvent::Dispatch { event_id: None });
    };

    let event_id = Uuid::now_v7();
    let record = to_file_watch_event(event_id, normalized);
    if event_bus.publish(record).await? {
        Ok(PersistedWatchEvent::Dispatch {
            event_id: Some(event_id),
        })
    } else {
        Ok(PersistedWatchEvent::Duplicate)
    }
}

fn to_file_watch_event(
    event_id: Uuid,
    normalized: &NormalizedWatchEvent,
) -> FileWatchEvent {
    let event = &normalized.event;
    FileWatchEvent {
        id: event_id,
        event_version: i32::from(event.version),
        library_id: event.library_id,
        library_root_id: i32::from(normalized.root_id.0),
        root_path: normalized.root_path.to_string_lossy().to_string(),
        event_type: file_watch_event_type(&event.kind),
        file_path: event.path.to_string_lossy().to_string(),
        path_key: event.path_key.clone(),
        old_path: event
            .old_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        fingerprint: event.fingerprint.clone(),
        file_size: normalized.file_size,
        file_modified_at: normalized.file_modified_at,
        correlation_id: event.correlation_id,
        idempotency_key: event.idempotency_key.clone(),
        detected_at: event.occurred_at,
        processed: false,
        processed_at: None,
        processing_attempts: 0,
        last_error: None,
    }
}

fn file_watch_event_type(kind: &FileSystemEventKind) -> FileWatchEventType {
    match kind {
        FileSystemEventKind::Created => FileWatchEventType::Created,
        FileSystemEventKind::Modified => FileWatchEventType::Modified,
        FileSystemEventKind::Deleted => FileWatchEventType::Deleted,
        FileSystemEventKind::Moved => FileWatchEventType::Moved,
        FileSystemEventKind::Overflow => FileWatchEventType::Overflow,
    }
}

fn file_system_event_kind(kind: &FileWatchEventType) -> FileSystemEventKind {
    match kind {
        FileWatchEventType::Created => FileSystemEventKind::Created,
        FileWatchEventType::Modified => FileSystemEventKind::Modified,
        FileWatchEventType::Deleted => FileSystemEventKind::Deleted,
        FileWatchEventType::Moved => FileSystemEventKind::Moved,
        FileWatchEventType::Overflow => FileSystemEventKind::Overflow,
    }
}

async fn replay_unacknowledged_events<O: FsWatchObserver + 'static>(
    library_id: LibraryId,
    roots: &[(LibraryRootsId, PathBuf)],
    observer: Arc<O>,
    command_executor: Arc<dyn LibraryCommandExecutor>,
    event_bus: Option<Arc<dyn FileChangeEventBus>>,
    consumer_group: &str,
    batch_limit: usize,
) -> Result<()> {
    let Some(event_bus) = event_bus else {
        return Ok(());
    };

    let limit = batch_limit.clamp(1, i32::MAX as usize) as i32;
    loop {
        let events =
            event_bus.get_unprocessed_events(library_id, limit).await?;
        if events.is_empty() {
            return Ok(());
        }

        let mut current_root: Option<LibraryRootsId> = None;
        let mut batch = Vec::new();

        for record in events {
            let pending = replay_record_to_pending(&record, roots)?;
            let root_id = LibraryRootsId(record.library_root_id as u16);
            if let Some(active_root) = current_root {
                if active_root != root_id {
                    dispatch_events(
                        Arc::clone(&observer),
                        library_id,
                        &command_executor,
                        Some(Arc::clone(&event_bus)),
                        consumer_group,
                        active_root,
                        std::mem::take(&mut batch),
                    )
                    .await?;
                }
            }

            current_root = Some(root_id);
            batch.push(pending);
        }

        if let Some(root_id) = current_root {
            dispatch_events(
                Arc::clone(&observer),
                library_id,
                &command_executor,
                Some(Arc::clone(&event_bus)),
                consumer_group,
                root_id,
                batch,
            )
            .await?;
        }
    }
}

fn replay_record_to_pending(
    record: &FileWatchEvent,
    roots: &[(LibraryRootsId, PathBuf)],
) -> Result<PendingWatchEvent> {
    let root_id = u16::try_from(record.library_root_id).map_err(|_| {
        MediaError::Internal(format!(
            "invalid file watch root id {} for event {}",
            record.library_root_id, record.id
        ))
    })?;
    let root_id = LibraryRootsId(root_id);

    if !roots.iter().any(|(candidate, _)| *candidate == root_id) {
        return Err(MediaError::Internal(format!(
            "file watch event {} references unregistered root {}",
            record.id, record.library_root_id
        )));
    }

    Ok(PendingWatchEvent {
        event_id: Some(record.id),
        event: FileSystemEvent {
            version: u16::try_from(record.event_version)
                .unwrap_or(EVENT_VERSION),
            correlation_id: record.correlation_id,
            idempotency_key: record.idempotency_key.clone(),
            library_id: record.library_id,
            path_key: record.path_key.clone(),
            fingerprint: record.fingerprint.clone(),
            path: PathBuf::from(&record.file_path),
            old_path: record.old_path.as_ref().map(|path| PathBuf::from(path)),
            kind: file_system_event_kind(&record.event_type),
            occurred_at: record.detected_at,
        },
    })
}

fn convert_event(
    library_id: LibraryId,
    roots: &[(LibraryRootsId, PathBuf)],
    event: Event,
) -> Option<NormalizedWatchEvent> {
    let (root_id, root_path) = locate_root(&event, roots)?;

    let (path, old_path) = extract_paths(&event, root_path)?;
    let kind = classify_event(&event.kind);

    let path_key = normalize_path(&path).ok()?;
    let old_path_key =
        old_path.as_ref().and_then(|path| normalize_path(path).ok());
    let occurred_at = chrono::Utc::now();
    let (file_size, file_modified_at) = file_metadata(&path);
    let metadata_token = file_modified_at
        .as_ref()
        .map(|modified| {
            modified
                .timestamp_nanos_opt()
                .unwrap_or_default()
                .to_string()
        })
        .or_else(|| file_size.map(|size| size.to_string()))
        .unwrap_or_else(|| occurred_at.timestamp().to_string());
    let kind_token = format!("{:?}", kind);
    let idempotency_key = encode_hash(&[
        "fs",
        &library_id.to_string(),
        &root_id.0.to_string(),
        &kind_token,
        &path_key,
        old_path_key.as_deref().unwrap_or(""),
        &metadata_token,
    ]);

    let event = FileSystemEvent {
        version: EVENT_VERSION,
        correlation_id: None,
        idempotency_key,
        library_id,
        path_key,
        fingerprint: None,
        path,
        old_path,
        kind,
        occurred_at,
    };

    Some(NormalizedWatchEvent {
        root_id,
        root_path: root_path.clone(),
        event,
        file_size,
        file_modified_at,
    })
}

fn locate_root<'a>(
    event: &Event,
    roots: &'a [(LibraryRootsId, PathBuf)],
) -> Option<(LibraryRootsId, &'a PathBuf)> {
    let primary = event.paths.first()?;
    for (root_id, root_path) in roots {
        if path_within_root(primary, root_path) {
            return Some((*root_id, root_path));
        }
    }
    None
}

fn path_within_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn extract_paths(
    event: &Event,
    root_path: &Path,
) -> Option<(PathBuf, Option<PathBuf>)> {
    let mut paths = event.paths.iter();
    let first = paths.next()?;

    match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let old = sanitize_path(root_path, first)?;
            let second = paths.next().and_then(|p| sanitize_path(root_path, p));
            let new = second.unwrap_or_else(|| old.clone());
            Some((fallback_root(root_path, new), Some(old)))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            let old = sanitize_path(root_path, first)?;
            Some((fallback_root(root_path, old.clone()), Some(old)))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            let new = sanitize_path(root_path, first)?;
            Some((fallback_root(root_path, new), None))
        }
        EventKind::Other => Some((root_path.to_path_buf(), None)),
        _ => {
            let new = sanitize_path(root_path, first)?;
            Some((fallback_root(root_path, new), None))
        }
    }
}

fn fallback_root(root_path: &Path, candidate: PathBuf) -> PathBuf {
    if candidate.as_os_str().is_empty() {
        root_path.to_path_buf()
    } else {
        candidate
    }
}

fn classify_event(kind: &EventKind) -> FileSystemEventKind {
    match kind {
        EventKind::Create(_) => FileSystemEventKind::Created,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
            FileSystemEventKind::Modified
        }
        EventKind::Modify(ModifyKind::Name(_)) => FileSystemEventKind::Moved,
        EventKind::Remove(
            RemoveKind::File | RemoveKind::Folder | RemoveKind::Any,
        ) => FileSystemEventKind::Deleted,
        EventKind::Other => FileSystemEventKind::Overflow,
        _ => FileSystemEventKind::Modified,
    }
}

fn file_metadata(
    path: &Path,
) -> (Option<i64>, Option<chrono::DateTime<chrono::Utc>>) {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let size = i64::try_from(metadata.len()).ok();
            let modified = metadata
                .modified()
                .ok()
                .map(chrono::DateTime::<chrono::Utc>::from);
            (size, modified)
        }
        Err(_) => (None, None),
    }
}

fn sanitize_path(root: &Path, path: &Path) -> Option<PathBuf> {
    if !path_within_root(path, root) {
        return None;
    }

    let rel = path.strip_prefix(root).ok()?;
    let mut clean = PathBuf::new();
    for component in rel.components() {
        match component {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            Component::ParentDir => {
                if !clean.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }

    let mut normalized = root.to_path_buf();
    normalized.push(clean);
    Some(normalized)
}

fn overflow_for_roots(
    library_id: LibraryId,
    roots: &[(LibraryRootsId, PathBuf)],
) -> Result<Vec<NormalizedWatchEvent>> {
    roots
        .iter()
        .map(|(root_id, root_path)| -> Result<_> {
            let path_key = normalize_path(root_path)?;
            let occurred_at = chrono::Utc::now();
            let overflow_token = occurred_at
                .timestamp_nanos_opt()
                .unwrap_or_default()
                .to_string();
            let idempotency_key = encode_hash(&[
                "fs-overflow",
                &library_id.to_string(),
                &root_id.0.to_string(),
                &path_key,
                &overflow_token,
            ]);

            let event = FileSystemEvent {
                version: EVENT_VERSION,
                correlation_id: None,
                idempotency_key,
                library_id,
                path_key,
                fingerprint: None,
                path: root_path.clone(),
                old_path: None,
                kind: FileSystemEventKind::Overflow,
                occurred_at,
            };

            Ok(NormalizedWatchEvent {
                root_id: *root_id,
                root_path: root_path.clone(),
                event,
                file_size: None,
                file_modified_at: None,
            })
        })
        .collect()
}

fn resolve_roots(
    roots: Vec<(LibraryRootsId, PathBuf)>,
) -> Vec<(LibraryRootsId, PathBuf)> {
    let cwd =
        env::current_dir().unwrap_or_else(|_| PathBuf::from("../../../../.."));
    roots
        .into_iter()
        .map(|(id, path)| {
            if path.is_absolute() {
                (id, path)
            } else {
                (id, cwd.join(path))
            }
        })
        .collect()
}

fn init_watchers(
    config: FsWatchConfig,
    watcher_roots: Vec<(LibraryRootsId, PathBuf)>,
    watcher_tx: mpsc::Sender<WatchMessage>,
) -> Result<Vec<ActiveWatcher>> {
    let mut watchers = Vec::with_capacity(watcher_roots.len());
    for (_root_id, root_path) in &watcher_roots {
        match config.strategy {
            WatchStrategy::Native => {
                let watcher =
                    build_native_watcher(root_path, watcher_tx.clone())?;
                watchers.push(ActiveWatcher::Native { _watcher: watcher });
            }
            WatchStrategy::Poll => {
                let poller = build_poll_watcher(
                    root_path,
                    watcher_tx.clone(),
                    config.poll_interval,
                )?;
                watchers.push(ActiveWatcher::Poll { _watcher: poller });
            }
            WatchStrategy::Auto => {
                match build_native_watcher(root_path, watcher_tx.clone()) {
                    Ok(watcher) => watchers
                        .push(ActiveWatcher::Native { _watcher: watcher }),
                    Err(native_err) => {
                        let native_err_msg = native_err.to_string();
                        warn!(
                            path = %root_path.display(),
                            "native watcher unavailable, falling back to polling: {}",
                            native_err_msg
                        );

                        match build_poll_watcher(
                            root_path,
                            watcher_tx.clone(),
                            config.poll_interval,
                        ) {
                            Ok(poller) => watchers
                                .push(ActiveWatcher::Poll { _watcher: poller }),
                            Err(poll_err) => {
                                let poll_err_msg = poll_err.to_string();
                                return Err(MediaError::Internal(format!(
                                    "failed to watch {} (native error: {}; polling error: {})",
                                    root_path.display(),
                                    native_err_msg,
                                    poll_err_msg
                                )));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(watchers)
}

fn build_native_watcher(
    root_path: &Path,
    watcher_tx: mpsc::Sender<WatchMessage>,
) -> Result<RecommendedWatcher> {
    let handler = build_event_handler(watcher_tx, root_path.to_path_buf());
    let mut watcher = RecommendedWatcher::new(handler, NotifyConfig::default())
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to create watcher for {}: {}",
                root_path.display(),
                err
            ))
        })?;

    watcher
        .watch(root_path, RecursiveMode::Recursive)
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to watch {}: {}",
                root_path.display(),
                err
            ))
        })?;

    Ok(watcher)
}

fn build_poll_watcher(
    root_path: &Path,
    watcher_tx: mpsc::Sender<WatchMessage>,
    poll_interval: Duration,
) -> Result<PollWatcher> {
    let handler = build_event_handler(watcher_tx, root_path.to_path_buf());
    let config = NotifyConfig::default().with_poll_interval(poll_interval);
    let mut watcher = PollWatcher::new(handler, config).map_err(|err| {
        MediaError::Internal(format!(
            "failed to create poll watcher for {}: {}",
            root_path.display(),
            err
        ))
    })?;

    watcher
        .watch(root_path, RecursiveMode::Recursive)
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to watch {} via polling: {}",
                root_path.display(),
                err
            ))
        })?;

    Ok(watcher)
}

fn build_event_handler(
    tx_event: mpsc::Sender<WatchMessage>,
    path_hint: PathBuf,
) -> impl FnMut(std::result::Result<Event, notify::Error>) + Send + 'static {
    let channel_closed = AtomicBool::new(false);
    move |res| {
        if channel_closed.load(Ordering::Relaxed) {
            return;
        }

        let message = match res {
            Ok(event) => WatchMessage::Event(event),
            Err(err) => WatchMessage::Error(err.to_string()),
        };

        match tx_event.try_send(message) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(
                    "fs_watch channel full for {}; scheduling overflow replay",
                    path_hint.display()
                );
                if tx_event
                    .blocking_send(WatchMessage::Error(
                        "fs_watch channel full; persisted overflow fallback"
                            .into(),
                    ))
                    .is_err()
                    && !channel_closed.swap(true, Ordering::Relaxed)
                {
                    warn!(
                        "fs_watch channel send failed for {}: channel closed",
                        path_hint.display()
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                if !channel_closed.swap(true, Ordering::Relaxed) {
                    warn!(
                        "fs_watch channel send failed for {}: channel closed",
                        path_hint.display()
                    );
                }
            }
        }
    }
}

fn encode_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(&digest[..16])
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use notify::Event;
    use notify::event::{CreateKind, DataChange, EventKind, ModifyKind};
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use tokio::time;
    use uuid::Uuid;

    use super::{
        EVENT_VERSION, FsWatchConfig, FsWatchService, NoopFsWatchObserver,
        WatchConfig, WatchMessage, WatchStrategy, encode_hash,
    };
    use crate::database::traits::{FileWatchEvent, FileWatchEventType};
    use crate::domain::scan::fs_watch::event_bus::FileChangeEventBus;
    use crate::domain::scan::orchestration::lease::DequeueRequest;
    use crate::domain::scan::orchestration::scan_cursor::normalize_path;
    use crate::domain::scan::orchestration::{
        CorrelationCache, DefaultLibraryActor, DependencyKey, DispatchStatus,
        EnqueueRequest, FileSystemEvent, FileSystemEventKind, FolderScanJob,
        InMemoryBudget, InProcJobEventBus, JobDispatcher, JobEvent,
        JobEventPayload, JobHandle, JobId, JobKind, JobLease, JobPayload,
        JobPriority, LeaseExpiryScanner, LeaseId, LeaseRenewal,
        LibraryActorCommand, LibraryActorConfig, LibraryActorHandle,
        LibraryCommandExecutor, LibraryRootsId, NoopActorObserver,
        OrchestratorConfig, OrchestratorRuntime, OrchestratorRuntimeBuilder,
        QueueService, ScanReason,
    };
    use crate::error::{MediaError, Result};
    use crate::types::{
        LibraryType, ids::LibraryId, prelude::LibraryReference,
    };

    type TestRuntime =
        OrchestratorRuntime<RecordingQueue, InProcJobEventBus, InMemoryBudget>;

    #[test]
    fn fs_watch_config_preserves_forced_poll_strategy() {
        let mut watch = WatchConfig::default();
        watch.strategy = WatchStrategy::Poll;
        watch.poll_interval_ms = 2_500;
        watch.poll_backoff_max_ms = 42_000;

        let config = FsWatchConfig::from(watch);

        assert_eq!(config.strategy, WatchStrategy::Poll);
        assert_eq!(config.poll_interval, Duration::from_millis(2_500));
        assert_eq!(config.poll_backoff_max, Duration::from_millis(42_000));
    }

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        job: FolderScanJob,
        priority: JobPriority,
        correlation_id: Option<Uuid>,
    }

    #[derive(Clone, Default)]
    struct RecordingQueue {
        records: Arc<Mutex<Vec<RecordedRequest>>>,
        accepted_by_dedupe: Arc<Mutex<HashMap<String, JobId>>>,
    }

    impl std::fmt::Debug for RecordingQueue {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let queued = self
                .records
                .try_lock()
                .map(|records| records.len())
                .unwrap_or_default();
            f.debug_struct("RecordingQueue")
                .field("queued", &queued)
                .finish()
        }
    }

    impl RecordingQueue {
        async fn records(&self) -> Vec<RecordedRequest> {
            self.records.lock().await.clone()
        }
    }

    #[async_trait]
    impl QueueService for RecordingQueue {
        async fn enqueue(&self, request: EnqueueRequest) -> Result<JobHandle> {
            let payload = request.payload.clone();
            let dedupe_key = request.dedupe_key().to_string();

            let mut accepted = self.accepted_by_dedupe.lock().await;
            if let Some(existing) = accepted.get(&dedupe_key).copied() {
                return Ok(JobHandle::merged(
                    existing,
                    &payload,
                    request.priority,
                ));
            }

            let job_id = JobId::new();
            accepted.insert(dedupe_key, job_id);
            drop(accepted);

            if let JobPayload::FolderScan(job) = payload.clone() {
                self.records.lock().await.push(RecordedRequest {
                    job,
                    priority: request.priority,
                    correlation_id: request.correlation_id,
                });
            }

            Ok(JobHandle::accepted(job_id, &payload, request.priority))
        }

        async fn dequeue(
            &self,
            _request: DequeueRequest,
        ) -> Result<Option<JobLease>> {
            Ok(None)
        }

        async fn renew(&self, _renewal: LeaseRenewal) -> Result<JobLease> {
            Err(MediaError::Internal(
                "renew not implemented in RecordingQueue".into(),
            ))
        }

        async fn complete(&self, _lease_id: LeaseId) -> Result<()> {
            Ok(())
        }

        async fn fail(
            &self,
            _lease_id: LeaseId,
            _retryable: bool,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn dead_letter(
            &self,
            _lease_id: LeaseId,
            _error: Option<String>,
        ) -> Result<()> {
            Ok(())
        }

        async fn cancel_job(&self, _job_id: JobId) -> Result<()> {
            Ok(())
        }

        async fn queue_depth(&self, kind: JobKind) -> Result<usize> {
            Ok(self
                .records()
                .await
                .into_iter()
                .filter(|record| {
                    JobPayload::FolderScan(record.job.clone()).kind() == kind
                })
                .count())
        }

        async fn release_dependency(
            &self,
            _library_id: LibraryId,
            _dependency_key: &DependencyKey,
        ) -> Result<u64> {
            Ok(0)
        }
    }

    #[async_trait]
    impl LeaseExpiryScanner for RecordingQueue {
        async fn scan_expired_leases(&self) -> Result<u64> {
            Ok(0)
        }
    }

    #[derive(Debug)]
    struct NoopDispatcher;

    #[async_trait]
    impl JobDispatcher for NoopDispatcher {
        async fn dispatch(&self, _lease: &JobLease) -> DispatchStatus {
            DispatchStatus::Success
        }
    }

    #[derive(Debug)]
    struct NoopCommandExecutor;

    #[async_trait]
    impl LibraryCommandExecutor for NoopCommandExecutor {
        async fn execute_library_command(
            &self,
            _library_id: LibraryId,
            _command: LibraryActorCommand,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct MemoryFileChangeEventBus {
        events: Arc<Mutex<Vec<FileWatchEvent>>>,
        acked: Arc<Mutex<Vec<Uuid>>>,
    }

    impl MemoryFileChangeEventBus {
        async fn events(&self) -> Vec<FileWatchEvent> {
            self.events.lock().await.clone()
        }

        async fn acked(&self) -> Vec<Uuid> {
            self.acked.lock().await.clone()
        }

        async fn clear(&self) {
            self.events.lock().await.clear();
            self.acked.lock().await.clear();
        }
    }

    #[async_trait]
    impl FileChangeEventBus for MemoryFileChangeEventBus {
        async fn publish(&self, event: FileWatchEvent) -> Result<bool> {
            let mut events = self.events.lock().await;
            if events.iter().any(|existing| {
                existing.idempotency_key == event.idempotency_key
            }) {
                return Ok(false);
            }
            events.push(event);
            Ok(true)
        }

        async fn ack(&self, _group: &str, event_id: Uuid) -> Result<()> {
            let mut events = self.events.lock().await;
            let event = events
                .iter_mut()
                .find(|event| event.id == event_id)
                .ok_or_else(|| {
                    MediaError::Internal(format!(
                        "missing in-memory file watch event {event_id}"
                    ))
                })?;
            event.processed = true;
            event.processed_at = Some(Utc::now());
            drop(events);
            self.acked.lock().await.push(event_id);
            Ok(())
        }

        async fn get_unprocessed_events(
            &self,
            library_id: LibraryId,
            limit: i32,
        ) -> Result<Vec<FileWatchEvent>> {
            let mut events: Vec<_> = self
                .events
                .lock()
                .await
                .iter()
                .filter(|event| {
                    event.library_id == library_id && !event.processed
                })
                .cloned()
                .collect();
            events.sort_by(|left, right| {
                left.detected_at
                    .cmp(&right.detected_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            events.truncate(limit.max(0) as usize);
            Ok(events)
        }

        async fn mark_processed(&self, event_id: Uuid) -> Result<()> {
            self.ack("test", event_id).await
        }

        async fn cleanup_retention(&self, _days_to_keep: i32) -> Result<u32> {
            Ok(0)
        }
    }

    #[derive(Clone, Default)]
    struct RecordingCommandExecutor {
        commands: Arc<Mutex<Vec<LibraryActorCommand>>>,
        fail: bool,
        assert_unacked_on_execute: Option<Arc<MemoryFileChangeEventBus>>,
    }

    impl RecordingCommandExecutor {
        async fn commands(&self) -> Vec<LibraryActorCommand> {
            self.commands.lock().await.clone()
        }

        async fn clear(&self) {
            self.commands.lock().await.clear();
        }
    }

    #[async_trait]
    impl LibraryCommandExecutor for RecordingCommandExecutor {
        async fn execute_library_command(
            &self,
            _library_id: LibraryId,
            command: LibraryActorCommand,
        ) -> Result<()> {
            if let Some(bus) = &self.assert_unacked_on_execute {
                let events = bus.events().await;
                assert!(
                    events.iter().any(|event| !event.processed),
                    "event must be durably persisted and unacked before handoff"
                );
            }
            self.commands.lock().await.push(command);
            if self.fail {
                Err(MediaError::Internal("handoff failed".into()))
            } else {
                Ok(())
            }
        }
    }

    struct RuntimeHarness {
        runtime: Arc<TestRuntime>,
        queue: Arc<RecordingQueue>,
        events: Arc<InProcJobEventBus>,
        library_id: LibraryId,
    }

    async fn runtime_harness(root: PathBuf) -> Result<RuntimeHarness> {
        let library_id = LibraryId::new();
        let queue = Arc::new(RecordingQueue::default());
        let events = Arc::new(InProcJobEventBus::new(32));
        let config = OrchestratorConfig::default();
        let budget = Arc::new(InMemoryBudget::new(config.budget.clone()));
        let dispatcher = Arc::new(NoopDispatcher);

        let runtime = Arc::new(
            OrchestratorRuntimeBuilder::new(config)
                .with_queue(Arc::clone(&queue))
                .with_events(Arc::clone(&events))
                .with_budget(budget)
                .with_dispatcher(dispatcher)
                .with_correlations(CorrelationCache::default())
                .build()?,
        );

        let actor_config = LibraryActorConfig {
            library: LibraryReference {
                id: library_id,
                name: "Watch Test".into(),
                library_type: LibraryType::Movies,
                paths: vec![root.clone()],
            },
            root_paths: vec![root.clone()],
            max_outstanding_jobs: 16,
        };
        let actor = DefaultLibraryActor::new(
            actor_config,
            Arc::clone(&queue),
            Arc::new(NoopActorObserver),
            Arc::clone(&events),
            CorrelationCache::default(),
        );
        let actor: LibraryActorHandle = Arc::new(Mutex::new(Box::new(actor)));

        runtime.register_library_actor(library_id, actor).await?;
        runtime.start_mailbox_runner().await?;

        Ok(RuntimeHarness {
            runtime,
            queue,
            events,
            library_id,
        })
    }

    fn make_fs_event(
        library_id: LibraryId,
        path: &Path,
        kind: FileSystemEventKind,
        correlation_id: Option<Uuid>,
    ) -> Result<FileSystemEvent> {
        let path_key = normalize_path(path)?;
        Ok(FileSystemEvent {
            version: EVENT_VERSION,
            correlation_id,
            idempotency_key: encode_hash(&[
                "fs-test",
                &library_id.to_string(),
                &path_key,
            ]),
            library_id,
            path_key,
            fingerprint: None,
            path: path.to_path_buf(),
            old_path: None,
            kind,
            occurred_at: Utc::now(),
        })
    }

    async fn wait_for_records(
        queue: &RecordingQueue,
        expected: usize,
    ) -> Vec<RecordedRequest> {
        time::timeout(Duration::from_secs(2), async {
            loop {
                let records = queue.records().await;
                if records.len() >= expected {
                    return records;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("queued folder scan records")
    }

    async fn wait_for_enqueued_event(
        rx: &mut tokio::sync::broadcast::Receiver<JobEvent>,
    ) -> JobEvent {
        time::timeout(Duration::from_secs(2), async {
            loop {
                let event = rx.recv().await.expect("job event");
                if matches!(event.payload, JobEventPayload::Enqueued { .. }) {
                    return event;
                }
            }
        })
        .await
        .expect("published enqueue event")
    }

    async fn wait_for_commands(
        executor: &RecordingCommandExecutor,
        expected: usize,
    ) -> Vec<LibraryActorCommand> {
        time::timeout(Duration::from_secs(2), async {
            loop {
                let commands = executor.commands().await;
                if commands.len() >= expected {
                    return commands;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("recorded library commands")
    }

    async fn wait_for_published_events(
        bus: &MemoryFileChangeEventBus,
        expected: usize,
    ) -> Vec<FileWatchEvent> {
        time::timeout(Duration::from_secs(2), async {
            loop {
                let events = bus.events().await;
                if events.len() >= expected {
                    return events;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("published durable file watch events")
    }

    async fn clear_startup_watch_noise(
        bus: &MemoryFileChangeEventBus,
        executor: &RecordingCommandExecutor,
    ) {
        time::sleep(Duration::from_millis(50)).await;
        bus.clear().await;
        executor.clear().await;
    }

    fn durable_event(
        library_id: LibraryId,
        root: &Path,
        path: &Path,
        kind: FileWatchEventType,
    ) -> Result<FileWatchEvent> {
        let id = Uuid::now_v7();
        Ok(FileWatchEvent {
            id,
            event_version: i32::from(EVENT_VERSION),
            library_id,
            library_root_id: 0,
            root_path: root.to_string_lossy().to_string(),
            event_type: kind,
            file_path: path.to_string_lossy().to_string(),
            path_key: normalize_path(path)?,
            old_path: None,
            fingerprint: None,
            file_size: Some(5),
            file_modified_at: Some(Utc::now()),
            correlation_id: Some(Uuid::now_v7()),
            idempotency_key: format!("test:{library_id}:{id}"),
            detected_at: Utc::now(),
            processed: false,
            processed_at: None,
            processing_attempts: 0,
            last_error: None,
        })
    }

    #[tokio::test]
    async fn registers_and_unregisters_library() -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let service: FsWatchService = FsWatchService::new(
            FsWatchConfig::default(),
            Arc::new(NoopFsWatchObserver),
            Arc::new(NoopCommandExecutor),
        );

        let library_id = LibraryId::new();
        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;
        assert_eq!(service.watcher_count().await, 1);
        service.unregister_library(library_id).await;
        assert_eq!(service.watcher_count().await, 0);
        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_persists_before_dispatch_and_acks_after_handoff()
    -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Durable A");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let library_id = LibraryId::new();
        let bus = Arc::new(MemoryFileChangeEventBus::default());
        let event_bus: Arc<dyn FileChangeEventBus> = bus.clone();
        let executor = RecordingCommandExecutor {
            assert_unacked_on_execute: Some(Arc::clone(&bus)),
            ..RecordingCommandExecutor::default()
        };
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            Arc::new(executor.clone());
        let service: FsWatchService = FsWatchService::with_event_bus(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
            event_bus,
        );

        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;
        clear_startup_watch_noise(&bus, &executor).await;
        service
            .send_watch_message_for_test(
                library_id,
                WatchMessage::Event(
                    Event::new(EventKind::Create(CreateKind::File))
                        .add_path(media_path),
                ),
            )
            .await?;

        let commands = wait_for_commands(&executor, 1).await;
        assert!(matches!(commands[0], LibraryActorCommand::FsEvents { .. }));
        let events = wait_for_published_events(&bus, 1).await;
        assert_eq!(events[0].event_type, FileWatchEventType::Created);
        assert_eq!(events[0].library_root_id, 0);
        assert!(events[0].processed);
        assert_eq!(bus.acked().await, vec![events[0].id]);
        service.unregister_library(library_id).await;
        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_leaves_durable_event_unacked_when_handoff_fails()
    -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Durable B");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let library_id = LibraryId::new();
        let bus = Arc::new(MemoryFileChangeEventBus::default());
        let event_bus: Arc<dyn FileChangeEventBus> = bus.clone();
        let executor = RecordingCommandExecutor {
            fail: true,
            ..RecordingCommandExecutor::default()
        };
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            Arc::new(executor.clone());
        let service: FsWatchService = FsWatchService::with_event_bus(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
            event_bus,
        );

        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;
        clear_startup_watch_noise(&bus, &executor).await;
        service
            .send_watch_message_for_test(
                library_id,
                WatchMessage::Event(
                    Event::new(EventKind::Modify(ModifyKind::Data(
                        DataChange::Content,
                    )))
                    .add_path(media_path),
                ),
            )
            .await?;

        let _commands = wait_for_commands(&executor, 1).await;
        let events = wait_for_published_events(&bus, 1).await;
        assert_eq!(events[0].event_type, FileWatchEventType::Modified);
        assert!(!events[0].processed);
        assert!(bus.acked().await.is_empty());
        service.unregister_library(library_id).await;
        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_replays_unacked_events_on_register() -> Result<()>
    {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Durable C");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let library_id = LibraryId::new();
        let bus = Arc::new(MemoryFileChangeEventBus::default());
        let stored = durable_event(
            library_id,
            &root,
            &media_path,
            FileWatchEventType::Created,
        )?;
        assert!(bus.publish(stored.clone()).await?);
        let event_bus: Arc<dyn FileChangeEventBus> = bus.clone();
        let executor = RecordingCommandExecutor::default();
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            Arc::new(executor.clone());
        let service: FsWatchService = FsWatchService::with_event_bus(
            FsWatchConfig::default(),
            Arc::new(NoopFsWatchObserver),
            command_executor,
            event_bus,
        );

        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;

        let commands = wait_for_commands(&executor, 1).await;
        assert!(matches!(commands[0], LibraryActorCommand::FsEvents { .. }));
        assert_eq!(bus.acked().await, vec![stored.id]);
        assert!(bus.events().await[0].processed);
        service.unregister_library(library_id).await;
        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_skips_duplicate_idempotency_keys() -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Durable D");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let library_id = LibraryId::new();
        let bus = Arc::new(MemoryFileChangeEventBus::default());
        let event_bus: Arc<dyn FileChangeEventBus> = bus.clone();
        let executor = RecordingCommandExecutor::default();
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            Arc::new(executor.clone());
        let service: FsWatchService = FsWatchService::with_event_bus(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
            event_bus,
        );

        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;
        clear_startup_watch_noise(&bus, &executor).await;
        for _ in 0..2 {
            service
                .send_watch_message_for_test(
                    library_id,
                    WatchMessage::Event(
                        Event::new(EventKind::Modify(ModifyKind::Data(
                            DataChange::Content,
                        )))
                        .add_path(media_path.clone()),
                    ),
                )
                .await?;
        }

        let commands = wait_for_commands(&executor, 1).await;
        assert_eq!(commands.len(), 1);
        assert_eq!(wait_for_published_events(&bus, 1).await.len(), 1);
        time::sleep(Duration::from_millis(100)).await;
        assert_eq!(executor.commands().await.len(), 1);
        assert_eq!(bus.events().await.len(), 1);
        service.unregister_library(library_id).await;
        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_persists_overflow_records() -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Durable E");
        std::fs::create_dir_all(&movie_dir).unwrap();

        let library_id = LibraryId::new();
        let bus = Arc::new(MemoryFileChangeEventBus::default());
        let event_bus: Arc<dyn FileChangeEventBus> = bus.clone();
        let executor = RecordingCommandExecutor::default();
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            Arc::new(executor.clone());
        let service: FsWatchService = FsWatchService::with_event_bus(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
            event_bus,
        );

        service
            .register_library(library_id, vec![(LibraryRootsId(0), root)])
            .await?;
        clear_startup_watch_noise(&bus, &executor).await;
        service
            .send_watch_message_for_test(
                library_id,
                WatchMessage::Error("overflow".into()),
            )
            .await?;

        let _commands = wait_for_commands(&executor, 1).await;
        let events = wait_for_published_events(&bus, 1).await;
        assert_eq!(events[0].event_type, FileWatchEventType::Overflow);
        assert!(events[0].processed);
        service.unregister_library(library_id).await;
        Ok(())
    }

    #[tokio::test]
    async fn direct_fs_events_command_enqueues_deduped_folder_scan()
    -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Movie A");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let harness = runtime_harness(root).await?;
        let correlation_id = Uuid::now_v7();
        let mut job_rx = harness.events.subscribe();
        let events = vec![
            make_fs_event(
                harness.library_id,
                &media_path,
                FileSystemEventKind::Created,
                Some(correlation_id),
            )?,
            make_fs_event(
                harness.library_id,
                &media_path,
                FileSystemEventKind::Modified,
                None,
            )?,
        ];

        harness
            .runtime
            .submit_library_command(
                harness.library_id,
                LibraryActorCommand::FsEvents {
                    root: LibraryRootsId(0),
                    events,
                    correlation_id: Some(correlation_id),
                },
            )
            .await?;

        let records = wait_for_records(&harness.queue, 1).await;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.priority, JobPriority::P0);
        assert_eq!(record.correlation_id, Some(correlation_id));
        assert_eq!(record.job.scan_reason, ScanReason::HotChange);
        assert_eq!(
            record.job.context.folder_path_norm(),
            normalize_path(&movie_dir)?.as_str()
        );

        let event = wait_for_enqueued_event(&mut job_rx).await;
        assert_eq!(event.meta.correlation_id, correlation_id);
        assert!(matches!(
            event.payload,
            JobEventPayload::Enqueued {
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
                ..
            }
        ));

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_batch_enqueues_deduped_folder_scan() -> Result<()>
    {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Movie B");
        std::fs::create_dir_all(&movie_dir).unwrap();
        let media_path = movie_dir.join("feature.mkv");
        std::fs::write(&media_path, b"movie").unwrap();

        let harness = runtime_harness(root.clone()).await?;
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            harness.runtime.clone();
        let service: FsWatchService = FsWatchService::new(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
        );
        service
            .register_library(
                harness.library_id,
                vec![(LibraryRootsId(0), root)],
            )
            .await?;

        let mut job_rx = harness.events.subscribe();
        // Native and polling watchers both enter the service through
        // WatchMessage::Event; inject that shared seam to keep the test
        // deterministic without relying on platform-specific notify timing.
        service
            .send_watch_message_for_test(
                harness.library_id,
                WatchMessage::Event(
                    Event::new(EventKind::Create(CreateKind::File))
                        .add_path(media_path.clone()),
                ),
            )
            .await?;
        service
            .send_watch_message_for_test(
                harness.library_id,
                WatchMessage::Event(
                    Event::new(EventKind::Modify(ModifyKind::Data(
                        DataChange::Content,
                    )))
                    .add_path(media_path),
                ),
            )
            .await?;

        let records = wait_for_records(&harness.queue, 1).await;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.priority, JobPriority::P0);
        assert_eq!(record.job.scan_reason, ScanReason::HotChange);
        assert_eq!(
            record.job.context.folder_path_norm(),
            normalize_path(&movie_dir)?.as_str()
        );

        let event = wait_for_enqueued_event(&mut job_rx).await;
        assert!(matches!(
            event.payload,
            JobEventPayload::Enqueued {
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
                ..
            }
        ));
        service.unregister_library(harness.library_id).await;

        Ok(())
    }

    #[tokio::test]
    async fn fs_watch_service_overflow_enqueues_p0_rescan_for_root_child()
    -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let movie_dir = root.join("Movie C");
        std::fs::create_dir_all(&movie_dir).unwrap();

        let harness = runtime_harness(root.clone()).await?;
        let command_executor: Arc<dyn LibraryCommandExecutor> =
            harness.runtime.clone();
        let service: FsWatchService = FsWatchService::new(
            FsWatchConfig {
                debounce_window: Duration::from_millis(25),
                max_batch_events: 16,
                strategy: WatchStrategy::Auto,
                poll_interval: Duration::from_secs(1),
                poll_backoff_max: Duration::from_secs(5 * 60),
            },
            Arc::new(NoopFsWatchObserver),
            command_executor,
        );
        service
            .register_library(
                harness.library_id,
                vec![(LibraryRootsId(0), root)],
            )
            .await?;

        let mut job_rx = harness.events.subscribe();
        // Watcher backend errors use the same service channel before being
        // converted into overflow rescans.
        service
            .send_watch_message_for_test(
                harness.library_id,
                WatchMessage::Error("overflow".into()),
            )
            .await?;

        let records = wait_for_records(&harness.queue, 1).await;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.priority, JobPriority::P0);
        assert_eq!(record.job.scan_reason, ScanReason::WatcherOverflow);
        assert_eq!(
            record.job.context.folder_path_norm(),
            normalize_path(&movie_dir)?.as_str()
        );

        let event = wait_for_enqueued_event(&mut job_rx).await;
        assert!(matches!(
            event.payload,
            JobEventPayload::Enqueued {
                kind: JobKind::FolderScan,
                priority: JobPriority::P0,
                ..
            }
        ));
        service.unregister_library(harness.library_id).await;

        Ok(())
    }
}
