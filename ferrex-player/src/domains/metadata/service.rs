use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering as AtomicOrdering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::messages::Message;
use crate::infrastructure::api_types::{MediaId, MediaReference};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;

/// Priority levels for fetch requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FetchPriority {
    High = 0,   // Visible items in current view
    Medium = 1, // Non-visible items in current library
    Low = 2,    // Items from other libraries
}

/// A request to fetch metadata for a media item
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub priority: FetchPriority,
    pub media_id: MediaId,
    pub library_id: Uuid,
    pub timestamp: Instant,
}

impl Eq for FetchRequest {}

impl PartialEq for FetchRequest {
    fn eq(&self, other: &Self) -> bool {
        self.media_id == other.media_id
    }
}

impl Ord for FetchRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower priority value = higher priority (reversed)
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| self.timestamp.cmp(&other.timestamp))
    }
}

impl PartialOrd for FetchRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Service that manages concurrent metadata fetching with prioritization
pub struct MetadataFetchService {
    workers: Vec<JoinHandle<()>>,
    request_queue: Arc<Mutex<BinaryHeap<FetchRequest>>>,
    queue_notify: Arc<Notify>,
    shared_cache: Arc<RwLock<HashMap<MediaId, MediaReference>>>,
    in_flight: Arc<Mutex<HashSet<MediaId>>>,
    shutdown: Arc<AtomicBool>,
    message_sender: Option<mpsc::UnboundedSender<Message>>,
    api_service: Arc<ApiClientAdapter>,
    // Metrics
    queued_count: Arc<Mutex<HashMap<FetchPriority, usize>>>,
}

impl MetadataFetchService {
    /// Create a new metadata fetch service with the specified number of workers
    pub fn new(
        api_service: Arc<ApiClientAdapter>,
        worker_count: usize,
        shared_cache: Arc<RwLock<HashMap<MediaId, MediaReference>>>,
        message_sender: Option<mpsc::UnboundedSender<Message>>,
    ) -> Self {
        let request_queue = Arc::new(Mutex::new(BinaryHeap::<FetchRequest>::new()));
        let queue_notify = Arc::new(Notify::new());
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let queued_count = Arc::new(Mutex::new(HashMap::<FetchPriority, usize>::new()));

        // Initialize metrics
        {
            match queued_count.try_lock() {
                Ok(mut counts) => {
                    counts.insert(FetchPriority::High, 0);
                    counts.insert(FetchPriority::Medium, 0);
                    counts.insert(FetchPriority::Low, 0);
                }
                Err(_) => {
                    // This shouldn't happen during initialization
                    log::warn!("Could not initialize queue metrics - lock busy");
                }
            }
        }

        // Spawn worker threads
        let mut workers = Vec::new();

        for worker_id in 0..worker_count {
            let api_service = Arc::clone(&api_service);
            let request_queue = Arc::clone(&request_queue);
            let queue_notify = Arc::clone(&queue_notify);
            let shared_cache: Arc<RwLock<HashMap<MediaId, MediaReference>>> =
                Arc::clone(&shared_cache);
            let in_flight = Arc::clone(&in_flight);
            let shutdown = Arc::clone(&shutdown);
            let message_sender = message_sender.clone();
            let queued_count = Arc::clone(&queued_count);

            let worker = match tokio::runtime::Handle::try_current() {
                Ok(_) => {
                    log::info!(
                        "Spawning metadata worker {} in existing tokio runtime",
                        worker_id
                    );
                    tokio::spawn(async move {
                        log::info!("Metadata fetch worker {} started", worker_id);

                        // Constants for batching
                        const BATCH_SIZE: usize = 100;
                        const BATCH_TIMEOUT: Duration = Duration::from_millis(500);
                        const HIGH_PRIORITY_BATCH_SIZE: usize = 30; // Optimized for initial visible items

                        while !shutdown.load(AtomicOrdering::Relaxed) {
                            // Collect requests for batching
                            let mut batch_requests = Vec::new();
                            let batch_start = Instant::now();
                            let mut batch_priority = FetchPriority::Low;
                            let mut batch_library_id = None;

                            // Wait for first item
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                                    // Timeout - check queue anyway
                                }
                                _ = queue_notify.notified() => {
                                    // New items available
                                }
                            }

                            // Collect batch of requests
                            'batch_collect: loop {
                                let should_process_batch = {
                                    let mut queue = request_queue.lock().await;

                                    // Determine batch size based on priority
                                    let target_batch_size = if batch_priority == FetchPriority::High
                                    {
                                        HIGH_PRIORITY_BATCH_SIZE
                                    } else {
                                        BATCH_SIZE
                                    };

                                    while batch_requests.len() < target_batch_size {
                                        if let Some(req) = queue.pop() {
                                            log::debug!(
                                                "Worker {} popped {:?} with priority {:?} (queue size before: {})",
                                                worker_id,
                                                req.media_id,
                                                req.priority,
                                                queue.len() + 1
                                            );

                                            // Update metrics
                                            let mut counts = queued_count.lock().await;
                                            if let Some(count) = counts.get_mut(&req.priority) {
                                                *count = (*count).saturating_sub(1);
                                            }
                                            drop(counts);

                                            // Check if already in flight or cached
                                            let mut in_flight_set = in_flight.lock().await;
                                            if in_flight_set.contains(&req.media_id) {
                                                continue;
                                            }

                                            let is_cached = shared_cache
                                                .read()
                                                .await
                                                .contains_key(&req.media_id);
                                            if is_cached {
                                                continue;
                                            }

                                            in_flight_set.insert(req.media_id.clone());
                                            drop(in_flight_set);

                                            // Set batch properties from first item
                                            if batch_requests.is_empty() {
                                                batch_priority = req.priority;
                                                batch_library_id = Some(req.library_id);
                                            }

                                            // Only batch items with same priority and library
                                            if req.priority == batch_priority
                                                && Some(req.library_id) == batch_library_id
                                            {
                                                batch_requests.push(req);
                                            } else {
                                                // Put it back and process current batch
                                                queue.push(req);
                                                break;
                                            }
                                        } else {
                                            break;
                                        }
                                    }

                                    // Decide if we should process the batch
                                    !batch_requests.is_empty()
                                        && (batch_requests.len() >= target_batch_size
                                            || batch_start.elapsed() >= BATCH_TIMEOUT
                                            || queue.is_empty())
                                };

                                if should_process_batch {
                                    break;
                                }

                                // Wait a bit before checking again
                                tokio::time::sleep(Duration::from_millis(50)).await;

                                // Check for timeout
                                if batch_start.elapsed() >= BATCH_TIMEOUT
                                    && !batch_requests.is_empty()
                                {
                                    break;
                                }
                            }

                            // Process the batch
                            if !batch_requests.is_empty() {
                                let library_id = batch_library_id.unwrap();
                                let media_ids: Vec<_> =
                                    batch_requests.iter().map(|r| r.media_id.clone()).collect();

                                log::info!(
                                    "Worker {} processing batch of {} {:?} priority items for library {}",
                                    worker_id,
                                    media_ids.len(),
                                    batch_priority,
                                    library_id
                                );

                                // Fetch batch from server
                                match crate::domains::media::library::fetch_media_details_batch(
                                    &api_service,
                                    library_id,
                                    media_ids.clone(),
                                )
                                .await
                                {
                                    Ok(batch_response) => {
                                        log::info!(
                                            "Worker {} fetched batch: {} successful, {} errors",
                                            worker_id,
                                            batch_response.items.len(),
                                            batch_response.errors.len()
                                        );

                                        // Send batch metadata ready event instead of individual updates
                                        if !batch_response.items.is_empty() {
                                            // Note: In a real implementation, we'd need a way to emit cross-domain events
                                            // from workers. For now, we'll keep the cache update and individual messages
                                            // as a placeholder. This requires architectural changes beyond metadata domain.

                                            // Update cache and send updates (temporary until cross-domain event system is ready)
                                            for media in batch_response.items {
                                                // Use trait method to get media ID
                                                let media_id = media.as_ref().id();

                                                // Update cache
                                                shared_cache
                                                    .write()
                                                    .await
                                                    .insert(media_id.clone(), media.clone());

                                                // Send individual update message (for backward compatibility)
                                                if let Some(sender) = &message_sender {
                                                    let _ = sender
                                                        .send(Message::MediaDetailsUpdated(media));
                                                }
                                            }
                                        }

                                        // Log errors
                                        for (media_id, error) in batch_response.errors {
                                            log::warn!(
                                                "Worker {} failed to fetch {:?}: {}",
                                                worker_id,
                                                media_id,
                                                error
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Worker {} failed to fetch batch: {}",
                                            worker_id,
                                            e
                                        );
                                    }
                                }

                                // Remove from in-flight
                                {
                                    let mut in_flight_set = in_flight.lock().await;
                                    for media_id in media_ids {
                                        in_flight_set.remove(&media_id);
                                    }
                                }
                            }
                        }

                        log::info!("Metadata fetch worker {} shutting down", worker_id);
                    })
                }
                Err(_) => {
                    log::error!(
                        "No tokio runtime available for metadata worker {}!",
                        worker_id
                    );
                    // Create a dummy handle
                    tokio::task::spawn_blocking(move || {
                        log::error!(
                            "Metadata worker {} cannot run without tokio runtime",
                            worker_id
                        );
                    })
                }
            };

            workers.push(worker);
        }

        Self {
            workers,
            request_queue,
            queue_notify,
            shared_cache,
            in_flight,
            shutdown,
            message_sender,
            api_service,
            queued_count,
        }
    }

    /// Queue a fetch request with the given priority
    /// TODO: This service still uses direct cache access and individual messages.
    /// Future improvement: Replace with CrossDomainEvent::MetadataUpdated emissions.
    pub fn queue_request(&self, request: FetchRequest) {
        if self.shutdown.load(AtomicOrdering::Relaxed) {
            return;
        }

        let priority = request.priority;
        let media_id = request.media_id.clone();

        // Since this is called from sync context, we use try_lock to avoid blocking
        let queued = match self.request_queue.try_lock() {
            Ok(mut queue) => {
                // Check if already in queue
                let existing_request = queue.iter().find(|r| r.media_id == media_id);

                if let Some(existing) = existing_request {
                    // If item is already queued with lower priority, upgrade it
                    if existing.priority > priority {
                        log::info!(
                            "Upgrading {:?} from {:?} to {:?} priority",
                            media_id,
                            existing.priority,
                            priority
                        );

                        // Remove old request and add new one with higher priority
                        let mut temp_queue = Vec::new();
                        while let Some(req) = queue.pop() {
                            if req.media_id != media_id {
                                temp_queue.push(req);
                            } else {
                                // Update metrics - remove old priority count
                                if let Ok(mut counts) = self.queued_count.try_lock() {
                                    if let Some(count) = counts.get_mut(&req.priority) {
                                        *count = (*count).saturating_sub(1);
                                    }
                                }
                            }
                        }

                        // Add back all other requests
                        for req in temp_queue {
                            queue.push(req);
                        }

                        // Add the new higher priority request
                        queue.push(request);

                        // Update metrics - add new priority count
                        if let Ok(mut counts) = self.queued_count.try_lock() {
                            if let Some(count) = counts.get_mut(&priority) {
                                *count += 1;
                            }
                        }

                        true
                    } else {
                        log::trace!(
                            "Item {:?} already in queue with same or higher priority, skipping",
                            media_id
                        );
                        false
                    }
                } else {
                    // Not in queue, add it
                    queue.push(request);

                    // Update metrics
                    if let Ok(mut counts) = self.queued_count.try_lock() {
                        if let Some(count) = counts.get_mut(&priority) {
                            *count += 1;
                        }

                        let total_queued: usize = counts.values().sum();
                        if priority == FetchPriority::High {
                            log::info!(
                                "Queued HIGH priority {:?} (total in queue: {}, high: {}, medium: {}, low: {})",
                                media_id,
                                total_queued,
                                counts.get(&FetchPriority::High).unwrap_or(&0),
                                counts.get(&FetchPriority::Medium).unwrap_or(&0),
                                counts.get(&FetchPriority::Low).unwrap_or(&0)
                            );
                        } else {
                            log::debug!(
                                "Queued {:?} priority {:?} (total in queue: {})",
                                priority,
                                media_id,
                                total_queued
                            );
                        }
                    }

                    true
                }
            }
            Err(_) => {
                log::warn!(
                    "Could not acquire queue lock to add {:?}, will retry",
                    media_id
                );
                // In production, you might want to implement a retry mechanism
                false
            }
        };

        // Wake up a worker immediately if we queued something
        if queued {
            self.queue_notify.notify_one();

            // For high priority items, wake all workers to ensure immediate processing
            if priority == FetchPriority::High {
                self.queue_notify.notify_waiters();
            }
        }
    }

    /// Queue multiple items for fetching
    pub fn queue_items(&self, items: Vec<(MediaId, Uuid)>, priority: FetchPriority) {
        let item_count = items.len();
        log::info!(
            "queue_items called with {} items, priority {:?}",
            item_count,
            priority
        );
        let base_timestamp = Instant::now();
        for (index, (media_id, library_id)) in items.into_iter().enumerate() {
            // Add a small offset to timestamp to preserve order within same priority
            let timestamp = base_timestamp + Duration::from_micros(index as u64);
            self.queue_request(FetchRequest {
                priority,
                media_id,
                library_id,
                timestamp,
            });
        }
        log::info!("Finished queueing {} items", item_count);
    }

    /// Check if an item is already cached
    pub async fn is_cached(&self, media_id: &MediaId) -> bool {
        self.shared_cache.read().await.contains_key(media_id)
    }

    /// Get the shared cache reference
    pub fn cache(&self) -> Arc<RwLock<HashMap<MediaId, MediaReference>>> {
        Arc::clone(&self.shared_cache)
    }

    /// Shutdown the service
    pub async fn shutdown(mut self) {
        self.shutdown.store(true, AtomicOrdering::Relaxed);

        // Wait for all workers to finish
        for worker in self.workers.drain(..) {
            let _ = worker.await;
        }
    }
}

impl Drop for MetadataFetchService {
    fn drop(&mut self) {
        self.shutdown.store(true, AtomicOrdering::Relaxed);
    }
}

impl std::fmt::Debug for MetadataFetchService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetadataFetchService")
            .field("workers_count", &self.workers.len())
            .field("shutdown", &self.shutdown.load(AtomicOrdering::Relaxed))
            .finish()
    }
}
