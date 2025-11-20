use super::image_types::ImageRequest;
use dashmap::DashMap;
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
        //log::info!("Requesting image with request: {:#?}", request);
        // Check if already cached
        if let Some(mut entry) = self.cache.get_mut(&request) {
            entry.last_accessed = std::time::Instant::now();
            
            // Don't retry if already loaded
            if matches!(entry.state, LoadState::Loaded(_)) {
                return;
            }
            
            // Don't retry if failed too many times
            if matches!(entry.state, LoadState::Failed(_)) && entry.retry_count >= MAX_RETRY_ATTEMPTS {
                log::debug!("Skipping image request for {:?} - exceeded max retries ({}/{})", 
                          request.media_id, entry.retry_count, MAX_RETRY_ATTEMPTS);
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
                    log::info!("Upgrading priority for {:?} from {} to {} ({})", 
                               request.media_id, existing_priority, new_priority,
                               if new_priority == 3 { "VISIBLE" } else if new_priority == 2 { "PRELOAD" } else { "BACKGROUND" });
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
        
        let retry_count = if let Some(mut entry) = self.cache.get_mut(request) {
            entry.state = LoadState::Failed(error.clone());
            // For 404 errors, immediately set to max retries to prevent further attempts
            if is_404 {
                entry.retry_count = MAX_RETRY_ATTEMPTS;
            } else {
                entry.retry_count += 1;
            }
            entry.retry_count
        } else {
            let retry_count = if is_404 { MAX_RETRY_ATTEMPTS } else { 1 };
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
        };
        
        // Log permanent failures for metadata aggregation
        if retry_count >= MAX_RETRY_ATTEMPTS {
            if is_404 {
                log::info!("Image not found on server (404): {:?}", request.media_id);
            } else {
                log::warn!("Image permanently failed after {} attempts: {:?} - {}", 
                          retry_count, request.media_id, error);
            }
            // TODO: Could aggregate these failures for missing metadata reporting
        } else {
            log::debug!("Image failed (attempt {}/{}): {:?} - {}", 
                       retry_count, MAX_RETRY_ATTEMPTS, request.media_id, error);
        }
    }

    pub fn get_next_request(&self) -> Option<ImageRequest> {
        let mut queue = self.queue.lock().ok()?;

        // Only process one at a time for staggered loading effect
        // This ensures images load sequentially, not in parallel
        if !self.loading.is_empty() {
            return None;
        }

        // Pop highest priority item
        while let Some((request, _priority)) = queue.pop() {
            if !self.loading.contains_key(&request) {
                return Some(request);
            }
        }

        None
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

/*
// URL Resolution - uses endpoints from server response types
pub async fn resolve_image_url(request: &ImageRequest, state: &State) -> Result<String, String> {
    match &request.media_id {
        MediaId::Movie(id) => resolve_movie_url(id, state).await,
        MediaId::Series(id) => resolve_series_url(id, state).await,
        MediaId::Season(id) => resolve_season_url(id, state).await,
        MediaId::Episode(id) => resolve_episode_url(id, state).await,
        MediaId::Person(id) => resolve_person_url(id, state).await,
    }
}

async fn resolve_movie_url(id: &MovieID, state: &State) -> Result<String, String> {
    // Find the movie reference
    if let Some(reference) = find_movie_reference(id, state) {
        match &reference.details {
            MediaDetailsOption::Endpoint(endpoint) => {
                // Use the endpoint directly from server
                Ok(format!("{}{}", state.server_url, endpoint))
            }
            MediaDetailsOption::Details(TmdbDetails::Movie(details)) => {
                // Use TMDB poster path if available
                if let Some(poster_path) = &details.poster_path {
                    Ok(crate::infrastructure::api_types::get_tmdb_image_url(poster_path))
                } else {
                    Err("No poster available".to_string())
                }
            }
            _ => Err("Invalid movie details type".to_string()),
        }
    } else {
        Err("Movie not found".to_string())
    }
}

async fn resolve_series_url(id: &SeriesID, state: &State) -> Result<String, String> {
    // Find the series reference
    if let Some(reference) = find_series_reference(id, state) {
        match &reference.details {
            MediaDetailsOption::Endpoint(endpoint) => {
                // Use the endpoint directly from server
                Ok(format!("{}{}", state.server_url, endpoint))
            }
            MediaDetailsOption::Details(TmdbDetails::Series(details)) => {
                // Use TMDB poster path if available
                if let Some(poster_path) = &details.poster_path {
                    Ok(crate::infrastructure::api_types::get_tmdb_image_url(poster_path))
                } else {
                    Err("No poster available".to_string())
                }
            }
            _ => Err("Invalid series details type".to_string()),
        }
    } else {
        Err("Series not found".to_string())
    }
}

async fn resolve_season_url(id: &SeasonID, state: &State) -> Result<String, String> {
    // Find the season reference
    if let Some(reference) = find_season_reference(id, state) {
        match &reference.details {
            MediaDetailsOption::Endpoint(endpoint) => {
                // Use the endpoint directly from server
                Ok(format!("{}{}", state.server_url, endpoint))
            }
            MediaDetailsOption::Details(TmdbDetails::Season(details)) => {
                // Use cached endpoint or TMDB poster path
                if let Some(poster_path) = &details.poster_path {
                    // Check if this is already a cached endpoint (starts with /images/)
                    if poster_path.starts_with("/images/") {
                        Ok(format!("{}{}", state.server_url, poster_path))
                    } else {
                        // It's a TMDB path
                        Ok(crate::infrastructure::api_types::get_tmdb_image_url(poster_path))
                    }
                } else {
                    Err("No poster available for season".to_string())
                }
            }
            _ => Err("Invalid season details type".to_string()),
        }
    } else {
        Err("Season not found".to_string())
    }
}

async fn resolve_episode_url(id: &EpisodeID, state: &State) -> Result<String, String> {
    // Episodes use still images, not posters
    if let Some(reference) = find_episode_reference(id, state) {
        match &reference.details {
            MediaDetailsOption::Endpoint(endpoint) => {
                // Use the endpoint directly from server
                Ok(format!("{}{}", state.server_url, endpoint))
            }
            MediaDetailsOption::Details(TmdbDetails::Episode(details)) => {
                // Use cached endpoint or TMDB still path
                if let Some(still_path) = &details.still_path {
                    // Check if this is already a cached endpoint (starts with /images/)
                    if still_path.starts_with("/images/") {
                        Ok(format!("{}{}", state.server_url, still_path))
                    } else {
                        // It's a TMDB path
                        Ok(crate::infrastructure::api_types::get_tmdb_image_url(still_path))
                    }
                } else {
                    Err("No still image available".to_string())
                }
            }
            _ => Err("Invalid episode details type".to_string()),
        }
    } else {
        Err("Episode not found".to_string())
    }
}

async fn resolve_person_url(id: &ferrex_core::media::PersonID, state: &State) -> Result<String, String> {
    // Person images are served directly from the API endpoint
    // We don't need to look up references since person images are cached separately
    // The URL pattern is /images/person/{id}/profile/0
    Ok(format!("{}/images/person/{}/profile/0", state.server_url, id.as_str()))
}

// Helper functions to find references in state
fn find_movie_reference(id: &MovieID, state: &State) -> Option<MovieReference> {
    if let Ok(store) = state.media_store.read() {
        store.get(&MediaId::Movie(id.clone()))
            .and_then(|media| media.as_movie().cloned())
    } else {
        None
    }
}

fn find_series_reference(id: &SeriesID, state: &State) -> Option<SeriesReference> {
    if let Ok(store) = state.media_store.read() {
        store.get(&MediaId::Series(id.clone()))
            .and_then(|media| media.as_series().cloned())
    } else {
        None
    }
}

fn find_season_reference(id: &SeasonID, state: &State) -> Option<SeasonReference> {
    if let Ok(store) = state.media_store.read() {
        store.get(&MediaId::Season(id.clone()))
            .and_then(|media| media.as_season().cloned())
    } else {
        None
    }
}

fn find_episode_reference(id: &EpisodeID, state: &State) -> Option<EpisodeReference> {
    if let Ok(store) = state.media_store.read() {
        store.get(&MediaId::Episode(id.clone()))
            .and_then(|media| media.as_episode().cloned())
    } else {
        None
    }
}
*/
