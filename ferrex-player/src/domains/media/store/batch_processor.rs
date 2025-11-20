//! Batch processing service for efficient bulk operations
//! 
//! This module handles batch operations on the MediaStore,
//! including metadata updates and bulk insertions with optimized performance.

use super::core::MediaStore;
use crate::infrastructure::api_types::MediaReference;
use std::sync::Arc;
use uuid::Uuid;

/// Configuration for batch processing operations
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Whether this is an initial load from server (skip sorting)
    pub is_initial_load: bool,
    /// Whether to preserve insertion order (for pre-sorted data)
    pub preserve_order: bool,
    /// Maximum items to process in a single batch
    pub max_batch_size: usize,
    /// Whether to run on background thread
    pub use_background_thread: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            is_initial_load: false,
            preserve_order: false,
            max_batch_size: 1000,
            use_background_thread: true,
        }
    }
}

/// Service for handling batch operations on MediaStore
pub struct BatchProcessor {
    media_store: Arc<std::sync::RwLock<MediaStore>>,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(media_store: Arc<std::sync::RwLock<MediaStore>>) -> Self {
        Self { media_store }
    }
    
    /// Process a batch of media items with the given configuration
    pub async fn process_batch(
        &self,
        items: Vec<MediaReference>,
        config: BatchConfig,
    ) -> Result<usize, String> {
        if config.use_background_thread {
            self.process_batch_background(items, config).await
        } else {
            self.process_batch_immediate(items, config).await
        }
    }
    
    /// Process batch on a background thread with chunked processing
    async fn process_batch_background(
        &self,
        items: Vec<MediaReference>,
        config: BatchConfig,
    ) -> Result<usize, String> {
        let store = Arc::clone(&self.media_store);
        let item_count = items.len();
        
        // If we have a reasonable batch size, process chunks asynchronously with lock releases
        if config.max_batch_size > 0 && config.max_batch_size < 1000 {
            self.process_batch_chunked_async(items, config).await
        } else {
            // For large batch sizes or unlimited, use blocking thread
            tokio::task::spawn_blocking(move || {
                let mut store = store.write().unwrap();
                Self::process_items(&mut store, items, config)
            })
            .await
            .map_err(|e| format!("Background batch processing failed: {}", e))?
            .map(|_| item_count)
        }
    }
    
    /// Process batch in chunks with lock releases between chunks for better UI responsiveness
    async fn process_batch_chunked_async(
        &self,
        items: Vec<MediaReference>,
        config: BatchConfig,
    ) -> Result<usize, String> {
        let item_count = items.len();
        let chunk_size = config.max_batch_size;
        
        log::debug!(
            "Processing {} items in chunks of {} with lock releases",
            item_count, chunk_size
        );
        
        // Begin batch mode on the store
        {
            let mut store = self.media_store.write().unwrap();
            if config.is_initial_load {
                store.set_initial_load(true);
            }
            store.begin_batch();
        }
        
        // Process items in chunks, releasing lock between chunks
        // Convert to iterator that consumes the Vec to avoid cloning
        let mut items_iter = items.into_iter();
        let mut chunk_idx = 0;
        let total_chunks = (item_count + chunk_size - 1) / chunk_size;
        
        loop {
            // Collect next chunk
            let chunk: Vec<_> = items_iter.by_ref().take(chunk_size).collect();
            if chunk.is_empty() {
                break;
            }
            
            // Process this chunk
            {
                let mut store = self.media_store.write().unwrap();
                for item in chunk {
                    // Items are moved, not cloned
                    store.upsert(item);
                }
            }
            
            // Yield to other tasks after each chunk to improve UI responsiveness
            chunk_idx += 1;
            if chunk_idx < total_chunks {
                tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            }
        }
        
        // End batch mode and process deferred items
        {
            let mut store = self.media_store.write().unwrap();
            store.end_batch();
            if config.is_initial_load {
                store.set_initial_load(false);
            }
            log::debug!(
                "Chunked processing complete - {} items in store",
                store.len()
            );
        }
        
        Ok(item_count)
    }
    
    /// Process batch immediately on current thread
    async fn process_batch_immediate(
        &self,
        items: Vec<MediaReference>,
        config: BatchConfig,
    ) -> Result<usize, String> {
        let mut store = self.media_store.write().unwrap();
        let item_count = items.len();
        Self::process_items(&mut store, items, config)?;
        Ok(item_count)
    }
    
    /// Internal method to process items
    fn process_items(
        store: &mut MediaStore,
        items: Vec<MediaReference>,
        config: BatchConfig,
    ) -> Result<(), String> {
        // Set initial load flag if specified
        if config.is_initial_load {
            store.set_initial_load(true);
        }
        
        // Begin batch mode
        store.begin_batch();
        
        // Log what we're processing
        let mut movie_count = 0;
        let mut series_count = 0;
        let mut season_count = 0;
        let mut episode_count = 0;
        
        for item in &items {
            match item {
                MediaReference::Movie(_) => movie_count += 1,
                MediaReference::Series(_) => series_count += 1,
                MediaReference::Season(_) => season_count += 1,
                MediaReference::Episode(_) => episode_count += 1,
            }
        }
        
        log::debug!(
            "Batch processor: Processing {} movies, {} series, {} seasons, {} episodes",
            movie_count, series_count, season_count, episode_count
        );
        
        // Process items - MediaStore will handle deferring seasons/episodes as needed
        if config.max_batch_size > 0 && items.len() > config.max_batch_size {
            // Process in chunks - must clone due to chunks() borrowing
            for chunk in items.chunks(config.max_batch_size) {
                for item in chunk {
                    store.upsert(item.clone());
                }
            }
        } else {
            // Process all items at once - consume the vector to avoid cloning
            for item in items {
                store.upsert(item);
            }
        }
        
        // End batch mode - this will process any deferred items
        log::debug!("Batch processor: Ending batch mode");
        store.end_batch();
        
        // Log final count only in debug mode
        log::debug!(
            "Batch processor: After batch - total items in store: {}",
            store.len()
        );
        
        // Reset initial load flag
        if config.is_initial_load {
            store.set_initial_load(false);
        }
        
        Ok(())
    }
    
    /// Process library data in parallel batches
    pub async fn process_libraries_parallel(
        &self,
        libraries_data: Vec<(Uuid, Vec<MediaReference>)>,
        config: BatchConfig,
    ) -> Result<usize, String> {
        let mut tasks = Vec::new();
        let mut total_items = 0;
        
        for (_library_id, items) in libraries_data {
            total_items += items.len();
            let processor = Self::new(Arc::clone(&self.media_store));
            let config = config.clone();
            
            tasks.push(tokio::spawn(async move {
                processor.process_batch(items, config).await
            }));
        }
        
        // Wait for all tasks to complete
        for task in tasks {
            task.await
                .map_err(|e| format!("Task join error: {}", e))?
                .map_err(|e| format!("Batch processing error: {}", e))?;
        }
        
        Ok(total_items)
    }
}

/// Coordinator for batch operations with metadata fetching
pub struct BatchCoordinator {
    pub batch_processor: BatchProcessor,
    sorting_service: super::sorting_service::SortingService,
}

impl BatchCoordinator {
    /// Create a new batch coordinator
    pub fn new(media_store: Arc<std::sync::RwLock<MediaStore>>) -> Self {
        Self {
            batch_processor: BatchProcessor::new(Arc::clone(&media_store)),
            sorting_service: super::sorting_service::SortingService::new(media_store),
        }
    }
    
    /// Process metadata batch with optimal performance
    /// Updates items directly without batch mode to avoid conflicts
    pub async fn process_metadata_batch(
        &self,
        items: Vec<MediaReference>,
    ) -> Result<(), String> {
        let item_count = items.len();
        log::debug!("Processing metadata batch: {} items", item_count);
        
        // Process on background thread to avoid blocking UI
        let store = Arc::clone(&self.batch_processor.media_store);
        tokio::task::spawn_blocking(move || {
            let mut store = store.write().unwrap();
            
            // Direct upsert without batch mode - metadata updates are incremental
            for item in items {
                store.upsert(item);
            }
            
            log::debug!("Metadata batch processed: {} items updated", item_count);
        })
        .await
        .map_err(|e| format!("Metadata batch processing failed: {}", e))?;
        
        Ok(())
    }
    
    /// Process initial library load with pre-sorted data
    /// Uses bulk_upsert to properly handle all media
    pub async fn process_initial_load(
        &self,
        libraries_data: Vec<(Uuid, Vec<MediaReference>)>,
    ) -> Result<(), String> {
        log::info!("Processing initial load for {} libraries", libraries_data.len());
        
        // Collect all items from all libraries
        let mut all_items = Vec::new();
        for (_library_id, items) in libraries_data {
            all_items.extend(items);
        }
        
        let item_count = all_items.len();
        log::info!("Total items to load: {}", item_count);
        
        // Use bulk_upsert for consistent insertion behavior
        {
            let mut store = self.batch_processor.media_store.write().unwrap();
            // Mark as initial load to preserve server sorting
            store.set_initial_load(true);
            store.begin_batch();
            
            // Use bulk_upsert which properly handles sorted lists
            store.bulk_upsert(all_items);
            
            store.end_batch();
            store.set_initial_load(false);
        }
        
        log::info!("Initial load complete - {} items inserted", item_count);
        Ok(())
    }
}