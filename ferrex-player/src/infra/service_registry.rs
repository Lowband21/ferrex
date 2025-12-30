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
    pub image_service: Arc<UnifiedImageService>,
}

impl std::fmt::Debug for ServiceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceRegistry")
            .field("image_service", &"<UnifiedImageService>")
            .finish()
    }
}

impl ServiceRegistry {
    /// Create a new service registry
    pub fn new(image_service: Arc<UnifiedImageService>) -> Self {
        Self { image_service }
    }
}

/// Initialize the global service registry
///
/// This should be called once during application startup
pub fn init_registry(image_service: Arc<UnifiedImageService>) {
    let registry = ServiceRegistry::new(image_service);

    match SERVICE_REGISTRY.write() {
        Ok(mut guard) => {
            *guard = Some(registry);
        }
        _ => {
            log::error!("Failed to initialize service registry");
        }
    }
}

/// Get the global image service handle
///
/// Returns None if the registry hasn't been initialized
pub fn get_image_service() -> Option<Arc<UnifiedImageService>> {
    SERVICE_REGISTRY.read().ok().and_then(|guard| {
        guard
            .as_ref()
            .map(|registry| registry.image_service.clone())
    })
}
