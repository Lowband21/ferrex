//! Image pipeline trait abstractions and types
//!
//! This module defines the core traits and types for the image loading pipeline,
//! following a three-tier architecture: Loader → Processor → Cache

use super::image_types::{ImageRequest, ImageSize};
use iced::widget::image::Handle;
use image::DynamicImage;
use std::sync::Arc;
use thiserror::Error;

pub mod cache;
pub mod loader;
pub mod processor;

/// Errors that can occur in the image pipeline
#[derive(Debug, Error)]
pub enum ImagePipelineError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Decode error: {0}")]
    Decode(String),

    #[error("Processing error: {0}")]
    Processing(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("URL resolution error: {0}")]
    UrlResolution(String),

    #[error("Timeout error")]
    Timeout,

    #[error("Cancelled")]
    Cancelled,
}

/// Result type for image pipeline operations
pub type Result<T> = std::result::Result<T, ImagePipelineError>;

/// Options for processing images
#[derive(Debug, Clone)]
pub struct ProcessOptions {
    /// Target size for the processed image
    pub size: ImageSize,

    /// Generate thumbnail versions
    pub generate_thumbnail: bool,

    /// Calculate dominant color
    pub calculate_dominant_color: bool,

    /// Maximum file size before re-encoding
    pub max_file_size: Option<usize>,

    /// JPEG quality for re-encoding (0-100)
    pub jpeg_quality: u8,
}

impl Default for ProcessOptions {
    fn default() -> Self {
        Self {
            size: ImageSize::Poster,
            generate_thumbnail: true,
            calculate_dominant_color: true,
            max_file_size: Some(2_000_000), // 2MB
            jpeg_quality: 85,
        }
    }
}

/// Processed image with metadata
#[derive(Debug, Clone)]
pub struct ProcessedImage {
    /// The main processed image
    pub image: Arc<DynamicImage>,

    /// Iced handle for the image
    pub handle: Handle,

    /// Thumbnail image (if generated)
    pub thumbnail: Option<Arc<DynamicImage>>,

    /// Low-quality placeholder for progressive loading
    pub lqip: Option<String>, // Base64 encoded

    /// Dominant color (if calculated)
    pub dominant_color: Option<[u8; 3]>,

    /// Original dimensions
    pub original_size: (u32, u32),

    /// Processed dimensions
    pub processed_size: (u32, u32),
}

/// Cache key for image storage
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CacheKey {
    /// The image request
    pub request: ImageRequest,

    /// Additional variant identifier (e.g., "thumb", "lqip")
    pub variant: Option<String>,
}

/// Progress information for image loading
#[derive(Debug, Clone)]
pub struct LoadProgress {
    /// Current bytes downloaded
    pub downloaded: u64,

    /// Total bytes to download (if known)
    pub total: Option<u64>,

    /// Current stage of loading
    pub stage: LoadStage,
}

/// Stages of image loading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStage {
    /// Starting the request
    Starting,

    /// Downloading from network
    Downloading,

    /// Decoding image data
    Decoding,

    /// Processing image (resize, etc.)
    Processing,

    /// Storing in cache
    Caching,

    /// Complete
    Complete,
}

/// Trait for loading images from various sources
#[async_trait::async_trait]
pub trait ImageLoader: Send + Sync {
    /// Load image data from a URL
    async fn load(&self, url: &str) -> Result<Vec<u8>>;

    /// Load image data with progress reporting
    async fn load_with_progress<F>(&self, url: &str, progress: F) -> Result<Vec<u8>>
    where
        F: Fn(LoadProgress) + Send + Sync;

    /// Check if a URL is supported by this loader
    fn supports_url(&self, url: &str) -> bool;
}

/// Trait for processing images
#[async_trait::async_trait]
pub trait ImageProcessor: Send + Sync {
    /// Process raw image data with the given options
    async fn process(&self, data: &[u8], options: ProcessOptions) -> Result<ProcessedImage>;

    /// Generate a low-quality image placeholder
    async fn generate_lqip(&self, image: &DynamicImage) -> Result<String>;

    /// Calculate the dominant color of an image
    async fn calculate_dominant_color(&self, image: &DynamicImage) -> Result<[u8; 3]>;
}

/// Trait for caching images
#[async_trait::async_trait]
pub trait ImageCache: Send + Sync {
    /// Get an image from the cache
    async fn get(&self, key: &CacheKey) -> Option<Arc<ProcessedImage>>;

    /// Insert an image into the cache
    async fn insert(&self, key: CacheKey, image: Arc<ProcessedImage>) -> Result<()>;

    /// Remove an image from the cache
    async fn remove(&self, key: &CacheKey) -> Result<()>;

    /// Clear all cached images
    async fn clear(&self) -> Result<()>;

    /// Get cache statistics
    async fn stats(&self) -> CacheStats;
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of items in memory cache
    pub memory_items: usize,

    /// Memory cache size in bytes
    pub memory_bytes: usize,

    /// Number of items in disk cache
    pub disk_items: usize,

    /// Disk cache size in bytes
    pub disk_bytes: usize,

    /// Cache hit rate (0.0-1.0)
    pub hit_rate: f32,
}

/// Complete image pipeline combining all components
#[async_trait::async_trait]
pub trait ImagePipeline: Send + Sync {
    /// Get an image, loading and processing if necessary
    async fn get_image(&self, request: ImageRequest) -> Result<Arc<ProcessedImage>>;

    /// Preload an image without returning it
    async fn preload(&self, request: ImageRequest) -> Result<()>;

    /// Cancel a pending request
    async fn cancel(&self, request: &ImageRequest) -> Result<()>;

    /// Clear the cache
    async fn clear_cache(&self) -> Result<()>;

    /// Get pipeline statistics
    async fn stats(&self) -> PipelineStats;
}

/// Pipeline statistics
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    /// Cache statistics
    pub cache: CacheStats,

    /// Number of pending requests
    pub pending_requests: usize,

    /// Number of active downloads
    pub active_downloads: usize,

    /// Number of images being processed
    pub processing_count: usize,
}
