//! Batch state registration for custom primitives
//!
//! This module handles registering our custom primitive batch states with the iced renderer.

use std::any::TypeId;
use iced_wgpu::primitive::Storage;

/// Register all custom primitive batch states with the renderer
pub fn register_batch_states(
    storage: &mut Storage,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) {
    use crate::domains::ui::widgets::rounded_image_shader::{
        RoundedImagePrimitive,
        rounded_image_batch_state::RoundedImageBatchState,
    };
    
    // Register RoundedImagePrimitive for batching
    let type_id = TypeId::of::<RoundedImagePrimitive>();
    let batch_state = RoundedImageBatchState::new(device, format);
    
    storage.store_batch_state(type_id, Box::new(batch_state));
    
    log::info!("Registered RoundedImagePrimitive for batched rendering");
    
    // Future: Register other custom primitives here
    // e.g., BackgroundPrimitive, VideoPrimitive, etc.
}

/// Check if batching is enabled for a specific primitive type
pub fn is_batching_enabled<T: 'static>() -> bool {
    // This could be controlled by a feature flag or runtime config
    // For now, always enable batching for registered types
    true
}

/// Configuration for batch rendering
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum instances per batch (0 = unlimited)
    pub max_batch_size: usize,
    /// Enable batching for rounded images
    pub batch_rounded_images: bool,
    /// Enable batching for background shaders
    pub batch_backgrounds: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 0, // Unlimited
            batch_rounded_images: true,
            batch_backgrounds: false, // Not implemented yet
        }
    }
}