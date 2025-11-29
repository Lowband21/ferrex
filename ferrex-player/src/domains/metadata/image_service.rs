use dashmap::DashMap;
use ferrex_core::player_prelude::ImageRequest;
use iced::widget::image::Handle;
use priority_queue::PriorityQueue;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

// Maximum number of retry attempts for failed images
const MAX_RETRY_ATTEMPTS: u8 = 5;

#[derive(Debug, Clone, Copy)]
pub enum FirstDisplayHint {
    FlipOnce,
    FastThenSlow,
}

#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Handle),
    Failed(String),
}

#[derive(Debug)]
pub struct ImageEntry {
    pub state: LoadState,
    pub last_accessed: Instant,
    pub loaded_at: Option<Instant>,
    pub requested_at: Option<Instant>,
    pub loading_started_at: Option<Instant>,
    pub first_displayed_at: Option<Instant>,
    pub retry_count: u8,
    pub last_failure: Option<Instant>,
    pub first_display_hint: Option<FirstDisplayHint>,
}

#[derive(Debug, Clone)]
pub struct UnifiedImageService {
    // Single cache for all images
    cache: Arc<DashMap<ImageRequest, ImageEntry>>,

    // Priority queue for pending loads (using u8 priority, higher is better)
    queue: Arc<Mutex<PriorityQueue<ImageRequest, u8>>>,

    // Currently loading requests
    loading: Arc<DashMap<ImageRequest, std::time::Instant>>,

    // Channel for wake-up signals to notify loader of new requests
    load_sender: mpsc::UnboundedSender<()>,

    // Maximum concurrent loads
    max_concurrent: Arc<AtomicUsize>,

    // Cancellation handles for in-flight downloads
    inflight_cancellers: Arc<DashMap<ImageRequest, oneshot::Sender<()>>>,

    // Rolling telemetry for latency and depth tracking
    telemetry: Arc<Telemetry>,

    // Counter for proportional retry scheduling
    // Every Nth request allows a retry slot to prevent starvation
    retry_slot_counter: Arc<AtomicUsize>,
}

// Minimum delay between retry attempts for transient (e.g., 404) failures
const RETRY_THROTTLE: std::time::Duration =
    std::time::Duration::from_millis(750);

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl UnifiedImageService {
    pub fn new(max_concurrent: usize) -> (Self, mpsc::UnboundedReceiver<()>) {
        let (load_sender, load_receiver) = mpsc::unbounded_channel();

        let service = Self {
            cache: Arc::new(DashMap::new()),
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            loading: Arc::new(DashMap::new()),
            load_sender,
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent)),
            inflight_cancellers: Arc::new(DashMap::new()),
            telemetry: Arc::new(Telemetry::default()),
            retry_slot_counter: Arc::new(AtomicUsize::new(0)),
        };

        (service, load_receiver)
    }

    /// Maximum allowed concurrent loads for the unified image service.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    pub fn get(&self, request: &ImageRequest) -> Option<Handle> {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("ImageService::get");

        self.cache
            .get(request)
            .and_then(|entry| match &entry.state {
                LoadState::Loaded(handle) => Some(handle.clone()),
                _ => None,
            })
    }

    /// Get image with load time for animation decisions
    /// Returns (Handle, Option<load_time>) where load_time is when the image was loaded from server
    pub fn get_with_load_time(
        &self,
        request: &ImageRequest,
    ) -> Option<(Handle, Option<std::time::Instant>)> {
        self.cache
            .get(request)
            .and_then(|entry| match &entry.state {
                LoadState::Loaded(handle) => {
                    Some((handle.clone(), entry.loaded_at))
                }
                _ => None,
            })
    }

    /// Get image with load time and consume any first-display hint.
    pub fn take_loaded_entry(
        &self,
        request: &ImageRequest,
    ) -> Option<(Handle, Option<std::time::Instant>, Option<FirstDisplayHint>)>
    {
        let mut entry = self.cache.get_mut(request)?;
        let handle = match &entry.state {
            LoadState::Loaded(handle) => handle.clone(),
            _ => return None,
        };

        let now = Instant::now();
        entry.last_accessed = now;

        // If this is the first time the UI consumes this texture,
        // record it and provide a one-time flip by default (unless set explicitly).
        let mut newly_displayed_duration = None;
        let default_first_hint = if entry.first_displayed_at.is_none() {
            entry.first_displayed_at = Some(now);
            if let Some(loaded_at) = entry.loaded_at {
                newly_displayed_duration =
                    Some(now.saturating_duration_since(loaded_at));
            }
            Some(FirstDisplayHint::FlipOnce)
        } else {
            None
        };
        let loaded_at = entry.loaded_at;
        let hint = entry.first_display_hint.take().or(default_first_hint);
        drop(entry);

        if let Some(duration) = newly_displayed_duration {
            self.telemetry.record_display_latency(duration);
        }

        Some((handle, loaded_at, hint))
    }

    pub fn request_image(&self, request: ImageRequest) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("ImageService::request_image");

        //log::info!("Requesting image with request: {:#?}", request);
        // Check if already cached
        if let Some(mut entry) = self.cache.get_mut(&request) {
            let now = std::time::Instant::now();

            // Don't retry if already loaded
            if matches!(entry.state, LoadState::Loaded(_)) {
                entry.last_accessed = now;
                return;
            }

            let is_failed = matches!(entry.state, LoadState::Failed(_));
            let is_failed_404 = matches!(entry.state, LoadState::Failed(ref err) if err.contains("404"));

            // For non-404 errors respect the max retry limit
            if is_failed
                && !is_failed_404
                && entry.retry_count >= MAX_RETRY_ATTEMPTS
            {
                entry.last_accessed = now;
                log::debug!(
                    "Skipping image request for {:?} - exceeded max retries ({}/{})",
                    request.media_id,
                    entry.retry_count,
                    MAX_RETRY_ATTEMPTS
                );
                return;
            }

            // Throttle repeated retries
            // - For 404s: exponential backoff (750ms, 1500ms, 3000ms, 6000ms, 8000ms max)
            // - For other failures: keep legacy throttle
            if is_failed && let Some(last_failure) = entry.last_failure {
                let throttle = if is_failed_404 {
                    // retry_count reflects the count before this request; cap growth
                    let exp = entry.retry_count.min(4) as u32; // 2^0..2^4
                    let base_ms: u64 = 750;
                    let backoff_ms = base_ms.saturating_mul(1u64 << exp);
                    std::cmp::min(backoff_ms, 8_000)
                } else {
                    RETRY_THROTTLE.as_millis() as u64
                };
                if now.duration_since(last_failure)
                    < std::time::Duration::from_millis(throttle)
                {
                    entry.last_accessed = now;
                    return;
                }
            }

            // Record first time we saw a request for this image
            if entry.requested_at.is_none() {
                entry.requested_at = Some(now);
            }
            entry.last_accessed = now;
        }

        // Check if already loading
        if self.loading.contains_key(&request) {
            return;
        }

        // Add to queue or adjust priority
        if let Ok(mut queue) = self.queue.lock() {
            // If this image previously failed, demote its priority to the back
            // of the queue to avoid starving fresh images. Use an even lower
            // priority than Background where possible.
            let is_failed_entry = self
                .cache
                .get(&request)
                .map(|entry| matches!(entry.state, LoadState::Failed(_)))
                .unwrap_or(false);

            // Normal priorities are 1..=3; use 0 for failed retries to push
            // them behind everything else.
            let requested_priority = request.priority.weight();
            let effective_priority: u8 = if is_failed_entry {
                0
            } else {
                requested_priority
            };

            if let Some(&existing_priority) = queue.get_priority(&request) {
                // If we have a failed entry, force a demotion to the lowest
                // effective priority so it doesn't jump the line. Otherwise
                // only upgrade if the caller requested a higher priority.
                if is_failed_entry {
                    if existing_priority != effective_priority {
                        queue.change_priority(&request, effective_priority);
                        match self.load_sender.send(()) {
                            Ok(_) => log::debug!(
                                "Sent wake-up signal for failed-request demotion"
                            ),
                            Err(e) => log::error!(
                                "Failed to send wake-up signal: {:?}",
                                e
                            ),
                        }
                    }
                } else if requested_priority > existing_priority {
                    /*
                    log::debug!("Upgrading priority for {:?} from {} to {} ({})",
                               request.media_id, existing_priority, requested_priority,
                               if requested_priority == 3 { "VISIBLE" } else if requested_priority == 2 { "PRELOAD" } else { "BACKGROUND" });
                     */
                    queue.change_priority(&request, requested_priority);
                    // Send wake-up signal to notify loader of priority change
                    match self.load_sender.send(()) {
                        Ok(_) => log::debug!(
                            "Sent wake-up signal for priority upgrade"
                        ),
                        Err(e) => log::error!(
                            "Failed to send wake-up signal: {:?}",
                            e
                        ),
                    }
                }
            } else {
                // New request - add to queue
                queue.push(request.clone(), effective_priority);
                // Send wake-up signal to notify loader of new request
                match self.load_sender.send(()) {
                    Ok(_) => log::debug!(
                        "Sent wake-up signal for new request: {:?} (priority {})",
                        request,
                        effective_priority
                    ),
                    Err(e) => {
                        log::error!("Failed to send wake-up signal: {:?}", e)
                    }
                }
            }
        }
    }

    pub fn mark_loading(&self, request: &ImageRequest) {
        let now = std::time::Instant::now();
        self.loading.insert(request.clone(), now);
        let (
            existing_hint,
            existing_retry_count,
            existing_last_failure,
            existing_requested_at,
            existing_first_displayed,
        ) = self
            .cache
            .get(request)
            .map(|entry| {
                (
                    entry.first_display_hint,
                    entry.retry_count,
                    entry.last_failure,
                    entry.requested_at,
                    entry.first_displayed_at,
                )
            })
            .unwrap_or((None, 0, None, Some(now), None));
        self.cache.insert(
            request.clone(),
            ImageEntry {
                state: LoadState::Loading,
                last_accessed: now,
                loaded_at: None,
                requested_at: existing_requested_at.or(Some(now)),
                loading_started_at: Some(now),
                first_displayed_at: existing_first_displayed,
                retry_count: existing_retry_count,
                last_failure: existing_last_failure,
                first_display_hint: existing_hint,
            },
        );
    }

    pub fn mark_loaded(&self, request: &ImageRequest, handle: Handle) {
        self.loading.remove(request);
        let now = std::time::Instant::now();

        let (
            existing_hint,
            prev_requested,
            prev_loading_started,
            prev_first_displayed,
        ) = {
            if let Some(entry) = self.cache.get(request) {
                (
                    entry.first_display_hint,
                    entry.requested_at,
                    entry.loading_started_at,
                    entry.first_displayed_at,
                )
            } else {
                (None, None, None, None)
            }
        };

        //log::debug!("mark_loaded called for {:?}", request.media_id);
        //log::debug!("  - Setting loaded_at to: {:?}", now);

        self.cache.insert(
            request.clone(),
            ImageEntry {
                state: LoadState::Loaded(handle),
                last_accessed: now,
                loaded_at: Some(now),
                requested_at: prev_requested,
                loading_started_at: prev_loading_started,
                first_displayed_at: prev_first_displayed,
                retry_count: 0,
                last_failure: None,
                first_display_hint: existing_hint,
            },
        );
        self.clear_inflight_cancel(request);

        if let Some(started) = prev_loading_started {
            self.telemetry
                .record_load_duration(now.saturating_duration_since(started));
        }
    }

    pub fn mark_failed(&self, request: &ImageRequest, error: String) {
        self.loading.remove(request);

        // Check if this is a 404 error (image doesn't exist on server)
        let is_404 = error.contains("404");

        let now = std::time::Instant::now();
        let retry_count = match self.cache.get_mut(request) {
            Some(mut entry) => {
                entry.state = LoadState::Failed(error.clone());
                entry.retry_count = entry.retry_count.saturating_add(1);
                entry.last_failure = Some(now);
                entry.retry_count
            }
            _ => {
                let retry_count = 1;
                self.cache.insert(
                    request.clone(),
                    ImageEntry {
                        state: LoadState::Failed(error.clone()),
                        last_accessed: now,
                        loaded_at: None,
                        requested_at: Some(now),
                        loading_started_at: Some(now),
                        first_displayed_at: None,
                        retry_count,
                        last_failure: Some(now),
                        first_display_hint: None,
                    },
                );
                retry_count
            }
        };

        // Log permanent failures for metadata aggregation
        if retry_count >= MAX_RETRY_ATTEMPTS && !is_404 {
            log::warn!(
                "Image permanently failed after {} attempts: {:?} - {}{}",
                retry_count,
                request.media_id,
                error,
                if is_404 { " [404]" } else { "" }
            );
            // TODO: Could aggregate these failures for missing metadata reporting
        } else if is_404 {
            log::debug!(
                "Image temporarily unavailable ({} attempts so far): {:?}",
                retry_count,
                request.media_id
            );
        } else {
            log::debug!(
                "Image failed (attempt {}/{}): {:?} - {}{}",
                retry_count,
                MAX_RETRY_ATTEMPTS,
                request.media_id,
                error,
                if is_404 { " [404]" } else { "" }
            );
        }
        self.clear_inflight_cancel(request);
    }

    pub fn flag_first_display_hint(
        &self,
        request: &ImageRequest,
        hint: FirstDisplayHint,
    ) {
        if let Some(mut entry) = self.cache.get_mut(request) {
            entry.first_display_hint = Some(hint);
            return;
        }

        self.cache.insert(
            request.clone(),
            ImageEntry {
                state: LoadState::Loading,
                last_accessed: Instant::now(),
                loaded_at: None,
                requested_at: Some(Instant::now()),
                loading_started_at: Some(Instant::now()),
                first_displayed_at: None,
                retry_count: 0,
                last_failure: None,
                first_display_hint: Some(hint),
            },
        );
    }

    pub fn mark_cancelled(&self, request: &ImageRequest) {
        self.loading.remove(request);
        self.cache.remove(request);
        self.clear_inflight_cancel(request);
    }

    pub fn get_next_request(&self) -> Option<ImageRequest> {
        let mut queue = self.queue.lock().ok()?;

        // Enforce concurrency cap strictly
        if self.loading.len() >= self.max_concurrent.load(Ordering::SeqCst) {
            return None;
        }

        // Optimized strategy using iter() instead of pop-all-reinsert:
        // 1) Fast path: check if top item is fresh and not loading (O(1))
        // 2) Slow path: scan with iter() to find candidates (O(n) scan, O(log n) remove)
        // 3) Proportional scheduling: allow 20% of slots for retries

        // Fast path: check if the highest-priority item is immediately usable
        if let Some((top_request, _prio)) = queue.peek() {
            if !self.loading.contains_key(top_request) {
                let retry_count = self
                    .cache
                    .get(top_request)
                    .map(|e| e.retry_count)
                    .unwrap_or(0);
                if retry_count == 0 {
                    // Top item is fresh and not loading - use it directly
                    return queue.pop().map(|(req, _)| req);
                }
            }
        }

        // Slow path: scan for candidates without removing items
        let mut fresh_candidate: Option<ImageRequest> = None;
        let mut retry_candidate: Option<ImageRequest> = None;

        for (request, _prio) in queue.iter() {
            let is_loading = self.loading.contains_key(request);
            if is_loading {
                continue;
            }

            let retry_count =
                self.cache.get(request).map(|e| e.retry_count).unwrap_or(0);
            let is_fresh = retry_count == 0;

            if is_fresh && fresh_candidate.is_none() {
                fresh_candidate = Some(request.clone());
            } else if !is_fresh && retry_candidate.is_none() {
                retry_candidate = Some(request.clone());
            }

            // Early exit if we have both candidates
            if fresh_candidate.is_some() && retry_candidate.is_some() {
                break;
            }
        }

        // Proportional scheduling: 20% of slots go to retries
        // This prevents retry starvation during continuous scrolling
        let use_retry_slot = self.should_use_retry_slot();

        // If we have both candidates and this is a retry slot, prefer retry
        if use_retry_slot {
            if let Some(ref retry) = retry_candidate {
                queue.remove(retry);
                return Some(retry.clone());
            }
        }

        // Otherwise, prefer fresh candidate
        if let Some(ref fresh) = fresh_candidate {
            queue.remove(fresh);
            return Some(fresh.clone());
        }

        // Fall back to retry if no fresh available
        if let Some(ref retry) = retry_candidate {
            queue.remove(retry);
            return Some(retry.clone());
        }

        None
    }

    /// Determines if the current request slot should be used for a retry.
    /// Returns true for ~20% of calls (every 5th request).
    fn should_use_retry_slot(&self) -> bool {
        let counter = self.retry_slot_counter.fetch_add(1, Ordering::Relaxed);
        (counter % 5) == 0
    }

    pub fn cleanup_stale_entries(&self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut to_remove = Vec::new();

        for entry in self.cache.iter() {
            if now.duration_since(entry.last_accessed) > max_age
                && (matches!(entry.state, LoadState::Failed(_))
                    || (matches!(entry.state, LoadState::Loading)
                        && self.loading.get(entry.key()).is_none_or(|start| {
                            now.duration_since(*start)
                                > std::time::Duration::from_secs(30)
                        })))
            {
                to_remove.push(entry.key().clone());
            }
        }

        for key in to_remove {
            self.cache.remove(&key);
            self.loading.remove(&key);
        }
    }

    /// Returns the current number of queued requests (not counting those already loading).
    pub fn queue_len(&self) -> usize {
        if let Ok(queue) = self.queue.lock() {
            queue.len()
        } else {
            0
        }
    }

    // === PR1 support APIs for Planner (snapshots and helpers) ===

    /// Immutable snapshot of the service state for planning purposes.
    pub fn snapshot_state(&self) -> ServiceStateSnapshot {
        // Loaded set: entries in cache with Loaded state
        let mut loaded = std::collections::HashSet::new();
        for entry in self.cache.iter() {
            if matches!(entry.state, LoadState::Loaded(_)) {
                loaded.insert(entry.key().clone());
            }
        }

        // Loading set
        let mut loading = std::collections::HashSet::new();
        for entry in self.loading.iter() {
            loading.insert(entry.key().clone());
        }

        // Queued set (best-effort)
        let mut queued = std::collections::HashSet::new();
        if let Ok(queue) = self.queue.lock() {
            // Iterate items; clone keys
            for (k, _p) in queue.iter() {
                queued.insert(k.clone());
            }
        }

        let in_flight = self.loading.len();
        let queue_depth = self.queue_len();
        self.telemetry.record_state(in_flight, queue_depth);

        ServiceStateSnapshot {
            loaded,
            loading,
            queued,
            in_flight,
            queue_depth,
        }
    }

    /// Helper: check if a request is already loaded.
    pub fn is_loaded(&self, request: &ImageRequest) -> bool {
        self.cache
            .get(request)
            .map(|e| matches!(e.state, LoadState::Loaded(_)))
            .unwrap_or(false)
    }

    /// Helper: check if a request is currently loading.
    pub fn is_loading(&self, request: &ImageRequest) -> bool {
        self.loading.contains_key(request)
    }

    /// Helper: get the timestamp when a request entered the loading state.
    pub fn loading_started_at(
        &self,
        request: &ImageRequest,
    ) -> Option<Instant> {
        self.loading.get(request).map(|entry| *entry.value())
    }

    /// Helper: check if a request is queued.
    pub fn is_queued(&self, request: &ImageRequest) -> bool {
        if let Ok(queue) = self.queue.lock() {
            queue.get_priority(request).is_some()
        } else {
            false
        }
    }

    /// Adjust maximum concurrent loads.
    pub fn set_max_concurrent(&self, n: usize) {
        self.max_concurrent.store(n.max(1), Ordering::SeqCst);
    }

    /// Outer bounds
    const MIN_CONCURRENT: usize = 2;
    const MAX_CONCURRENT: usize = 32;

    /// Adjusts concurrency based on observed load latency.
    /// Uses a target-relative approach: compares current latency to a learned baseline.
    /// Includes hysteresis to prevent oscillation and faster convergence when far from target.
    pub fn adapt_concurrency(&self) {
        let Some(median_ms) = self.telemetry.median_load_ms() else {
            return; // Not enough samples yet
        };

        log::info!("median_ms: {}", median_ms);

        let current = self.max_concurrent.load(Ordering::Relaxed);
        let queue_depth = self.queue_len();

        // Safety: if queue is backing up significantly, reduce concurrency regardless of latency
        // This catches cases where high concurrency causes system-wide slowdown
        if queue_depth > 50 && current > Self::MIN_CONCURRENT {
            let reduced = (current / 2).max(Self::MIN_CONCURRENT);
            log::info!(
                "Adaptive concurrency: {} → {} (queue backup: {} items)",
                current,
                reduced,
                queue_depth
            );
            self.max_concurrent.store(reduced, Ordering::SeqCst);
            return;
        }

        // Target latency: we want sub-200ms loads for good UX
        // - Below 80ms: definitely have headroom, can increase
        // - Above 400ms: definitely saturated, should decrease
        // - 120-400ms: stable zone with hysteresis
        const TARGET_LOW_MS: u64 = 80;
        const TARGET_HIGH_MS: u64 = 400;

        // Calculate adjustment magnitude based on how far we are from target
        // Faster convergence when clearly outside the stable zone
        let new_concurrent = if median_ms < TARGET_LOW_MS {
            // Well below target - increase, faster if very low latency
            let step = if median_ms < TARGET_LOW_MS / 2 { 2 } else { 1 };
            (current + step).min(Self::MAX_CONCURRENT)
        } else if median_ms > TARGET_HIGH_MS {
            // Well above target - decrease, faster if very high latency
            let step = if median_ms > TARGET_HIGH_MS * 2 { 2 } else { 1 };
            current.saturating_sub(step).max(Self::MIN_CONCURRENT)
        } else {
            // In stable zone - no change (hysteresis)
            current
        };

        if new_concurrent != current {
            log::debug!(
                "Adaptive concurrency: {} → {} (median latency {}ms, queue {})",
                current,
                new_concurrent,
                median_ms,
                queue_depth
            );
            self.max_concurrent.store(new_concurrent, Ordering::SeqCst);
        }
    }

    /// Register a cancellation sender for an in-flight request.
    pub fn register_inflight_cancel(
        &self,
        request: &ImageRequest,
        sender: oneshot::Sender<()>,
    ) {
        self.inflight_cancellers.insert(request.clone(), sender);
    }

    /// Clear a cancellation handle once the in-flight operation completes.
    pub fn clear_inflight_cancel(&self, request: &ImageRequest) {
        self.inflight_cancellers.remove(request);
    }

    /// Request cancellation of an in-flight download; returns true if a signal was sent.
    pub fn cancel_inflight(&self, request: &ImageRequest) -> bool {
        if let Some((_, sender)) = self.inflight_cancellers.remove(request) {
            let _ = sender.send(());
            true
        } else {
            false
        }
    }

    /// Remove a request from the pending queue.
    pub fn remove_from_queue(&self, request: &ImageRequest) -> bool {
        if let Ok(mut queue) = self.queue.lock() {
            queue.remove(request).is_some()
        } else {
            false
        }
    }
}

/// Read-only snapshot of the unified image service state for planning/diffing.
#[derive(Debug, Clone)]
pub struct ServiceStateSnapshot {
    pub loaded: std::collections::HashSet<ImageRequest>,
    pub loading: std::collections::HashSet<ImageRequest>,
    pub queued: std::collections::HashSet<ImageRequest>,
    pub in_flight: usize,
    pub queue_depth: usize,
}

#[derive(Debug, Default)]
struct Telemetry {
    load_ms: Mutex<VecDeque<u64>>,
    display_ms: Mutex<VecDeque<u64>>,
    max_queue_depth: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl Telemetry {
    const WINDOW: usize = 128;

    fn push(data: &mut VecDeque<u64>, value: u64) {
        data.push_back(value);
        if data.len() > Self::WINDOW {
            data.pop_front();
        }
    }

    fn record_load_duration(&self, duration: std::time::Duration) {
        let value = duration.as_millis() as u64;
        if let Ok(mut data) = self.load_ms.lock() {
            Self::push(&mut data, value);
        }
    }

    fn record_display_latency(&self, duration: std::time::Duration) {
        let value = duration.as_millis() as u64;
        if let Ok(mut data) = self.display_ms.lock() {
            Self::push(&mut data, value);
            Self::maybe_warn("loaded→first_display", &data, 250, 600);
        }
    }

    fn record_state(&self, in_flight: usize, queue_depth: usize) {
        self.max_in_flight.fetch_max(in_flight, Ordering::Relaxed);
        let previous = self
            .max_queue_depth
            .fetch_max(queue_depth, Ordering::Relaxed);
        if queue_depth > previous {
            log::info!(
                "Poster pipeline queue depth high watermark: {} (in-flight={})",
                queue_depth,
                in_flight
            );
        }
    }

    fn maybe_warn(
        metric: &str,
        data: &VecDeque<u64>,
        median_warn_ms: u64,
        p95_warn_ms: u64,
    ) {
        if data.len() < 8 {
            return;
        }

        let mut sorted: Vec<u64> = data.iter().copied().collect();
        sorted.sort_unstable();

        let median = Self::percentile(&sorted, 0.5);
        let p95 = Self::percentile(&sorted, 0.95);

        if median > median_warn_ms || p95 > p95_warn_ms {
            log::warn!(
                "Poster pipeline latency ({}) median={}ms p95={}ms (sample={})",
                metric,
                median,
                p95,
                sorted.len()
            );
        }
    }

    fn percentile(sorted: &[u64], pct: f64) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let rank = ((sorted.len() as f64 - 1.0) * pct).round() as usize;
        sorted[rank.min(sorted.len() - 1)]
    }

    /// Returns the median load latency in ms, or None if insufficient samples.
    fn median_load_ms(&self) -> Option<u64> {
        let data = self.load_ms.lock().ok()?;
        if data.len() < 8 {
            return None;
        }
        let mut sorted: Vec<u64> = data.iter().copied().collect();
        sorted.sort_unstable();
        Some(Self::percentile(&sorted, 0.5))
    }
}
