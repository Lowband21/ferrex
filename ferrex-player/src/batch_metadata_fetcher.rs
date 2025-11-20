//! Simple batch metadata fetcher that processes media in batches without queuing
//!
//! This replaces the complex MetadataFetchService with a simpler approach:
//! - No individual item queuing
//! - Direct batch processing
//! - First 30 items, then 100 at a time
//! - All libraries processed in parallel

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::api_client::ApiClient;
use crate::api_types::{MediaId, MediaReference};

/// Simple batch metadata fetcher
#[derive(Debug)]
pub struct BatchMetadataFetcher {
    api_client: Arc<ApiClient>,
    pending_updates: Arc<Mutex<Vec<MediaReference>>>,
    is_complete: Arc<AtomicBool>,
}

impl BatchMetadataFetcher {
    /// Create a new batch metadata fetcher
    pub fn new(api_client: Arc<ApiClient>) -> Self {
        Self {
            api_client,
            pending_updates: Arc::new(Mutex::new(Vec::new())),
            is_complete: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get pending updates and clear the internal list
    pub async fn get_pending_updates(&self) -> Vec<MediaReference> {
        let mut updates = self.pending_updates.lock().await;
        std::mem::take(&mut *updates)
    }

    /// Check if metadata fetching is complete
    pub fn is_complete(&self) -> bool {
        self.is_complete.load(Ordering::Relaxed)
    }

    /// Reset the completion state for future re-runs
    pub fn reset(&self) {
        self.is_complete.store(false, Ordering::Relaxed);
    }

    /// Process a library's media references by fetching metadata in batches
    /// First batch: 30 items (for immediate display)
    /// Subsequent batches: 100 items each
    pub async fn process_library(&self, library_id: Uuid, media_refs: Vec<MediaReference>) {
        log::info!(
            "[BatchMetadataFetcher] Processing {} media references for library {}",
            media_refs.len(),
            library_id
        );

        // Filter items that need metadata (details == Endpoint variant)
        let items_needing_metadata: Vec<MediaId> = media_refs
            .into_iter()
            .filter_map(|media_ref| {
                match &media_ref {
                    MediaReference::Movie(movie) => {
                        if crate::api_types::needs_details_fetch(&movie.details) {
                            Some(MediaId::Movie(movie.id.clone()))
                        } else {
                            None
                        }
                    }
                    MediaReference::Series(series) => {
                        if crate::api_types::needs_details_fetch(&series.details) {
                            Some(MediaId::Series(series.id.clone()))
                        } else {
                            None
                        }
                    }
                    _ => None, // Skip seasons/episodes
                }
            })
            .collect();

        if items_needing_metadata.is_empty() {
            log::info!("No items need metadata for library {}", library_id);
            return;
        }

        log::info!(
            "{} items need metadata for library {}",
            items_needing_metadata.len(),
            library_id
        );

        // Create batches: first 30, then 100s
        let mut batches = Vec::new();
        let mut start = 0;

        // First batch: up to 30 items
        let first_batch_size = std::cmp::min(30, items_needing_metadata.len());
        if first_batch_size > 0 {
            batches.push(items_needing_metadata[0..first_batch_size].to_vec());
            start = first_batch_size;
        }

        // Remaining batches: 100 items each
        while start < items_needing_metadata.len() {
            let end = std::cmp::min(start + 100, items_needing_metadata.len());
            batches.push(items_needing_metadata[start..end].to_vec());
            start = end;
        }

        log::info!(
            "Created {} batches for library {}: [{}]",
            batches.len(),
            library_id,
            batches
                .iter()
                .map(|b| b.len().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Process all batches concurrently
        let mut tasks = Vec::new();
        for (batch_index, batch) in batches.into_iter().enumerate() {
            let api_client = Arc::clone(&self.api_client);
            let pending_updates = Arc::clone(&self.pending_updates);

            let task = tokio::spawn(async move {
                log::info!(
                    "Processing batch {} ({} items) for library {}",
                    batch_index,
                    batch.len(),
                    library_id
                );

                match crate::media_library::fetch_media_details_batch(
                    &api_client,
                    library_id,
                    batch,
                )
                .await
                {
                    Ok(batch_response) => {
                        log::info!(
                            "Batch {} for library {} completed: {} items fetched, {} errors",
                            batch_index,
                            library_id,
                            batch_response.items.len(),
                            batch_response.errors.len()
                        );

                        // Store pending updates for UI processing
                        let mut updates = pending_updates.lock().await;
                        let items_count = batch_response.items.len();

                        for media_ref in batch_response.items {
                            // Store update for later retrieval
                            updates.push(media_ref);
                        }

                        log::debug!(
                            "Batch {} added {} items to pending updates (total pending: {})",
                            batch_index,
                            items_count,
                            updates.len()
                        );

                        // Log errors
                        for (media_id, error) in batch_response.errors {
                            log::warn!("Failed to fetch metadata for {:?}: {}", media_id, error);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Batch {} for library {} failed: {}",
                            batch_index,
                            library_id,
                            e
                        );
                    }
                }
            });

            tasks.push(task);
        }

        // Wait for all batches to complete
        let results = futures::future::join_all(tasks).await;
        let successful = results.iter().filter(|r| r.is_ok()).count();
        log::info!(
            "Library {} processing complete: {}/{} batches successful",
            library_id,
            successful,
            results.len()
        );
    }

    /// Process multiple libraries in parallel
    pub async fn process_libraries(self: Arc<Self>, libraries: Vec<(Uuid, Vec<MediaReference>)>) {
        log::info!("Processing {} libraries in parallel", libraries.len());

        let mut tasks = Vec::new();
        for (library_id, media_refs) in libraries {
            let fetcher = Arc::clone(&self);
            let task = tokio::spawn(async move {
                fetcher.process_library(library_id, media_refs).await;
            });
            tasks.push(task);
        }

        let results = futures::future::join_all(tasks).await;
        let successful = results.iter().filter(|r| r.is_ok()).count();
        log::info!(
            "[BatchMetadataFetcher] All libraries processed: {}/{} successful",
            successful,
            results.len()
        );

        // Mark processing as complete
        self.is_complete.store(true, Ordering::Relaxed);
        log::info!(
            "[BatchMetadataFetcher] Marked as complete - UI loading state should transition"
        );
    }
}
