//! Batch Metadata Fetch Helper Utility
//!
//! This module provides a reusable utility for batch fetching metadata with verification.
//! It encapsulates the complete flow from identifying items needing metadata to verifying
//! that they have full details after fetching, making it suitable for both production
//! code and benchmarking.

use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::domains::media::store::MediaStore;
use crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter;
use crate::infrastructure::api_types::{needs_details_fetch, MediaId, MediaReference};

/// Result of a batch metadata fetch operation with detailed metrics
#[derive(Debug, Clone)]
pub struct BatchFetchResult {
    /// Number of items that initially needed metadata (had Endpoint variant)
    pub items_needing_metadata: usize,
    /// Number of items that successfully got full metadata (now have Details variant)
    pub items_successfully_fetched: usize,
    /// Number of items that failed to get metadata
    pub items_failed: usize,
    /// Total time spent on the batch fetch operation
    pub fetch_duration: Duration,
    /// Whether the verification passed (all items that needed metadata now have Details)
    pub verification_passed: bool,
    /// Number of batches processed
    pub batches_processed: usize,
    /// Items per batch breakdown for analysis
    pub batch_sizes: Vec<usize>,
    /// Any error messages encountered
    pub errors: Vec<String>,
}

impl BatchFetchResult {
    /// Calculate the success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.items_needing_metadata == 0 {
            100.0
        } else {
            (self.items_successfully_fetched as f64 / self.items_needing_metadata as f64) * 100.0
        }
    }

    /// Calculate average items per batch
    pub fn average_batch_size(&self) -> f64 {
        if self.batches_processed == 0 {
            0.0
        } else {
            self.items_needing_metadata as f64 / self.batches_processed as f64
        }
    }

    /// Check if the batch fetch was completely successful
    pub fn is_fully_successful(&self) -> bool {
        self.verification_passed
            && self.items_failed == 0
            && self.items_successfully_fetched == self.items_needing_metadata
    }
}

/// Helper utility for batch fetching metadata with comprehensive verification
#[derive(Debug)]
pub struct BatchFetchHelper {
    api_service: Arc<ApiClientAdapter>,
    media_store: Arc<std::sync::RwLock<MediaStore>>,
}

impl BatchFetchHelper {
    /// Create a new batch fetch helper
    pub fn new(
        api_service: Arc<ApiClientAdapter>,
        media_store: Arc<std::sync::RwLock<MediaStore>>,
    ) -> Self {
        Self {
            api_service,
            media_store,
        }
    }

    /// Perform batch metadata fetching with comprehensive verification
    ///
    /// This method:
    /// 1. Identifies items needing metadata (have Endpoint variant)
    /// 2. Fetches metadata in batches (30 first, then 100s)
    /// 3. Updates the MediaStore with full metadata
    /// 4. Verifies that items now have Details variant
    /// 5. Returns detailed metrics for analysis
    pub async fn batch_fetch_with_verification(
        &self,
        library_id: Uuid,
        media_refs: Vec<MediaReference>,
    ) -> BatchFetchResult {
        let start_time = Instant::now();
        let mut result = BatchFetchResult {
            items_needing_metadata: 0,
            items_successfully_fetched: 0,
            items_failed: 0,
            fetch_duration: Duration::from_secs(0),
            verification_passed: false,
            batches_processed: 0,
            batch_sizes: Vec::new(),
            errors: Vec::new(),
        };

        log::info!(
            "[BatchFetchHelper] Starting batch metadata fetch for library {} with {} items",
            library_id,
            media_refs.len()
        );

        // Step 1: Identify items that need metadata
        let items_needing_metadata: Vec<(MediaId, MediaReference)> = media_refs
            .into_iter()
            .filter_map(|media_ref| {
                // Only process movies and series, skip seasons/episodes
                match media_ref.media_type() {
                    "movie" | "series" => {
                        if needs_details_fetch(media_ref.as_ref().details()) {
                            Some((media_ref.as_ref().id(), media_ref))
                        } else {
                            None
                        }
                    }
                    _ => None, // Skip seasons/episodes
                }
            })
            .collect();

        result.items_needing_metadata = items_needing_metadata.len();

        if items_needing_metadata.is_empty() {
            log::info!(
                "[BatchFetchHelper] No items need metadata for library {}",
                library_id
            );
            result.verification_passed = true;
            result.fetch_duration = start_time.elapsed();
            return result;
        }

        log::info!(
            "[BatchFetchHelper] {} items need metadata fetching for library {}",
            items_needing_metadata.len(),
            library_id
        );

        // Step 2: Create batches with the same strategy as BatchMetadataFetcher
        let media_ids: Vec<MediaId> = items_needing_metadata
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        let batches = self.create_batches(media_ids);
        result.batches_processed = batches.len();
        result.batch_sizes = batches.iter().map(|b| b.len()).collect();

        log::info!(
            "[BatchFetchHelper] Created {} batches: [{}]",
            batches.len(),
            result
                .batch_sizes
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Step 3: Process batches and fetch metadata
        let mut total_fetched = 0;
        let mut total_failed = 0;

        for (batch_index, batch) in batches.into_iter().enumerate() {
            let batch_size = batch.len();
            log::info!(
                "[BatchFetchHelper] Processing batch {} ({} items)",
                batch_index,
                batch_size
            );

            match crate::domains::media::library::fetch_media_details_batch(
                &self.api_service,
                library_id,
                batch,
            )
            .await
            {
                Ok(batch_response) => {
                    let fetched_count = batch_response.items.len();
                    let error_count = batch_response.errors.len();

                    log::info!(
                        "[BatchFetchHelper] Batch {} completed: {} items fetched, {} errors",
                        batch_index,
                        fetched_count,
                        error_count
                    );

                    // Count successful fetches
                    total_fetched += fetched_count;
                    total_failed += error_count;

                    // Log errors
                    for (media_id, error) in batch_response.errors {
                        let error_msg =
                            format!("Failed to fetch metadata for {:?}: {}", media_id, error);
                        log::warn!("[BatchFetchHelper] {}", error_msg);
                        result.errors.push(error_msg);
                    }

                    // Update MediaStore with fetched items
                    if !batch_response.items.is_empty() {
                        if let Ok(mut store) = self.media_store.write() {
                            store.bulk_upsert(batch_response.items);
                            log::debug!(
                                "[BatchFetchHelper] MediaStore updated with {} items from batch {}",
                                fetched_count,
                                batch_index
                            );
                        } else {
                            let error_msg = format!(
                                "Failed to acquire MediaStore write lock for batch {}",
                                batch_index
                            );
                            log::error!("[BatchFetchHelper] {}", error_msg);
                            result.errors.push(error_msg);
                        }
                    }
                }
                Err(e) => {
                    let error_msg = format!("Batch {} failed: {}", batch_index, e);
                    log::error!("[BatchFetchHelper] {}", error_msg);
                    result.errors.push(error_msg);
                    // Consider all items in this batch as failed
                    total_failed += batch_size;
                }
            }
        }

        result.items_successfully_fetched = total_fetched;
        result.items_failed = total_failed;

        // Step 4: Verification - check that items now have Details instead of Endpoint
        //let verification_start = Instant::now();
        let verification_result = self
            .verify_metadata_fetch(library_id, &items_needing_metadata)
            .await;
        result.verification_passed = verification_result.is_ok();

        if let Err(verification_error) = verification_result {
            result
                .errors
                .push(format!("Verification failed: {}", verification_error));
        }

        result.fetch_duration = start_time.elapsed();

        log::info!(
            "[BatchFetchHelper] Batch fetch completed for library {}: {}/{} items fetched ({}% success), verification: {}, duration: {:?}",
            library_id,
            result.items_successfully_fetched,
            result.items_needing_metadata,
            result.success_rate(),
            if result.verification_passed { "PASSED" } else { "FAILED" },
            result.fetch_duration
        );

        result
    }

    /// Create batches using the same strategy as BatchMetadataFetcher
    /// First batch: up to 30 items, subsequent batches: 100 items each
    fn create_batches(&self, media_ids: Vec<MediaId>) -> Vec<Vec<MediaId>> {
        let mut batches = Vec::new();
        let mut start = 0;

        // First batch: up to 30 items for quick initial UI response
        let first_batch_size = std::cmp::min(30, media_ids.len());
        if first_batch_size > 0 {
            batches.push(media_ids[0..first_batch_size].to_vec());
            start = first_batch_size;
        }

        // Remaining batches: 100 items each
        while start < media_ids.len() {
            let end = std::cmp::min(start + 100, media_ids.len());
            batches.push(media_ids[start..end].to_vec());
            start = end;
        }

        batches
    }

    /// Verify that metadata was actually fetched by checking MediaStore contents
    ///
    /// This checks that items which originally had Endpoint variant now have Details variant
    async fn verify_metadata_fetch(
        &self,
        library_id: Uuid,
        original_items: &[(MediaId, MediaReference)],
    ) -> Result<(), String> {
        log::debug!(
            "[BatchFetchHelper] Verifying metadata fetch for {} items",
            original_items.len()
        );

        if original_items.is_empty() {
            return Err("No items to verify".to_string());
        }

        let store = self
            .media_store
            .read()
            .map_err(|_| "Failed to acquire MediaStore read lock for verification".to_string())?;

        let mut still_endpoints = 0;
        let mut now_have_details = 0;
        let mut not_found = 0;

        for (media_id, original_ref) in original_items {
            // Find the item in the MediaStore
            let found_item = match media_id {
                MediaId::Movie(movie_id) => store
                    .get_movies(Some(library_id))
                    .iter()
                    .find(|m| &m.id == movie_id)
                    .map(|m| MediaReference::Movie((*m).clone())),
                MediaId::Series(series_id) => store
                    .get_series(Some(library_id))
                    .iter()
                    .find(|s| &s.id == series_id)
                    .map(|s| MediaReference::Series((*s).clone())),
                _ => None, // Skip other types
            };

            match found_item {
                Some(updated_ref) => {
                    // Check if it now has Details instead of Endpoint
                    if needs_details_fetch(updated_ref.as_ref().details()) {
                        still_endpoints += 1;
                        log::warn!(
                            "[BatchFetchHelper] Item {} '{}' still has Endpoint after batch fetch!",
                            updated_ref.media_type(),
                            updated_ref.as_ref().title()
                        );
                    } else {
                        now_have_details += 1;
                        log::debug!(
                            "[BatchFetchHelper] Item {} '{}' now has Details after batch fetch",
                            updated_ref.media_type(),
                            updated_ref.as_ref().title()
                        );
                    }
                }
                None => {
                    not_found += 1;
                    log::warn!(
                        "[BatchFetchHelper] Item {:?} not found in MediaStore after batch fetch",
                        media_id
                    );
                }
            }
        }

        log::info!(
            "[BatchFetchHelper] Verification results: {} have Details, {} still Endpoint, {} not found",
            now_have_details,
            still_endpoints,
            not_found
        );

        let fail_rate = still_endpoints as f64 / original_items.len() as f64;
        let not_found_rate = not_found as f64 / original_items.len() as f64;

        match (
            fail_rate > 0.05,
            not_found_rate > 0.001,
        ) {
            (true, true) => {
                    Err(format!(
                        "Verification failed: {} out of {} items still have Endpoint, exceeding the 5% threshold at {}, additionally {} items not found in MediaStore",
                        still_endpoints,
                        original_items.len(),
                        fail_rate,
                        not_found
                    ))
            }
            (true, false) => {
                    Err(format!(
                        "Verification failed: {} out of {} items Endpoint, exceeding the 5% threshold at {}",
                        still_endpoints,
                        original_items.len(),
                        fail_rate
                    ))
            }
            (false, true) => {
                    Err(format!(
                        "Verification failed: {} out of {} items not found in MediaStore",
                        not_found,
                        original_items.len()
                    ))
            }
            (false, false) => Ok(()),
        }
    }

    /// Process multiple libraries with batch fetching and verification
    /// Returns results for each library
    pub async fn batch_fetch_multiple_libraries(
        &self,
        libraries: Vec<(Uuid, Vec<MediaReference>)>,
    ) -> Vec<(Uuid, BatchFetchResult)> {
        log::info!(
            "[BatchFetchHelper] Processing {} libraries",
            libraries.len()
        );

        let mut results = Vec::new();

        for (library_id, media_refs) in libraries {
            let result = self
                .batch_fetch_with_verification(library_id, media_refs)
                .await;
            results.push((library_id, result));
        }

        // Log summary
        let total_items_needed: usize = results.iter().map(|(_, r)| r.items_needing_metadata).sum();
        let total_items_fetched: usize = results
            .iter()
            .map(|(_, r)| r.items_successfully_fetched)
            .sum();
        let all_verified = results.iter().all(|(_, r)| r.verification_passed);

        log::info!(
            "[BatchFetchHelper] All libraries processed: {}/{} items fetched ({}% success), all verified: {}",
            total_items_fetched,
            total_items_needed,
            if total_items_needed > 0 { (total_items_fetched as f64 / total_items_needed as f64) * 100.0 } else { 100.0 },
            all_verified
        );

        results
    }
}
