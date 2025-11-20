//! # File Watcher Rewrite (Comment-First Design)
//!
//! This module intentionally contains no Rust statements yet. Every former code path is explained
//! below so we can reason about behaviour without committing to types or APIs prematurely. Once
//! the design feels correct we will translate these sections into concrete structs/functions.
//!
//! ---
//!
//! ## Public Surface (`FileWatcher` facade)
//!
//! ```text
//! struct FileWatcher {
//!   // shared state for all libraries we're watching
//! }
//!
//! impl FileWatcher {
//!   fn new(bus: Arc<dyn FileChangeEventBus>, actor_mailbox: ActorMailboxHandle, config: WatcherConfig)
//!   fn register_library(&self, library: LibraryReference)
//!   fn unregister_library(&self, library_id: LibraryID)
//!   fn shutdown(&self)
//! }
//! ```
//!
//! - `FileWatcher::new` accepts the durable Postgres event bus handle, an indirect handle for
//!   sending commands to library actors, and configuration (debounce window, polling cadence,
//!   overflow thresholds).
//! - `register_library` is idempotent: if the library is already registered we no-op; otherwise we:
//!   1. Resolve each root path to an absolute path.
//!   2. Decide per root whether native notifications or polling should be used (see detection below).
//!   3. Spawn the appropriate watcher task and stash handles so we can tear them down later.
//!   4. Start a shared flush task that collects raw events, persists them to Postgres, and forwards
//!      batches to the actor.
//! - `unregister_library` aborts active watchers and flush tasks, ensuring no outstanding events are
//!   left unpersisted.
//! - `shutdown` iterates all libraries and calls `unregister_library`.
//!
//! ### Configuration (`WatcherConfig`)
//! - `debounce_window_ms`: how long to wait before flushing a burst to the actor.
//! - `max_batch_events`: soft cap on events per flush; beyond this we cut the batch and continue.
//! - `poll_interval_ms`: fallback cadence for polling watchers.
//! - `overflow_batch_capacity`: size of the channel; hitting this triggers an overflow event.
//! - `ignored_extensions`: optional allow/deny list to filter noise (images, subtitles, temp files).
//! - `maintenance_tick_interval_ms`: how often to examine partitions for sweep scheduling.
//!
//! ---
//!
//! ## Library Registration Flow (replacement for legacy `watch_library`)
//!
//! 1. Acquire a write lock on the watcher registry to prevent double registration.
//! 2. For each root path:
//!    - Normalize path (resolve relative components, symlinks if possible).
//!    - Call `detect_strategy`:
//!      ```text
//!      fn detect_strategy(path) -> WatchStrategy {
//!          if notify::recommended_watcher_supported_for(path) -> WatchStrategy::Debounced
//!          else -> WatchStrategy::Polling(poll_interval_ms)
//!      }
//!      ```
//!    - Build a `WatcherTask` descriptor with:
//!      - strategy
//!      - root id (LibraryRootsId)
//!      - path
//!      - optional metadata (network FS flag, etc.)
//! 3. Create an mpsc channel per library (`raw_tx`, `raw_rx`) sized by `overflow_batch_capacity`.
//! 4. Spawn watcher tasks:
//!    - Debounced watcher: `notify::RecommendedWatcher` with closure that sends raw events into
//!      `raw_tx`.
//!    - Poller: maintain a `HashMap<PathBuf, FileFingerprint>` per root and periodically diff.
//! 5. Spawn the flush loop (see next section). Store join handles + watcher guards in the registry.
//!
//! ### Error Handling
//! - If initialization fails for any root, log the error, notify the observer, emit a synthetic
//!   overflow event for the library, and continue with the remaining roots (partial watching is
//!   permitted).
//! - If all roots fail we tear down the registration attempt and surface the error to caller.
//!
//! ---
//!
//! ## Flush Loop (replacement for the legacy `spawn_watch_loop` + `flush_pending`)
//!
//! Pseudocode outline:
//! ```text
//! async fn flush_loop(library_id, roots, raw_rx, bus, actor_mailbox, config, correlation_tracker) {
//!     pending: HashMap<LibraryRootsId, Vec<DurableEvent>>
//!     loop:
//!         if pending empty:
//!             msg = raw_rx.recv().await
//!         else:
//!             msg = timeout(debounce_window, raw_rx.recv())
//!
//!         match msg:
//!             Some(RawEvent::Fs(event)) => {
//!                 let durable = normalize_and_classify(event)
//!                 match durable.kind:
//!                     Overflow => push immediate overflow batch (see below)
//!                     _ => push onto pending[root_id], trimming by max_batch_events
//!             }
//!             Some(RawEvent::PollSnapshot(diff)) => same as Fs
//!             Some(RawEvent::WatcherError(err)) => emit overflow for every root
//!             None => flush_remaining(); break
//!             Timeout => flush_pending()
//!     }
//! }
//! ```
//!
//! - `normalize_and_classify` mirrors the legacy `convert_event` logic: map to absolute path, drop
//!   ignored files, detect moves, compute idempotency key (`encode_hash` equivalent), attach the
//!   library’s active correlation id (if any).
//! - Before forwarding to the actor we must persist every `DurableEvent` via
//!   `FileChangeEventBus::publish` so crash recovery can replay.
//! - After persistence we call `actor_mailbox.send(FsEvents { ... })` with the batch.
//! - Overflow handling: if the channel is full or watcher reports overflow we generate a synthetic
//!   `DurableEvent` with kind `Overflow`, path set to the root, and push it immediately (no debounce)
//!   so the actor can schedule a rescan.
//! - The flush loop also records metrics (events per second, lag, overflow count) for observability.
//!
//! ---
//!
//! ## Durable Event Representation (`DurableEvent`)
//! ```text
//! struct DurableEvent {
//!     version: u16,
//!     library_id: LibraryID,
//!     root_id: LibraryRootsId,
//!     kind: FileSystemEventKind,
//!     path: PathBuf,
//!     old_path: Option<PathBuf>,
//!     file_size: Option<i64>,
//!     detected_at: DateTime<Utc>,
//!     correlation_id: Option<Uuid>,
//!     idempotency_key: String (deterministic hash),
//! }
//! ```
//! - Mirrors the actor-facing `FileSystemEvent` for compatibility.
//! - `correlation_id` is sourced from:
//!   - event metadata if watcher supplied one (e.g., from bulk seed job), else
//!   - library actor’s current correlation (maintenance run), else
//!   - newly generated UUID (so downstream jobs always have a correlation).
//!
//! ---
//!
//! ## Actor Dispatch Flow (replacement for the legacy `dispatch_events` helper)
//!
//! ```text
//! async fn dispatch_batch(actor_mailbox, library_id, root_id, events: Vec<FileSystemEvent>) {
//!     if events.is_empty() return
//!     actor_mailbox.send(LibraryActorCommand::FsEvents {
//!         root: root_id,
//!         events,
//!         correlation_id: events.first().correlation_id,
//!     }).await
//! }
//! ```
//! - The mailbox handle abstracts over whether the actor lives in-process or behind a channel.
//! - Errors from the actor (e.g., paused state) are logged and expose metrics; we keep the events in
//!   Postgres for replay so no data is lost.
//! - If dispatch fails repeatedly we stop the watcher and transition the library into a “needs
//!   maintenance sweep” state.
//!
//! ---
//!
//! ## Maintenance Sweep Scheduler
//!
//! - Separate async task owned by `FileWatcher`:
//!   ```text
//!   loop every maintenance_tick_interval_ms:
//!       for library in libraries:
//!           let partitions = library_actor.snapshot_partitions()
//!           let due = filter partitions whose `last_scan_at` older than threshold
//!           for each due partition:
//!               enqueue MaintenanceSweep job via actor_mailbox (ScanReason::MaintenanceSweep)
//!               persist synthetic event to Postgres for audit
//!   ```
//! - When the watcher detects sustained overflow or the flush loop notices gaps in persisted events,
//!   it can mark partitions as “stale” so the next tick prioritises them.
//! - This scheduler also replays missed events by scanning `file_watch_events` for records newer
//!   than the last processed cursor and feeding them back through `dispatch_batch`.
//!
//! ---
//!
//! ## Polling Implementation Sketch
//!
//! - Maintain `(path -> metadata)` map per root, where metadata includes modified timestamp and
//!   size.
//! - On each tick:
//!   - List directory recursively up to a reasonable depth (configurable) or maintain a queue of
//!     paths to scan to avoid re-walking everything every time.
//!   - Compare to previous snapshot to detect creates, modifies, deletes, moves (by inode/size).
//!   - Emit `RawEvent::PollSnapshot` entries just like native watcher events.
//! - Backoff logic: if the system load is high or we detect large numbers of changes, slow the
//!   polling interval to avoid thrashing.
//! - Poll watchers co-exist with realtime watchers: per-root strategy selection allows a mix.
//!
//! ---
//!
//! ## Observer & Telemetry Hooks
//!
//! - Provide callbacks for:
//!   - watcher initialization success/failure
//!   - event persistence errors
//!   - dispatch failures / retries
//!   - overflow occurrences
//! - Emit structured logs with context (`library_id`, `root_path`, `reason`).
//! - Integrate with metrics crate (once available) to increment counters and gauges.
//!
//! ---
//!
//! ## Next Steps Before Coding
//!
//! 1. Validate watcher strategy detection heuristics (network mounts, containerised paths).
//! 2. Decide on replay ordering guarantees when mixing live events and maintenance sweeps.
//! 3. Define the actor mailbox abstraction so `FileWatcher` doesn’t depend on concrete runtime
//!    types.
//! 4. Codify the persistence schema expectations (ensure SQLx metadata matches the payload shape).
//! 5. Draft tests mirroring existing behaviour: registration idempotence, overflow, hot-change batch
//!    coalescing, polling fallback, maintenance sweep triggers.
