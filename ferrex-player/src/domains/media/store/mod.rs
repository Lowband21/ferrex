//! MediaStore modular organization
//!
//! This module re-exports the MediaStore and its trait extensions

mod batch_processor;
mod core;
pub mod querying;
pub mod sorting;
mod sorting_service;

// Re-export the main MediaStore type
pub use core::{MediaStore, MediaType, MediaChangeEvent, MediaStoreSubscriber, MediaStoreNotifier, ChangeType};

// Re-export the services
pub use batch_processor::{BatchProcessor, BatchConfig, BatchCoordinator};
pub use sorting_service::SortingService;

// Re-export the traits for convenience
pub use sorting::MediaStoreSorting;
pub use querying::MediaStoreQuerying;