//! Storage implementation that uses atlas entries for custom shader images
//!
//! This module provides a storage layer that can use either iced's atlas
//! or fall back to individual textures when the atlas is full.

use crate::rendering::atlas_bridge::{AtlasBridge, AtlasEntry, AtlasTracker};
use crate::rendering::texture_pool::{TexturePool, PooledTexture};
use crate::rendering::upload_interceptor::{UploadInterceptor, UploadPriority};
use iced::advanced::graphics::core::image;
use iced::wgpu;
use iced_wgpu::image as wgpu_image;  // Now we can access the image module directly!
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Storage entry that can be either an atlas entry or a standalone texture
#[derive(Clone)]
pub enum StorageEntry {
    /// Image stored in the atlas
    Atlas(AtlasEntry),
    /// Standalone texture (fallback when atlas is full)
    Texture {
        texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        bind_group: wgpu::BindGroup,
    },
    /// Image is pending upload
    Pending,
}

/// Storage system that manages both atlas entries and individual textures
pub struct AtlasStorage {
    /// Bridge to iced's atlas
    bridge: Arc<AtlasBridge>,
    /// Upload interceptor for atlas uploads
    interceptor: Arc<Mutex<UploadInterceptor>>,
    /// Texture pool for fallback textures
    texture_pool: Arc<Mutex<TexturePool>>,
    /// Storage entries by image ID
    entries: HashMap<image::Id, StorageEntry>,
    /// Bind group layout for individual textures
    texture_layout: Option<Arc<wgpu::BindGroupLayout>>,
    /// Sampler for textures
    sampler: Option<Arc<wgpu::Sampler>>,
}

impl AtlasStorage {
    /// Create a new atlas storage system
    pub fn new(bridge: Arc<AtlasBridge>) -> Self {
        let interceptor = Arc::new(Mutex::new(UploadInterceptor::new(bridge.clone())));
        
        Self {
            bridge,
            interceptor,
            texture_pool: Arc::new(Mutex::new(TexturePool::new())),
            entries: HashMap::new(),
            texture_layout: None,
            sampler: None,
        }
    }

    /// Initialize storage with GPU resources
    pub fn init(
        &mut self,
        device: &wgpu::Device,
        texture_layout: Arc<wgpu::BindGroupLayout>,
        sampler: Arc<wgpu::Sampler>,
    ) {
        self.texture_layout = Some(texture_layout);
        self.sampler = Some(sampler);
    }

    /// Upload an image to storage (atlas or texture)
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        handle: &image::Handle,
        data: &[u8],
        width: u32,
        height: u32,
        priority: UploadPriority,
    ) -> &StorageEntry {
        let image_id = handle.id();

        // Check if already stored
        if self.entries.contains_key(&image_id) {
            return self.entries.get(&image_id).unwrap();
        }

        // Try to get from interceptor's atlas tracker first
        {
            let interceptor = self.interceptor.lock().unwrap();
            if let Some(atlas_entry) = interceptor.get_atlas_entry(image_id) {
                self.entries.insert(image_id, StorageEntry::Atlas(atlas_entry));
                return self.entries.get(&image_id).unwrap();
            }
        }

        // Queue for atlas upload
        {
            let mut interceptor = self.interceptor.lock().unwrap();
            interceptor.queue_upload(
                handle.clone(),
                data.to_vec(),
                width,
                height,
                priority,
            );
        }

        // For now, also create a fallback texture for immediate use
        // This ensures the image displays while waiting for atlas allocation
        if let Some(layout) = &self.texture_layout {
            if let Some(sampler) = &self.sampler {
                let entry = self.create_fallback_texture(
                    device,
                    queue,
                    data,
                    width,
                    height,
                    layout,
                    sampler,
                );
                
                self.entries.insert(image_id, entry);
            } else {
                self.entries.insert(image_id, StorageEntry::Pending);
            }
        } else {
            self.entries.insert(image_id, StorageEntry::Pending);
        }

        self.entries.get(&image_id).unwrap()
    }

    /// Create a fallback texture when atlas is unavailable
    fn create_fallback_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[u8],
        width: u32,
        height: u32,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> StorageEntry {
        // Get texture from pool
        let mut pool = self.texture_pool.lock().unwrap();
        let pooled = pool.get_or_create(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );

        // Upload data to texture
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &pooled.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        // Create bind group for the texture
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fallback Texture Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&pooled.view),
                },
            ],
        });

        StorageEntry::Texture {
            texture: pooled.texture,
            view: pooled.view,
            bind_group,
        }
    }

    /// Process pending atlas uploads
    pub fn process_uploads(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        cache: &mut wgpu_image::Cache,
    ) {
        let mut interceptor = self.interceptor.lock().unwrap();
        let result = interceptor.process_uploads(device, encoder, cache);

        // Update entries with newly allocated atlas entries
        // This would iterate through completed uploads and update storage entries
        log::debug!(
            "Processed {} uploads ({:.1}% complete)",
            result.uploads_completed,
            result.completion_percent()
        );
    }

    /// Get a storage entry by image ID
    pub fn get(&self, image_id: image::Id) -> Option<&StorageEntry> {
        self.entries.get(&image_id)
    }

    /// Check if an image is stored
    pub fn contains(&self, image_id: image::Id) -> bool {
        self.entries.contains_key(&image_id)
    }

    /// Update atlas entries from the interceptor
    pub fn update_atlas_entries(&mut self) {
        let interceptor = self.interceptor.lock().unwrap();
        let tracker = interceptor.tracker();
        let tracker = tracker.lock().unwrap();

        // Update any pending entries that now have atlas allocations
        for (image_id, entry) in &mut self.entries {
            if matches!(entry, StorageEntry::Texture { .. } | StorageEntry::Pending) {
                if let Some(atlas_entry) = tracker.get(*image_id) {
                    *entry = StorageEntry::Atlas(atlas_entry.clone());
                }
            }
        }
    }

    /// Clear all storage entries
    pub fn clear(&mut self) {
        self.entries.clear();
        
        let mut pool = self.texture_pool.lock().unwrap();
        pool.clear();
        
        let mut interceptor = self.interceptor.lock().unwrap();
        interceptor.clear_pending();
    }

    /// Get statistics about storage usage
    pub fn stats(&self) -> StorageStats {
        let mut atlas_count = 0;
        let mut texture_count = 0;
        let mut pending_count = 0;

        for entry in self.entries.values() {
            match entry {
                StorageEntry::Atlas(_) => atlas_count += 1,
                StorageEntry::Texture { .. } => texture_count += 1,
                StorageEntry::Pending => pending_count += 1,
            }
        }

        let interceptor = self.interceptor.lock().unwrap();
        
        StorageStats {
            total_entries: self.entries.len(),
            atlas_entries: atlas_count,
            texture_entries: texture_count,
            pending_entries: pending_count,
            pending_uploads: interceptor.pending_count(),
        }
    }

    /// Trim unused entries
    pub fn trim(&mut self) {
        // Remove pending entries that never loaded
        self.entries.retain(|_, entry| !matches!(entry, StorageEntry::Pending));

        // Trim texture pool
        let mut pool = self.texture_pool.lock().unwrap();
        pool.trim();
    }
}

/// Statistics about storage usage
#[derive(Debug, Clone)]
pub struct StorageStats {
    /// Total number of stored entries
    pub total_entries: usize,
    /// Entries using the atlas
    pub atlas_entries: usize,
    /// Entries using individual textures
    pub texture_entries: usize,
    /// Entries pending upload
    pub pending_entries: usize,
    /// Uploads queued but not processed
    pub pending_uploads: usize,
}

impl StorageStats {
    /// Get the percentage of entries using the atlas
    pub fn atlas_usage_percent(&self) -> f32 {
        if self.total_entries == 0 {
            0.0
        } else {
            (self.atlas_entries as f32 / self.total_entries as f32) * 100.0
        }
    }

    /// Check if the system is healthy (most entries in atlas)
    pub fn is_healthy(&self) -> bool {
        self.atlas_usage_percent() > 75.0 && self.pending_uploads < 10
    }
}