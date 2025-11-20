use dashmap::DashMap;
use ferrex_core::ImageRequest;
use iced::widget::image::Handle;
use priority_queue::PriorityQueue;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;

// Maximum number of retry attempts for failed images
const MAX_RETRY_ATTEMPTS: u8 = 5;

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
    pub retry_count: u8,
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
    max_concurrent: usize,
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
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            loading: Arc::new(DashMap::new()),
            load_sender,
            max_concurrent,
        };

        (service, load_receiver)
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
                LoadState::Loaded(handle) => Some((handle.clone(), entry.loaded_at)),
                _ => None,
            })
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
            entry.last_accessed = std::time::Instant::now();

            // Don't retry if already loaded
            if matches!(entry.state, LoadState::Loaded(_)) {
                return;
            }

            // Don't retry if failed too many times
            if matches!(entry.state, LoadState::Failed(_))
                && entry.retry_count >= MAX_RETRY_ATTEMPTS
            {
                log::debug!(
                    "Skipping image request for {:?} - exceeded max retries ({}/{})",
                    request.media_id,
                    entry.retry_count,
                    MAX_RETRY_ATTEMPTS
                );
                return;
            }
        }

        // Check if already loading
        if self.loading.contains_key(&request) {
            return;
        }

        // Add to queue or upgrade priority
        if let Ok(mut queue) = self.queue.lock() {
            let new_priority = request.priority.weight();

            if let Some(&existing_priority) = queue.get_priority(&request) {
                // Image already queued - upgrade priority if new is higher
                if new_priority > existing_priority {
                    /*
                    log::debug!("Upgrading priority for {:?} from {} to {} ({})",
                               request.media_id, existing_priority, new_priority,
                               if new_priority == 3 { "VISIBLE" } else if new_priority == 2 { "PRELOAD" } else { "BACKGROUND" });
                     */
                    queue.change_priority(&request, new_priority);
                    // Send wake-up signal to notify loader of priority change
                    match self.load_sender.send(()) {
                        Ok(_) => log::debug!("Sent wake-up signal for priority upgrade"),
                        Err(e) => log::error!("Failed to send wake-up signal: {:?}", e),
                    }
                }
            } else {
                // New request - add to queue
                queue.push(request.clone(), new_priority);
                // Send wake-up signal to notify loader of new request
                match self.load_sender.send(()) {
                    Ok(_) => log::debug!("Sent wake-up signal for new request: {:?}", request),
                    Err(e) => log::error!("Failed to send wake-up signal: {:?}", e),
                }
            }
        }
    }

    pub fn mark_loading(&self, request: &ImageRequest) {
        self.loading
            .insert(request.clone(), std::time::Instant::now());
        self.cache.insert(
            request.clone(),
            ImageEntry {
                state: LoadState::Loading,
                last_accessed: std::time::Instant::now(),
                loaded_at: None,
                retry_count: 0,
            },
        );
    }

    pub fn mark_loaded(&self, request: &ImageRequest, handle: Handle) {
        self.loading.remove(request);
        let now = std::time::Instant::now();

        //log::debug!("mark_loaded called for {:?}", request.media_id);
        //log::debug!("  - Setting loaded_at to: {:?}", now);

        self.cache.insert(
            request.clone(),
            ImageEntry {
                state: LoadState::Loaded(handle),
                last_accessed: now,
                loaded_at: Some(now),
                retry_count: 0,
            },
        );
    }

    pub fn mark_failed(&self, request: &ImageRequest, error: String) {
        self.loading.remove(request);

        // Check if this is a 404 error (image doesn't exist on server)
        let is_404 = error.contains("404");

        let retry_count = match self.cache.get_mut(request) {
            Some(mut entry) => {
                entry.state = LoadState::Failed(error.clone());
                entry.retry_count = entry.retry_count.saturating_add(1);
                entry.retry_count
            }
            _ => {
                let retry_count = 1;
                self.cache.insert(
                    request.clone(),
                    ImageEntry {
                        state: LoadState::Failed(error.clone()),
                        last_accessed: std::time::Instant::now(),
                        loaded_at: None,
                        retry_count,
                    },
                );
                retry_count
            }
        };

        // Log permanent failures for metadata aggregation
        if retry_count >= MAX_RETRY_ATTEMPTS {
            log::warn!(
                "Image permanently failed after {} attempts: {:?} - {}{}",
                retry_count,
                request.media_id,
                error,
                if is_404 { " [404]" } else { "" }
            );
            // TODO: Could aggregate these failures for missing metadata reporting
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
    }

    pub fn get_next_request(&self) -> Option<ImageRequest> {
        let mut queue = self.queue.lock().ok()?;

        if self.loading.len() > self.max_concurrent {
            return None;
        }

        if let Some((request, _priority)) = queue.pop() {
            if !self.loading.contains_key(&request) {
                return Some(request);
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn cleanup_stale_entries(&self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut to_remove = Vec::new();

        for entry in self.cache.iter() {
            if now.duration_since(entry.last_accessed) > max_age {
                if matches!(entry.state, LoadState::Failed(_))
                    || (matches!(entry.state, LoadState::Loading)
                        && self.loading.get(entry.key()).map_or(true, |start| {
                            now.duration_since(*start) > std::time::Duration::from_secs(30)
                        }))
                {
                    to_remove.push(entry.key().clone());
                }
            }
        }

        for key in to_remove {
            self.cache.remove(&key);
            self.loading.remove(&key);
        }
    }
}
