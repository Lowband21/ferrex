use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Background poster monitoring service that tracks what's visible and fetches metadata
#[derive(Debug, Clone)]
pub struct PosterMonitor {
    /// Currently visible media indices (movies)
    visible_movies: Arc<Mutex<Vec<usize>>>,
    /// Currently visible TV show indices
    visible_tv_shows: Arc<Mutex<Vec<usize>>>,
    /// Queue of media IDs that need poster checking
    check_queue: Arc<Mutex<VecDeque<String>>>,
    /// Set of media IDs currently being checked
    checking: Arc<Mutex<HashSet<String>>>,
}

impl PosterMonitor {
    pub fn new() -> Self {
        Self {
            visible_movies: Arc::new(Mutex::new(Vec::new())),
            visible_tv_shows: Arc::new(Mutex::new(Vec::new())),
            check_queue: Arc::new(Mutex::new(VecDeque::new())),
            checking: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Update what's currently visible - called from scroll handlers
    pub fn update_visible_movies(&self, indices: Vec<usize>) {
        let visible = self.visible_movies.clone();
        tokio::spawn(async move {
            *visible.lock().await = indices;
        });
    }

    pub fn update_visible_tv_shows(&self, indices: Vec<usize>) {
        let visible = self.visible_tv_shows.clone();
        tokio::spawn(async move {
            *visible.lock().await = indices;
        });
    }

    /// Get next batch of media IDs to check (prioritizes visible items)
    pub async fn get_next_batch(&self, max_items: usize) -> Vec<String> {
        let mut queue = self.check_queue.lock().await;
        let mut checking = self.checking.lock().await;

        let mut batch = Vec::new();
        let mut count = 0;

        // Take items from queue, skipping those already being checked
        while count < max_items && !queue.is_empty() {
            if let Some(media_id) = queue.pop_front() {
                if !checking.contains(&media_id) {
                    checking.insert(media_id.clone());
                    batch.push(media_id);
                    count += 1;
                } else {
                    // Put it back at the end if already checking
                    queue.push_back(media_id);
                }
            }
        }

        batch
    }

    /// Mark items as completed checking
    pub async fn mark_completed(&self, media_ids: &[String]) {
        let mut checking = self.checking.lock().await;
        for id in media_ids {
            checking.remove(id);
        }
    }

    /// Add items to check queue (called from background task)
    pub async fn queue_for_checking(&self, media_ids: Vec<String>) {
        let mut queue = self.check_queue.lock().await;
        for id in media_ids {
            if !queue.contains(&id) {
                queue.push_back(id);
            }
        }
    }

    /// Clear specific items from the queue (e.g., when they're marked as failed)
    pub async fn remove_from_queue(&self, media_ids: &[String]) {
        let mut queue = self.check_queue.lock().await;
        queue.retain(|id| !media_ids.contains(id));
    }

    /// Get current visible indices for priority checking
    pub async fn get_visible_indices(&self) -> (Vec<usize>, Vec<usize>) {
        let movies = self.visible_movies.lock().await.clone();
        let tv_shows = self.visible_tv_shows.lock().await.clone();
        (movies, tv_shows)
    }
}
