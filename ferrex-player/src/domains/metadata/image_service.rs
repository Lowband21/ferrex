use super::image_types::ImageRequest;
use dashmap::DashMap;
use iced::widget::image::Handle;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum LoadState {
    Loading,
    Loaded(Handle),
    Failed(String),
}

#[derive(Debug)]
pub struct ImageEntry {
    pub state: LoadState,
    pub last_accessed: std::time::Instant,
    pub loaded_at: Option<std::time::Instant>,
    pub retry_count: u8,
}

// Priority queue item for loading
#[derive(Debug, Clone)]
struct QueuedRequest {
    request: ImageRequest,
    queued_at: std::time::Instant,
}

impl PartialEq for QueuedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.request == other.request
    }
}

impl Eq for QueuedRequest {}

impl PartialOrd for QueuedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then older requests first
        match self
            .request
            .priority
            .weight()
            .cmp(&other.request.priority.weight())
        {
            Ordering::Equal => other.queued_at.cmp(&self.queued_at),
            other => other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedImageService {
    // Single cache for all images
    cache: Arc<DashMap<ImageRequest, ImageEntry>>,

    // Priority queue for pending loads
    queue: Arc<Mutex<BinaryHeap<Reverse<QueuedRequest>>>>,

    // Currently loading requests
    loading: Arc<DashMap<ImageRequest, std::time::Instant>>,

    // Channel for load requests
    load_sender: mpsc::UnboundedSender<ImageRequest>,

    // Maximum concurrent loads
    max_concurrent: usize,
}

impl UnifiedImageService {
    pub fn new(max_concurrent: usize) -> (Self, mpsc::UnboundedReceiver<ImageRequest>) {
        let (load_sender, load_receiver) = mpsc::unbounded_channel();

        let service = Self {
            cache: Arc::new(DashMap::new()),
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
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
            if matches!(entry.state, LoadState::Loaded(_)) {
                return;
            }
        }

        // Check if already loading
        if self.loading.contains_key(&request) {
            return;
        }

        // Add to queue
        let queued = QueuedRequest {
            request: request.clone(),
            queued_at: std::time::Instant::now(),
        };

        if let Ok(mut queue) = self.queue.lock() {
            // Check if already in queue
            let already_queued = queue.iter().any(|Reverse(q)| q.request == request);
            if !already_queued {
                queue.push(Reverse(queued));
                // Notify loader
                match self.load_sender.send(request.clone()) {
                    Ok(_) => log::debug!("Sent image request through channel: {:?}", request),
                    Err(e) => log::error!("Failed to send image request: {:?}", e),
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

        if let Some(mut entry) = self.cache.get_mut(request) {
            entry.state = LoadState::Failed(error);
            entry.retry_count += 1;
        } else {
            self.cache.insert(
                request.clone(),
                ImageEntry {
                    state: LoadState::Failed(error),
                    last_accessed: std::time::Instant::now(),
                    loaded_at: None,
                    retry_count: 1,
                },
            );
        }
    }

    pub fn get_next_request(&self) -> Option<ImageRequest> {
        let mut queue = self.queue.lock().ok()?;

        // Skip if we're at capacity
        if self.loading.len() >= self.max_concurrent {
            return None;
        }

        // Find next request that isn't already loading
        while let Some(Reverse(queued)) = queue.pop() {
            if !self.loading.contains_key(&queued.request) {
                return Some(queued.request);
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
