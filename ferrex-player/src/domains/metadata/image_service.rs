use dashmap::DashMap;
use ferrex_core::player_prelude::ImageRequest;
use iced::widget::image::Handle;
use log::{info, warn};
use priority_queue::PriorityQueue;
use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

use crate::infra::constants::image::{
    IMAGE_MAX_RETRY_ATTEMPTS, IMAGE_RETRY_THROTTLE,
};
use crate::infra::constants::memory_usage;
use crate::infra::units::ByteSize;

#[cfg(target_os = "linux")]
use libc;

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
    Pending,
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
    pub estimated_bytes: ByteSize,
}

#[derive(Debug, Clone)]
pub struct UnifiedImageService {
    // Single cache for all images
    cache: Arc<DashMap<ImageRequest, ImageEntry>>,

    // Ready immutable blob tokens keyed by request (v2 image pipeline).
    ready_tokens: Arc<DashMap<ImageRequest, String>>,

    // Priority queue for pending loads (using u8 priority, higher is better)
    queue: Arc<Mutex<PriorityQueue<ImageRequest, u8>>>,

    // Currently loading requests
    loading: Arc<DashMap<ImageRequest, Instant>>,

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

    // Estimated resident bytes held by loaded image handles in this cache.
    last_known_ram_usage: Arc<AtomicU64>,

    // Hard cap for loaded images in RAM (best-effort, enforced via eviction).
    ram_max_bytes: Arc<AtomicU64>,
}

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
            ready_tokens: Arc::new(DashMap::new()),
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            loading: Arc::new(DashMap::new()),
            load_sender,
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent)),
            inflight_cancellers: Arc::new(DashMap::new()),
            telemetry: Arc::new(Telemetry::default()),
            retry_slot_counter: Arc::new(AtomicUsize::new(0)),
            last_known_ram_usage: Arc::new(AtomicU64::new(0)),
            ram_max_bytes: Arc::new(AtomicU64::new(
                memory_usage::MAX_RAM_BYTES,
            )),
        };

        (service, load_receiver)
    }

    pub fn set_ready_token(&self, request: &ImageRequest, token: String) {
        self.ready_tokens.insert(request.clone(), token);
        // Clear pending throttle so an SSE notification can trigger an immediate fetch.
        if let Some(mut entry) = self.cache.get_mut(request)
            && matches!(entry.state, LoadState::Pending)
        {
            entry.last_failure = None;
        }
    }

    pub fn ready_token(&self, request: &ImageRequest) -> Option<String> {
        self.ready_tokens.get(request).map(|t| t.value().clone())
    }

    pub fn clear_ready_token(&self, request: &ImageRequest) {
        self.ready_tokens.remove(request);
    }

    /// Maximum allowed concurrent loads for the unified image service.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    /// Hard cap for loaded image handles held in RAM (best-effort).
    pub fn set_ram_max_bytes(&self, byte_size: ByteSize) {
        self.ram_max_bytes
            .store(byte_size.as_bytes(), Ordering::SeqCst);
        self.enforce_ram_budget();
    }

    pub fn ram_max_bytes(&self) -> u64 {
        self.ram_max_bytes.load(Ordering::Relaxed)
    }

    pub fn resident_bytes(&self) -> ByteSize {
        ByteSize::from_bytes(self.last_known_ram_usage.load(Ordering::Relaxed))
    }

    /// Touch a loaded entry to keep it warm for LRU eviction.
    ///
    /// This is throttled by `min_interval` to avoid excessive churn.
    pub fn touch_loaded(&self, request: &ImageRequest, min_interval: Duration) {
        let mut entry = match self.cache.get_mut(request) {
            Some(entry) => entry,
            None => return,
        };
        if !matches!(entry.state, LoadState::Loaded(_)) {
            return;
        }
        let now = Instant::now();
        if now.duration_since(entry.last_accessed) >= min_interval {
            entry.last_accessed = now;
        }
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
            let is_pending = matches!(entry.state, LoadState::Pending);
            let is_failed_404 = matches!(entry.state, LoadState::Failed(ref err) if err.contains("404"));

            // For non-404 errors respect the max retry limit
            if is_failed
                && !is_failed_404
                && entry.retry_count >= IMAGE_MAX_RETRY_ATTEMPTS
            {
                entry.last_accessed = now;
                log::debug!(
                    "Skipping image request for {:?} - exceeded max retries ({}/{})",
                    request.iid,
                    entry.retry_count,
                    IMAGE_MAX_RETRY_ATTEMPTS
                );
                return;
            }

            // Throttle repeated retries
            // - For 404s: exponential backoff (750ms, 1500ms, 3000ms, 6000ms, 8000ms max)
            // - For other failures: keep legacy throttle
            if (is_failed || is_pending)
                && let Some(last_failure) = entry.last_failure
            {
                let throttle = if is_failed_404 {
                    // retry_count reflects the count before this request; cap growth
                    let exp = entry.retry_count.min(4) as u32; // 2^0..2^4
                    let base_ms: u64 = 750;
                    let backoff_ms = base_ms.saturating_mul(1u64 << exp);
                    std::cmp::min(backoff_ms, 8_000)
                } else {
                    IMAGE_RETRY_THROTTLE.as_millis() as u64
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
            // let is_failed_entry = self
            //     .cache
            //     .get(&request)
            //     .map(|entry| matches!(entry.state, LoadState::Failed(_)))
            //     .unwrap_or(false);

            // // Normal priorities are 1..=3; use 0 for failed retries to push
            // // them behind everything else.
            let requested_priority = request.priority.weight();
            // let effective_priority: u8 = if is_failed_entry {
            //     1
            // } else {
            //     requested_priority
            // };

            if let Some(&existing_priority) = queue.get_priority(&request) {
                // If we have a failed entry, force a demotion to the lowest
                // effective priority so it doesn't jump the line. Otherwise
                // only upgrade if the caller requested a higher priority.
                // if is_failed_entry {
                //     if existing_priority != effective_priority {
                //         queue.change_priority(&request, effective_priority);
                //         match self.load_sender.send(()) {
                //             Ok(_) => log::debug!(
                //                 "Sent wake-up signal for failed-request demotion"
                //             ),
                //             Err(e) => log::error!(
                //                 "Failed to send wake-up signal: {:?}",
                //                 e
                //             ),
                //         }
                //     }
                // } else
                if requested_priority > existing_priority {
                    /*
                    log::debug!("Upgrading priority for {:?} from {} to {} ({})",
                               request.media_id, existing_priority, requested_priority,
                               if requested_priority == 3 { "VISIBLE" } else if requested_priority == 2 { "PRELOAD" } else { "BACKGROUND" });
                     */
                    queue.change_priority(&request, requested_priority);
                    // Send wake-up signal to notify loader of priority change
                    match self.load_sender.send(()) {
                        Ok(_) => log::trace!(
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
                queue.push(request.clone(), requested_priority);
                // Send wake-up signal to notify loader of new request
                match self.load_sender.send(()) {
                    Ok(_) => log::trace!(
                        "Sent wake-up signal for new request: {:?} (priority {})",
                        request,
                        requested_priority
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
            existing_estimated_bytes,
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
                    entry.estimated_bytes,
                )
            })
            .unwrap_or((None, 0, None, Some(now), None, ByteSize::ZERO));
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
                estimated_bytes: existing_estimated_bytes,
            },
        );
    }

    pub fn mark_loaded(
        &self,
        request: &ImageRequest,
        handle: Handle,
        estimated_bytes: u64,
    ) {
        self.clear_ready_token(request);
        self.loading.remove(request);
        let now = std::time::Instant::now();

        let (
            existing_hint,
            prev_requested,
            prev_loading_started,
            prev_first_displayed,
            prev_estimated_bytes,
        ) = {
            if let Some(entry) = self.cache.get(request) {
                (
                    entry.first_display_hint,
                    entry.requested_at,
                    entry.loading_started_at,
                    entry.first_displayed_at,
                    entry.estimated_bytes,
                )
            } else {
                (None, None, None, None, ByteSize::ZERO)
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
                estimated_bytes: ByteSize::from_bytes(estimated_bytes),
            },
        );
        self.clear_inflight_cancel(request);

        self.adjust_resident_bytes(
            prev_estimated_bytes.as_bytes(),
            estimated_bytes,
        );
        self.enforce_ram_budget();

        if let Some(started) = prev_loading_started {
            let since = now.saturating_duration_since(started);

            // TODO: Implement more robust detection of loaded versus displayed
            if since < Duration::from_secs(1) {
                self.telemetry.record_load_duration(
                    now.saturating_duration_since(started),
                );
            }
        }
    }

    pub fn mark_failed(&self, request: &ImageRequest, error: String) {
        self.clear_ready_token(request);
        self.loading.remove(request);

        // Check if this is a 404 error (image doesn't exist on server)
        let is_404 = error.contains("404");

        let now = std::time::Instant::now();
        let retry_count = match self.cache.get_mut(request) {
            Some(mut entry) => {
                // If this entry previously held a loaded handle, drop it and
                // account for released bytes so RAM budgeting stays consistent.
                if matches!(entry.state, LoadState::Loaded(_))
                    && entry.estimated_bytes > ByteSize::ZERO
                {
                    self.last_known_ram_usage.fetch_sub(
                        entry.estimated_bytes.as_bytes(),
                        Ordering::Relaxed,
                    );
                    entry.estimated_bytes = ByteSize::ZERO;
                }
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
                        estimated_bytes: ByteSize::ZERO,
                    },
                );
                retry_count
            }
        };

        // Log permanent failures for metadata aggregation
        if retry_count >= IMAGE_MAX_RETRY_ATTEMPTS && !is_404 {
            log::warn!(
                "Image permanently failed after {} attempts: {:?} - {}{}",
                retry_count,
                request.iid,
                error,
                if is_404 { " [404]" } else { "" }
            );
            // TODO: Could aggregate these failures for missing metadata reporting
        } else if is_404 {
            log::debug!(
                "Image temporarily unavailable ({} attempts so far): {:?}",
                retry_count,
                request.iid
            );
        } else {
            log::debug!(
                "Image failed (attempt {}/{}): {:?} - {}{}",
                retry_count,
                IMAGE_MAX_RETRY_ATTEMPTS,
                request.iid,
                error,
                if is_404 { " [404]" } else { "" }
            );
        }
        self.clear_inflight_cancel(request);
    }

    pub fn mark_pending(&self, request: &ImageRequest) {
        self.clear_ready_token(request);
        self.loading.remove(request);

        let now = std::time::Instant::now();
        match self.cache.get_mut(request) {
            Some(mut entry) => {
                if matches!(entry.state, LoadState::Loaded(_))
                    && entry.estimated_bytes > ByteSize::ZERO
                {
                    self.last_known_ram_usage.fetch_sub(
                        entry.estimated_bytes.as_bytes(),
                        Ordering::Relaxed,
                    );
                    entry.estimated_bytes = ByteSize::ZERO;
                }
                entry.state = LoadState::Pending;
                entry.last_accessed = now;
                entry.last_failure = Some(now);
            }
            None => {
                self.cache.insert(
                    request.clone(),
                    ImageEntry {
                        state: LoadState::Pending,
                        last_accessed: now,
                        loaded_at: None,
                        requested_at: Some(now),
                        loading_started_at: Some(now),
                        first_displayed_at: None,
                        retry_count: 0,
                        last_failure: Some(now),
                        first_display_hint: None,
                        estimated_bytes: ByteSize::ZERO,
                    },
                );
            }
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
                estimated_bytes: ByteSize::ZERO,
            },
        );
    }

    pub fn mark_cancelled(&self, request: &ImageRequest) {
        self.clear_ready_token(request);
        self.loading.remove(request);
        let _ = self.remove_entry_and_account(request);
        self.clear_inflight_cancel(request);
    }

    /// Pop up to `max` queued requests (highest priority first) for manifest lookup.
    ///
    /// This removes requests from the queue temporarily; callers should requeue
    /// any requests they still want to download after manifest processing.
    pub fn take_manifest_batch(&self, max: usize) -> Vec<ImageRequest> {
        let Ok(mut queue) = self.queue.lock() else {
            return Vec::new();
        };

        let mut batch = Vec::with_capacity(max);
        let mut skipped = Vec::new();

        while batch.len() < max {
            let Some((req, prio)) = queue.pop() else {
                break;
            };
            if self.loading.contains_key(&req) {
                skipped.push((req, prio));
                continue;
            }
            batch.push(req);
        }

        // Restore any items we skipped due to being in-flight.
        for (req, prio) in skipped {
            queue.push(req, prio);
        }

        batch
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
        if let Some((top_request, _prio)) = queue.peek()
            && !self.loading.contains_key(top_request)
        {
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
        if use_retry_slot && let Some(ref retry) = retry_candidate {
            queue.remove(retry);
            return Some(retry.clone());
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
        counter.is_multiple_of(5)
    }

    pub fn cleanup_stale_entries(&self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut to_remove = Vec::new();

        for entry in self.cache.iter() {
            if now.duration_since(entry.last_accessed) > max_age
                && (matches!(entry.state, LoadState::Failed(_))
                    || matches!(entry.state, LoadState::Pending)
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
            let _ = self.remove_entry_and_account(&key);
            self.loading.remove(&key);
        }

        self.enforce_ram_budget();
    }

    fn adjust_resident_bytes(&self, prev: u64, next: u64) {
        if next == prev {
            return;
        }
        if next > prev {
            self.last_known_ram_usage
                .fetch_add(next - prev, Ordering::Relaxed);
        } else {
            self.last_known_ram_usage
                .fetch_sub(prev - next, Ordering::Relaxed);
        }
    }

    fn remove_entry_and_account(&self, request: &ImageRequest) -> u64 {
        let removed = self.cache.remove(request);
        if let Some((_key, entry)) = removed
            && matches!(entry.state, LoadState::Loaded(_))
            && entry.estimated_bytes > ByteSize::ZERO
        {
            let bytes = entry.estimated_bytes.as_bytes();
            self.last_known_ram_usage
                .fetch_sub(bytes, Ordering::Relaxed);
            bytes
        } else {
            0
        }
    }

    /// Enforce `ram_max_bytes` by evicting least-recently-accessed loaded entries.
    ///
    /// This is best-effort and intentionally simple: it prioritizes stability
    /// (no unbounded growth) over perfect LRU fidelity.
    pub fn enforce_ram_budget(&self) {
        let max = self.ram_max_bytes.load(Ordering::Relaxed);
        let mut resident = self.last_known_ram_usage.load(Ordering::Relaxed);
        if resident <= max {
            return;
        }

        // When we are over budget, evict down to a lower "water mark" to avoid
        // immediately bouncing back over the cap on the next couple loads.
        //
        // This also tends to be more effective at reducing *observed* RSS on
        // many allocators because it frees a larger contiguous amount at once.
        let target = max.saturating_sub(max / 10); // 90%

        #[derive(Debug)]
        struct Candidate {
            request: ImageRequest,
            last_accessed: Instant,
        }

        let mut candidates: Vec<Candidate> = Vec::new();
        for entry in self.cache.iter() {
            if matches!(entry.state, LoadState::Loaded(_)) {
                candidates.push(Candidate {
                    request: entry.key().clone(),
                    last_accessed: entry.last_accessed,
                });
            }
        }

        candidates.sort_by_key(|c| c.last_accessed);

        let mut removed = 0usize;
        let mut freed = 0u64;
        for cand in candidates {
            if resident <= target {
                break;
            }

            // Evict from the cache (dropping the handle) and update resident accounting.
            let evicted_bytes = self.remove_entry_and_account(&cand.request);

            if evicted_bytes > 0 {
                resident = resident.saturating_sub(evicted_bytes);
                freed = freed.saturating_add(evicted_bytes);
                removed += 1;
            } else {
                // Even if the size estimate is zero, keep going to ensure we
                // don't get stuck above the cap.
                removed += 1;
            }
        }

        // Re-read the authoritative resident estimate after evictions.
        resident = self.last_known_ram_usage.load(Ordering::Relaxed);

        if freed > 0 {
            info!(
                "Image RAM cap: evicted {} images (~{:.1}MiB) => {:.1}MiB / {:.1}MiB",
                removed,
                ByteSize::from_bytes(freed).as_mib(),
                ByteSize::from_bytes(resident).as_mib(),
                ByteSize::from_bytes(max).as_mib(),
            );

            // Best-effort: try to encourage the allocator to return freed pages
            // back to the OS on Linux/glibc. This helps the *process RSS* match
            // the image cache budget more closely.
            //
            // This is intentionally coarse-grained to avoid performance cliffs.
            #[cfg(target_os = "linux")]
            {
                const TRIM_THRESHOLD_BYTES: u64 = 32 * 1024 * 1024; // 32MiB
                if freed >= TRIM_THRESHOLD_BYTES {
                    // SAFETY: malloc_trim is a C API; it is safe to call as a best-effort hint.
                    // It returns non-zero if it released memory.
                    let trimmed = unsafe { libc::malloc_trim(0) };
                    if trimmed != 0 {
                        log::debug!(
                            "Image RAM cap: malloc_trim freed pages back to OS (freed_est≈{:.1}MiB)",
                            ByteSize::from_bytes(freed).as_mib()
                        );
                    }
                }
            }
        } else {
            warn!(
                "Image RAM cap: attempted eviction of {} images but freed 0 bytes (resident {:.1}MiB / cap {:.1}MiB)",
                removed,
                ByteSize::from_bytes(resident).as_mib(),
                ByteSize::from_bytes(max).as_mib(),
            );
        }

        self.cache.shrink_to_fit();
    }

    /// Returns the current number of queued requests (not counting those already loading).
    // TODO: This should return a result, not 0
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
    // TODO: Give the settings treatment
    const MIN_CONCURRENT: usize = 8;
    const MAX_CONCURRENT: usize = 128;

    /// Adjusts concurrency based on observed load latency.
    /// Uses a target-relative approach: compares current latency to a learned baseline.
    /// Includes hysteresis to prevent oscillation and faster convergence when far from target.
    pub fn adapt_concurrency(&self) {
        let Some(median_ms) = self.telemetry.median_load_ms() else {
            return; // Not enough samples yet
        };

        log::trace!("median_ms: {}", median_ms);

        let current = self.max_concurrent.load(Ordering::Relaxed);
        let queue_depth = self.queue_len();

        // Safety: if queue is backing up significantly, reduce concurrency regardless of latency
        // This catches cases where high concurrency causes system-wide slowdown
        if queue_depth > 100 && current > Self::MIN_CONCURRENT {
            let reduced = (current / 2).max(Self::MIN_CONCURRENT);

            if reduced < (Self::MAX_CONCURRENT - Self::MIN_CONCURRENT) / 4 {
                log::warn!(
                    "Setting poster concurrency below 25% maximum due to queue backup: {} → {} (queue backup: {} items)",
                    current,
                    reduced,
                    queue_depth
                );
            }
            self.max_concurrent.store(reduced, Ordering::SeqCst);
            return;
        }

        // Target latency: we want sub-200ms loads for good UX
        // - Below 80ms: definitely have headroom, can increase
        // - Above 400ms: definitely saturated, should decrease
        // - 120-400ms: stable zone with hysteresis
        const TARGET_LOW_MS: u64 = 40;
        const TARGET_HIGH_MS: u64 = 200;

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
        }
    }

    fn record_state(&self, in_flight: usize, queue_depth: usize) {
        self.max_in_flight.fetch_max(in_flight, Ordering::Relaxed);
        let previous = self
            .max_queue_depth
            .fetch_max(queue_depth, Ordering::Relaxed);
        if queue_depth > previous {
            log::info!(
                "Poster provider queue depth high watermark: {} (in-flight={})",
                queue_depth,
                in_flight
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

#[cfg(test)]
mod tests {
    use super::UnifiedImageService;
    use crate::infra::units::ByteSize;
    use ferrex_core::player_prelude::ImageRequest;
    use ferrex_model::image::{ImageSize, PosterSize};
    use iced::widget::image::Handle;
    use std::time::Duration;
    use uuid::Uuid;

    fn rgba_handle(width: u32, height: u32) -> Handle {
        let bytes = vec![0u8; (width * height * 4) as usize];
        Handle::from_rgba(width, height, bytes)
    }

    #[test]
    fn evicts_least_recently_accessed_until_under_cap() {
        let (svc, _rx) = UnifiedImageService::new(4);

        // Prevent eviction while loading so we can arrange access timestamps deterministically.
        svc.set_ram_max_bytes(ByteSize::from_bytes(10_000));

        let a = ImageRequest::new(
            Uuid::new_v4(),
            ImageSize::Poster(PosterSize::W185),
        );
        let b = ImageRequest::new(
            Uuid::new_v4(),
            ImageSize::Poster(PosterSize::W185),
        );
        let c = ImageRequest::new(
            Uuid::new_v4(),
            ImageSize::Poster(PosterSize::W185),
        );

        let per_image = 10u32 * 10u32 * 4u32; // RGBA
        let per_image = per_image as u64;

        svc.mark_loaded(&a, rgba_handle(10, 10), per_image);
        svc.mark_loaded(&b, rgba_handle(10, 10), per_image);
        svc.mark_loaded(&c, rgba_handle(10, 10), per_image);

        let now = std::time::Instant::now();
        if let Some(mut entry) = svc.cache.get_mut(&a) {
            entry.last_accessed = now - Duration::from_secs(30);
        }
        if let Some(mut entry) = svc.cache.get_mut(&b) {
            entry.last_accessed = now - Duration::from_secs(20);
        }
        if let Some(mut entry) = svc.cache.get_mut(&c) {
            entry.last_accessed = now - Duration::from_secs(10);
        }

        // Total = 3 * 400 = 1200 bytes. Cap at 500 => should evict A then B, keeping C.
        svc.set_ram_max_bytes(ByteSize::from_bytes(500));

        assert!(svc.get(&c).is_some());
        assert!(svc.get(&a).is_none());
        assert!(svc.get(&b).is_none());
        assert!(svc.resident_bytes().as_bytes() <= 500);
    }

    #[test]
    fn evicts_one_when_slightly_over_cap() {
        let (svc, _rx) = UnifiedImageService::new(4);

        svc.set_ram_max_bytes(ByteSize::from_bytes(10_000));

        let old = ImageRequest::new(
            Uuid::new_v4(),
            ImageSize::Poster(PosterSize::W185),
        );
        let new = ImageRequest::new(
            Uuid::new_v4(),
            ImageSize::Poster(PosterSize::W185),
        );

        let per_image = 10u32 * 10u32 * 4u32; // RGBA
        let per_image = per_image as u64;

        svc.mark_loaded(&old, rgba_handle(10, 10), per_image);
        svc.mark_loaded(&new, rgba_handle(10, 10), per_image);

        let now = std::time::Instant::now();
        if let Some(mut entry) = svc.cache.get_mut(&old) {
            entry.last_accessed = now - Duration::from_secs(30);
        }
        if let Some(mut entry) = svc.cache.get_mut(&new) {
            entry.last_accessed = now - Duration::from_secs(10);
        }

        // Total = 800 bytes; cap at 600 => should evict only the oldest entry.
        svc.set_ram_max_bytes(ByteSize::from_bytes(600));

        assert!(svc.get(&new).is_some());
        assert!(svc.get(&old).is_none());
        assert!(svc.resident_bytes().as_bytes() <= 600);
    }
}
