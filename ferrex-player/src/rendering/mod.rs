//! Rendering module for optimized GPU texture operations
//!
//! This module provides pragmatic performance improvements:
//! - Texture pooling to reuse GPU textures and avoid allocation overhead
//! - Upload batching to reduce GPU synchronization from 155 to <5 operations
//! - Bridge to integrate with iced's existing atlas system
//! - Upload interceptor to route images through iced's atlas
//! - Atlas storage for managing both atlas entries and fallback textures
//! - Debug visualization for verifying optimization effectiveness

pub mod atlas_bridge;
pub mod atlas_debug;
pub mod atlas_storage;
pub mod texture_pool;
pub mod upload_batcher;
pub mod upload_interceptor;

pub use atlas_bridge::{AtlasBridge, AtlasEntry, AtlasTracker};
pub use atlas_debug::{AtlasDebugOverlay, AtlasIndicator, PerformanceMetrics};
pub use atlas_storage::{AtlasStorage, StorageEntry};
pub use texture_pool::TexturePool;
pub use upload_batcher::{UploadBatcher, UploadPriority};
pub use upload_interceptor::{UploadInterceptor, ProcessResult};