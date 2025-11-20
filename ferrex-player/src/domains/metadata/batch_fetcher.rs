//! Simple batch metadata fetcher that processes media in batches without queuing
//!
//! This replaces the complex MetadataFetchService with a simpler approach:
//! - No individual item queuing
//! - Direct batch processing
//! - First 30 items, then 100 at a time
//! - All libraries processed in parallel

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use crate::common::messages::CrossDomainEvent;
use crate::common::messages::DomainMessage;
use crate::domains::media::store::MediaStore;
use crate::domains::metadata::batch_fetch_helper::{BatchFetchHelper, BatchFetchResult};
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use crate::infrastructure::api_types::{MediaId, MediaReference};
use iced::Task;

/// Simple batch metadata fetcher that emits events instead of storing results
#[derive(Debug)]
pub struct BatchMetadataFetcher {
    api_service: Arc<ApiClientAdapter>,
    media_store: Arc<std::sync::RwLock<MediaStore>>,
    is_complete: Arc<AtomicBool>,
    helper: BatchFetchHelper,
}

impl BatchMetadataFetcher {
    /// Create a new batch metadata fetcher
    pub fn new(api_service: Arc<ApiClientAdapter>, media_store: Arc<std::sync::RwLock<MediaStore>>) -> Self {
        let helper = BatchFetchHelper::new(Arc::clone(&api_service), Arc::clone(&media_store));
        Self {
            api_service,
            media_store,
            is_complete: Arc::new(AtomicBool::new(false)),
            helper,
        }
    }

    /// Check if metadata fetching is complete
    pub fn is_complete(&self) -> bool {
        let complete = self.is_complete.load(Ordering::Relaxed);
        log::debug!("[BatchMetadataFetcher] is_complete check: {}", complete);
        complete
    }

    /// Reset the completion state for future re-runs
    pub fn reset(&self) {
        log::info!("[BatchMetadataFetcher] Resetting is_complete flag");
        self.is_complete.store(false, Ordering::Relaxed);
    }

    /// Process a library's media references by fetching metadata in batches
    /// First batch: 30 items (for immediate display)
    /// Subsequent batches: 100 items each
    /// Returns tasks that emit CrossDomainEvent::BatchMetadataReady events
    pub async fn process_library(
        &self,
        library_id: Uuid,
        media_refs: Vec<MediaReference>,
    ) -> Vec<Task<DomainMessage>> {
        log::info!(
            "[BatchMetadataFetcher] Processing {} media references for library {}",
            media_refs.len(),
            library_id
        );

        // Filter items that need metadata (details == Endpoint variant)
        let items_needing_metadata: Vec<MediaId> = media_refs
            .into_iter()
            .filter_map(|media_ref| {
                // Only process movies and series, skip seasons/episodes
                match media_ref.media_type() {
                    "movie" | "series" => {
                        if crate::infrastructure::api_types::needs_details_fetch(media_ref.as_ref().details()) {
                            Some(media_ref.as_ref().id())
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
            return vec![Task::none()];
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

        // Process all batches and create event emission tasks
        let mut event_tasks = Vec::new();
        for (batch_index, batch) in batches.into_iter().enumerate() {
            let api_service = Arc::clone(&self.api_service);

            let async_task = tokio::spawn(async move {
                log::info!(
                    "Processing batch {} ({} items) for library {}",
                    batch_index,
                    batch.len(),
                    library_id
                );

                match crate::domains::media::library::fetch_media_details_batch(
                    &api_service,
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

                        // Log errors
                        for (media_id, error) in batch_response.errors {
                            log::warn!("Failed to fetch metadata for {:?}: {}", media_id, error);
                        }

                        // DEBUG: Check what we actually received
                        let mut still_endpoints = 0;
                        let mut has_details = 0;
                        for item in &batch_response.items {
                            // Only check movies and series for metadata status
                            if matches!(item.media_type(), "movie" | "series") {
                                let title = item.as_ref().title();
                                if crate::infrastructure::api_types::needs_details_fetch(item.as_ref().details()) {
                                    still_endpoints += 1;
                                    log::warn!("BATCH ISSUE: {} '{}' still has Endpoint after batch fetch!", 
                                        item.media_type(), title);
                                } else {
                                    has_details += 1;
                                    log::debug!("{} '{}' has full Details after batch fetch", 
                                        item.media_type(), title);
                                }
                            }
                        }
                        
                        if still_endpoints > 0 {
                            log::error!(
                                "BATCH FETCH PROBLEM: {} items still have Endpoint (not Details) after batch fetch! {} have Details.",
                                still_endpoints, has_details
                            );
                        } else {
                            log::info!("Batch fetch successful: All {} items have full Details", has_details);
                        }

                        // Return the batch results for event emission
                        Some(batch_response.items)
                    }
                    Err(e) => {
                        log::error!(
                            "Batch {} for library {} failed: {}",
                            batch_index,
                            library_id,
                            e
                        );
                        None
                    }
                }
            });

            // Create a task that waits for the async work and emits events
            let event_task = Task::perform(async_task, |join_result| match join_result {
                Ok(Some(items)) if !items.is_empty() => {
                    log::debug!(
                        "Emitting BatchMetadataReady event for {} items",
                        items.len()
                    );
                    DomainMessage::Event(CrossDomainEvent::BatchMetadataReady(items))
                }
                Ok(Some(_)) => {
                    log::debug!("Batch completed but no items to emit");
                    DomainMessage::NoOp
                }
                Ok(None) => {
                    log::debug!("Batch failed, no event to emit");
                    DomainMessage::NoOp
                }
                Err(e) => {
                    log::error!("Batch task join failed: {}", e);
                    DomainMessage::NoOp
                }
            });
            event_tasks.push(event_task);
        }

        log::info!(
            "Library {} processing initiated: {} batch event tasks created",
            library_id,
            event_tasks.len()
        );

        event_tasks
    }

    /// Process multiple libraries in parallel
    /// Returns a task that batches all event emission tasks
    pub async fn process_libraries(
        self: Arc<Self>,
        libraries: Vec<(Uuid, Vec<MediaReference>)>,
    ) -> Task<DomainMessage> {
        log::info!("Processing {} libraries in parallel", libraries.len());

        let mut all_tasks = Vec::new();
        for (library_id, media_refs) in libraries {
            let library_tasks = self.process_library(library_id, media_refs).await;
            all_tasks.extend(library_tasks);
        }

        // Mark processing as complete
        self.is_complete.store(true, Ordering::Relaxed);
        log::info!(
            "[BatchMetadataFetcher] Created {} total batch tasks across all libraries",
            all_tasks.len()
        );

        // Return a batched task of all event emissions
        Task::batch(all_tasks)
    }

    /// Process multiple libraries directly without returning Iced tasks
    /// This prevents the infinite loop caused by re-executable tasks
    pub async fn process_libraries_direct(
        self: Arc<Self>,
        libraries: Vec<(Uuid, Vec<MediaReference>)>,
    ) {
        log::info!("Processing {} libraries directly (no tasks)", libraries.len());
        
        // NOTE: We do NOT use batch mode here anymore!
        // Batch mode should only be used for the initial reference load,
        // not for metadata fetching which happens incrementally.
        // This allows ViewModels to show data immediately.

        // Process each library's metadata fetching
        for (library_id, media_refs) in libraries {
            log::info!(
                "[BatchMetadataFetcher] Processing {} media references for library {}",
                media_refs.len(),
                library_id
            );

            // Filter items that need metadata (details == Endpoint variant)
            let items_needing_metadata: Vec<MediaId> = media_refs
                .into_iter()
                .filter_map(|media_ref| {
                    // Only process movies and series, skip seasons/episodes
                    match media_ref.media_type() {
                        "movie" | "series" => {
                            if crate::infrastructure::api_types::needs_details_fetch(media_ref.as_ref().details()) {
                                Some(media_ref.as_ref().id())
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
                continue;
            }

            log::info!(
                "{} items need metadata for library {}",
                items_needing_metadata.len(),
                library_id
            );

            // Create batches: first 30, then 100s
            let mut batches = Vec::new();
            let mut start = 0;

            // First batch: up to 30 items for quick initial UI response
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

            // Process all batches directly without creating tasks
            for (batch_index, batch) in batches.into_iter().enumerate() {
                let api_service = Arc::clone(&self.api_service);

                log::info!(
                    "Processing batch {} ({} items) for library {}",
                    batch_index,
                    batch.len(),
                    library_id
                );

                match crate::domains::media::library::fetch_media_details_batch(
                    &api_service,
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

                        // Log errors (but only in debug mode to reduce noise)
                        if log::log_enabled!(log::Level::Debug) {
                            for (media_id, error) in batch_response.errors {
                                log::debug!("Failed to fetch metadata for {:?}: {}", media_id, error);
                            }
                        }

                        // Update MediaStore directly with the fetched metadata
                        // Since we're in batch mode, this won't trigger ViewModels refresh
                        if !batch_response.items.is_empty() {
                            if let Ok(mut store) = self.media_store.write() {
                                // Use bulk_upsert for better performance
                                store.bulk_upsert(batch_response.items);
                                log::debug!(
                                    "MediaStore updated with batch {} for library {}",
                                    batch_index,
                                    library_id
                                );
                            } else {
                                log::error!("Failed to acquire MediaStore write lock for batch {}", batch_index);
                            }
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
            }
        }

        // No need to end batch mode since we're not using it for metadata fetching
        // ViewModels will update incrementally as metadata arrives

        // Mark processing as complete
        self.is_complete.store(true, Ordering::Relaxed);
        log::info!("[BatchMetadataFetcher] All libraries processed - ViewModels will refresh once");
    }

    /// Process multiple libraries with verification results for benchmarking
    /// This provides detailed metrics about the batch fetching process
    pub async fn process_libraries_with_verification(
        self: Arc<Self>,
        libraries: Vec<(Uuid, Vec<MediaReference>)>,
    ) -> Vec<(Uuid, BatchFetchResult)> {
        log::info!("[BatchMetadataFetcher] Processing {} libraries with verification", libraries.len());
        
        let results = self.helper.batch_fetch_multiple_libraries(libraries).await;
        
        // Mark processing as complete
        self.is_complete.store(true, Ordering::Relaxed);
        
        log::info!("[BatchMetadataFetcher] All libraries processed with verification - {} results", results.len());
        
        results
    }
}
