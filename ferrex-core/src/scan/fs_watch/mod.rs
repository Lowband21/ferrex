//! Filesystem watch pipeline for library actors.
//!
//! A thin wrapper around `notify` that debounces raw filesystem notifications
//! into batches and forwards them as `LibraryActorCommand::FsEvents` messages.
//! Overflow conditions are surfaced explicitly so the library actor can fall
//! back to breadth-first rescans of the affected subtree.

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use notify::event::{EventKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, mpsc};
use tokio::task::{JoinHandle, spawn_blocking};
use tokio::time::{Duration, timeout};
use tracing::warn;

pub mod event_bus;
pub mod watcher;

// `watcher` currently holds the markdown design for the upcoming reimplementation that will unify
// realtime watchers, polling fallback, and the Postgres event bus. Once the design solidifies we
// will fold that module into this file and retire the legacy service below.

use crate::error::MediaError;
use crate::error::Result;
use crate::orchestration::FileSystemEvent;
use crate::orchestration::FileSystemEventKind;
use crate::orchestration::LibraryActorCommand;
use crate::orchestration::LibraryActorHandle;
use crate::orchestration::LibraryRootsId;
use crate::orchestration::config::WatchConfig;
use crate::orchestration::scan_cursor::normalize_path;
use crate::types::ids::LibraryID;
/// Version field stamped on emitted `FileSystemEvent`s.
pub const EVENT_VERSION: u16 = 1;

/// Configuration knobs for watch processing.
#[derive(Clone, Debug)]
pub struct FsWatchConfig {
    /// Debounce window for coalescing rapid event bursts per library root.
    pub debounce_window: Duration,
    /// Maximum number of filesystem events bundled into a single flush.
    pub max_batch_events: usize,
}

impl Default for FsWatchConfig {
    fn default() -> Self {
        Self {
            debounce_window: Duration::from_millis(250),
            max_batch_events: 1024,
        }
    }
}

impl From<WatchConfig> for FsWatchConfig {
    fn from(cfg: WatchConfig) -> Self {
        Self {
            debounce_window: Duration::from_millis(cfg.debounce_window_ms.max(1)),
            max_batch_events: cfg.max_batch_events.max(1),
        }
    }
}

/// Observer hook for surfacing watcher errors.
pub trait FsWatchObserver: Send + Sync {
    fn on_error(&self, library_id: LibraryID, error: &str);
}

/// No-op observer used when metrics instrumentation is not wired up.
pub struct NoopFsWatchObserver;

impl FsWatchObserver for NoopFsWatchObserver {
    fn on_error(&self, _library_id: LibraryID, _error: &str) {}
}

impl fmt::Debug for NoopFsWatchObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NoopFsWatchObserver")
    }
}

/// Dispatches debounced filesystem notifications to library actors.
pub struct FsWatchService<O: FsWatchObserver = NoopFsWatchObserver> {
    config: FsWatchConfig,
    observer: Arc<O>,
    libraries: Arc<RwLock<HashMap<LibraryID, LibraryWatch>>>,
}

impl<O: FsWatchObserver + 'static> fmt::Debug for FsWatchService<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("FsWatchService");
        debug
            .field("config", &self.config)
            .field("observer_type", &std::any::type_name::<O>());

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
    pub fn new(config: FsWatchConfig, observer: Arc<O>) -> Self {
        Self {
            config,
            observer,
            libraries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Attach notify watchers for the supplied library roots. Events are
    /// debounced and forwarded directly to the actor handle.
    pub async fn register_library(
        &self,
        library_id: LibraryID,
        roots: Vec<(LibraryRootsId, PathBuf)>,
        actor: LibraryActorHandle,
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
            actor,
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
            },
        );

        let libraries = Arc::clone(&self.libraries);
        let observer = Arc::clone(&self.observer);
        let watcher_roots = resolved_roots.clone();
        let watcher_tx = tx.clone();

        tokio::spawn(async move {
            let build_result =
                spawn_blocking(move || init_watchers(watcher_roots, watcher_tx)).await;

            match build_result {
                Ok(Ok(watchers)) => {
                    let mut guard = libraries.write().await;
                    if let Some(entry) = guard.get_mut(&library_id) {
                        entry.watchers = Some(watchers);
                    }
                }
                Ok(Err(err)) => {
                    let msg = err.to_string();
                    observer.on_error(library_id, &msg);
                    let mut guard = libraries.write().await;
                    if let Some(entry) = guard.remove(&library_id) {
                        entry.flush_task.abort();
                    }
                }
                Err(join_err) => {
                    let msg = format!("watcher initialization panicked: {join_err}");
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
    pub async fn unregister_library(&self, library_id: LibraryID) {
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
}

struct LibraryWatch {
    watchers: Option<Vec<RecommendedWatcher>>,
    flush_task: JoinHandle<()>,
}

impl LibraryWatch {
    fn shutdown(self) {
        self.flush_task.abort();
        // Dropping `watchers` stops notify streams.
    }
}

impl fmt::Debug for LibraryWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let watcher_count = self.watchers.as_ref().map(|watchers| watchers.len());
        f.debug_struct("LibraryWatch")
            .field("watcher_count", &watcher_count)
            .field("flush_task_finished", &self.flush_task.is_finished())
            .finish()
    }
}

enum WatchMessage {
    Event(Event),
    Error(String),
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
    library_id: LibraryID,
    roots: Vec<(LibraryRootsId, PathBuf)>,
    observer: Arc<O>,
    actor: LibraryActorHandle,
    mut rx: mpsc::Receiver<WatchMessage>,
    config: FsWatchConfig,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut pending: HashMap<LibraryRootsId, Vec<FileSystemEvent>> = HashMap::new();

        loop {
            let msg = if pending.is_empty() {
                rx.recv().await
            } else {
                match timeout(config.debounce_window, rx.recv()).await {
                    Ok(msg) => msg,
                    Err(_) => {
                        if let Err(err) =
                            flush_pending(Arc::clone(&observer), library_id, &mut pending, &actor)
                                .await
                        {
                            observer.on_error(library_id, &err.to_string());
                        }
                        continue;
                    }
                }
            };

            let Some(msg) = msg else {
                if let Err(err) =
                    flush_pending(Arc::clone(&observer), library_id, &mut pending, &actor).await
                {
                    observer.on_error(library_id, &err.to_string());
                }
                break;
            };

            match msg {
                WatchMessage::Event(event) => {
                    if let Some((root_id, fs_event)) = convert_event(library_id, &roots, event) {
                        if matches!(fs_event.kind, FileSystemEventKind::Overflow) {
                            if let Err(err) = dispatch_events(
                                Arc::clone(&observer),
                                library_id,
                                &actor,
                                root_id,
                                vec![fs_event],
                            )
                            .await
                            {
                                observer.on_error(library_id, &err.to_string());
                            }
                            continue;
                        }

                        let entry = pending.entry(root_id).or_default();
                        entry.push(fs_event);
                        if entry.len() >= config.max_batch_events {
                            let events = std::mem::take(entry);
                            if let Err(err) = dispatch_events(
                                Arc::clone(&observer),
                                library_id,
                                &actor,
                                root_id,
                                events,
                            )
                            .await
                            {
                                observer.on_error(library_id, &err.to_string());
                            }
                        }
                    }
                }
                WatchMessage::Error(error) => {
                    observer.on_error(library_id, &error);
                    let overflow_events = overflow_for_roots(library_id, &roots);
                    for (root_id, events) in overflow_events {
                        if let Err(err) = dispatch_events(
                            Arc::clone(&observer),
                            library_id,
                            &actor,
                            root_id,
                            events,
                        )
                        .await
                        {
                            observer.on_error(library_id, &err.to_string());
                        }
                    }
                }
            }
        }
    })
}

async fn flush_pending<O: FsWatchObserver + 'static>(
    observer: Arc<O>,
    library_id: LibraryID,
    pending: &mut HashMap<LibraryRootsId, Vec<FileSystemEvent>>,
    actor: &LibraryActorHandle,
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
        dispatch_events(Arc::clone(&observer), library_id, actor, root_id, events).await?;
    }
    pending.clear();
    Ok(())
}

async fn dispatch_events<O: FsWatchObserver + 'static>(
    observer: Arc<O>,
    library_id: LibraryID,
    actor: &LibraryActorHandle,
    root_id: LibraryRootsId,
    events: Vec<FileSystemEvent>,
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    let mut events = events;

    let correlation_hint = events
        .iter()
        .filter_map(|event| event.correlation_id)
        .next();

    if let Some(correlation) = correlation_hint {
        for event in events.iter_mut() {
            if event.correlation_id.is_none() {
                event.correlation_id = Some(correlation);
            }
        }
    }

    let mut guard = actor.lock().await;
    if let Err(err) = guard
        .handle_command(LibraryActorCommand::FsEvents {
            root: root_id,
            events,
            correlation_id: correlation_hint,
        })
        .await
    {
        observer.on_error(library_id, &err.to_string());
        Err(err)
    } else {
        Ok(())
    }
}

fn convert_event(
    library_id: LibraryID,
    roots: &[(LibraryRootsId, PathBuf)],
    event: Event,
) -> Option<(LibraryRootsId, FileSystemEvent)> {
    let (root_id, root_path) = locate_root(&event, roots)?;

    let (path, old_path) = extract_paths(&event, root_path)?;
    let kind = classify_event(&event.kind);

    let path_key = normalize_path(&path);
    let idempotency_key = encode_hash(&[
        "fs",
        &library_id.to_string(),
        &root_id.0.to_string(),
        &path_key,
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
        occurred_at: chrono::Utc::now(),
    };

    Some((root_id, event))
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

fn extract_paths(event: &Event, root_path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
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
        EventKind::Remove(RemoveKind::File | RemoveKind::Folder | RemoveKind::Any) => {
            FileSystemEventKind::Deleted
        }
        EventKind::Other => FileSystemEventKind::Overflow,
        _ => FileSystemEventKind::Modified,
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
    library_id: LibraryID,
    roots: &[(LibraryRootsId, PathBuf)],
) -> Vec<(LibraryRootsId, Vec<FileSystemEvent>)> {
    roots
        .iter()
        .map(|(root_id, root_path)| {
            let path_key = normalize_path(root_path);
            let idempotency_key = encode_hash(&[
                "fs-overflow",
                &library_id.to_string(),
                &root_id.0.to_string(),
                &path_key,
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
                occurred_at: chrono::Utc::now(),
            };

            (*root_id, vec![event])
        })
        .collect()
}

fn resolve_roots(roots: Vec<(LibraryRootsId, PathBuf)>) -> Vec<(LibraryRootsId, PathBuf)> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
    watcher_roots: Vec<(LibraryRootsId, PathBuf)>,
    watcher_tx: mpsc::Sender<WatchMessage>,
) -> Result<Vec<RecommendedWatcher>> {
    let mut watchers = Vec::with_capacity(watcher_roots.len());
    for (_root_id, root_path) in &watcher_roots {
        let path_clone = root_path.clone();
        let tx_event = watcher_tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| match res {
                Ok(event) => {
                    if let Err(err) = tx_event.blocking_send(WatchMessage::Event(event)) {
                        warn!(
                            "fs_watch channel send failed for {}: {}",
                            path_clone.display(),
                            err
                        );
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    let _ = tx_event.blocking_send(WatchMessage::Error(msg));
                }
            },
            NotifyConfig::default(),
        )
        .map_err(|err| {
            MediaError::Internal(format!(
                "failed to create watcher for {}: {}",
                root_path.display(),
                err
            ))
        })?;

        if let Err(err) = watcher.watch(root_path, RecursiveMode::Recursive) {
            return Err(MediaError::Internal(format!(
                "failed to watch {}: {}",
                root_path.display(),
                err
            )));
        }

        watchers.push(watcher);
    }

    Ok(watchers)
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
    use std::sync::Arc;

    use crate::error::Result;
    use crate::fs_watch::{FsWatchConfig, FsWatchService, NoopFsWatchObserver};
    use crate::orchestration::{
        LibraryActor, LibraryActorCommand, LibraryActorConfig, LibraryActorEvent,
        LibraryActorHandle, LibraryActorState, LibraryRootsId,
    };
    use crate::types::ids::LibraryID;

    use tempfile::tempdir;
    use tokio::sync::Mutex;

    struct DummyActor;

    #[async_trait::async_trait]
    impl LibraryActor for DummyActor {
        fn config(&self) -> &LibraryActorConfig {
            panic!("not used")
        }

        fn state(&self) -> &LibraryActorState {
            panic!("not used")
        }

        fn state_mut(&mut self) -> &mut LibraryActorState {
            panic!("not used")
        }

        async fn handle_command(
            &mut self,
            _command: LibraryActorCommand,
        ) -> Result<Vec<LibraryActorEvent>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn registers_and_unregisters_library() -> Result<()> {
        let tmp = tempdir().unwrap();
        let root = tmp.path().to_path_buf();

        let service: FsWatchService =
            FsWatchService::new(FsWatchConfig::default(), Arc::new(NoopFsWatchObserver));

        let actor: LibraryActorHandle = Arc::new(Mutex::new(Box::new(DummyActor)));
        let library_id = LibraryID::new();
        service
            .register_library(
                library_id,
                vec![(LibraryRootsId(0), root)],
                Arc::clone(&actor),
            )
            .await?;
        assert_eq!(service.watcher_count().await, 1);
        service.unregister_library(library_id).await;
        assert_eq!(service.watcher_count().await, 0);
        Ok(())
    }
}
