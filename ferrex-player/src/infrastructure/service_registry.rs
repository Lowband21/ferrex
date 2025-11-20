//! Global service registry for accessing services from widgets
//!
//! This module provides a way for widgets to access services without direct state dependency,
//! solving the immediate mode GUI limitation where widgets can't access state during conversion.

use crate::domains::metadata::image_service::UnifiedImageService;
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};

/// Global service registry instance
static SERVICE_REGISTRY: Lazy<Arc<RwLock<Option<ServiceRegistry>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// Registry containing all global services
#[derive(Clone)]
pub struct ServiceRegistry {
    /// The unified image service for loading and caching images
    pub image_service: ImageServiceHandle,
}

/// A thread-safe handle to the image service
#[derive(Clone)]
pub struct ImageServiceHandle {
    inner: Arc<UnifiedImageService>,
}

impl ImageServiceHandle {
    /// Create a new handle from a UnifiedImageService
    pub fn new(service: UnifiedImageService) -> Self {
        Self {
            inner: Arc::new(service),
        }
    }

    /// Get a reference to the inner service
    pub fn get(&self) -> &UnifiedImageService {
        &self.inner
    }
}

impl ServiceRegistry {
    /// Create a new service registry
    pub fn new(image_service: UnifiedImageService) -> Self {
        Self {
            image_service: ImageServiceHandle::new(image_service),
        }
    }
}

/// Initialize the global service registry
///
/// This should be called once during application startup
pub fn init_registry(image_service: UnifiedImageService) {
    let registry = ServiceRegistry::new(image_service);

    if let Ok(mut guard) = SERVICE_REGISTRY.write() {
        *guard = Some(registry);
    } else {
        log::error!("Failed to initialize service registry");
    }
}

/// Get the global image service handle
///
/// Returns None if the registry hasn't been initialized
pub fn get_image_service() -> Option<ImageServiceHandle> {
    SERVICE_REGISTRY.read().ok().and_then(|guard| {
        guard
            .as_ref()
            .map(|registry| registry.image_service.clone())
    })
}
