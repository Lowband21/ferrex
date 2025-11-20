//! Bridge between iced's atlas system and custom shader widgets
//!
//! This module provides an interface to use iced's existing texture atlas
//! infrastructure with custom shaders like our rounded_image_shader.

use iced::advanced::graphics::core::image;
use iced::wgpu;
use std::sync::Arc;

/// Represents a region in iced's texture atlas
#[derive(Debug, Clone)]
pub struct AtlasEntry {
    /// UV coordinates in the atlas (0.0-1.0)
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Which texture array layer this entry is in
    pub layer: u32,
    /// Size of the region in pixels
    pub size: [u32; 2],
}

/// Bridge to iced's atlas system for custom shaders
pub struct AtlasBridge {
    /// Reference to iced's atlas texture array
    atlas_texture: Option<Arc<wgpu::Texture>>,
    /// Bind group for the atlas texture array
    atlas_bind_group: Option<wgpu::BindGroup>,
    /// Layout for creating compatible bind groups
    bind_group_layout: Option<Arc<wgpu::BindGroupLayout>>,
}

impl AtlasBridge {
    /// Create a new atlas bridge
    pub fn new() -> Self {
        Self {
            atlas_texture: None,
            atlas_bind_group: None,
            bind_group_layout: None,
        }
    }

    /// Initialize the bridge with iced's atlas resources
    pub fn init(
        &mut self,
        atlas_texture: Arc<wgpu::Texture>,
        bind_group: wgpu::BindGroup,
        layout: Arc<wgpu::BindGroupLayout>,
    ) {
        self.atlas_texture = Some(atlas_texture);
        self.atlas_bind_group = Some(bind_group);
        self.bind_group_layout = Some(layout);
    }

    /// Check if the bridge is initialized
    pub fn is_initialized(&self) -> bool {
        self.atlas_texture.is_some()
    }

    /// Get the atlas texture for binding
    pub fn texture(&self) -> Option<&Arc<wgpu::Texture>> {
        self.atlas_texture.as_ref()
    }

    /// Get the atlas bind group
    pub fn bind_group(&self) -> Option<&wgpu::BindGroup> {
        self.atlas_bind_group.as_ref()
    }

    /// Convert pixel coordinates to UV coordinates
    pub fn pixels_to_uv(x: u32, y: u32, atlas_size: u32) -> [f32; 2] {
        [
            x as f32 / atlas_size as f32,
            y as f32 / atlas_size as f32,
        ]
    }

    /// Convert an iced atlas allocation to our AtlasEntry format
    pub fn create_entry(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        layer: u32,
        atlas_size: u32,
    ) -> AtlasEntry {
        let uv_min = Self::pixels_to_uv(x, y, atlas_size);
        let uv_max = Self::pixels_to_uv(x + width, y + height, atlas_size);

        AtlasEntry {
            uv_min,
            uv_max,
            layer,
            size: [width, height],
        }
    }

    /// Create a bind group layout compatible with texture arrays
    pub fn create_texture_array_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Atlas Texture Array Layout"),
            entries: &[
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Texture array
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        })
    }
}

/// Tracker for managing atlas entries for images
pub struct AtlasTracker {
    /// Map from image ID to atlas entry
    entries: std::collections::HashMap<image::Id, AtlasEntry>,
}

impl AtlasTracker {
    /// Create a new atlas tracker
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
        }
    }

    /// Register an atlas entry for an image
    pub fn register(&mut self, image_id: image::Id, entry: AtlasEntry) {
        self.entries.insert(image_id, entry);
    }

    /// Get an atlas entry for an image
    pub fn get(&self, image_id: image::Id) -> Option<&AtlasEntry> {
        self.entries.get(&image_id)
    }

    /// Remove an atlas entry
    pub fn remove(&mut self, image_id: image::Id) -> Option<AtlasEntry> {
        self.entries.remove(&image_id)
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the number of tracked entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AtlasBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AtlasTracker {
    fn default() -> Self {
        Self::new()
    }
}