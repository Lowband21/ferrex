//! Connector to use iced's atlas system with custom shaders
//!
//! This module provides a simplified interface to use iced's atlas
//! with our custom rounded_image_shader. Instead of trying to bypass
//! iced's image loading, we work WITH the system.

use iced::advanced::graphics::core::image;
use iced::wgpu;
use iced_wgpu::image as wgpu_image;
use std::sync::Arc;

/// Connection to iced's atlas for custom shaders
pub struct AtlasConnector {
    /// The atlas texture from iced
    atlas_texture: Option<Arc<wgpu::Texture>>,
    /// View of the atlas texture array
    atlas_view: Option<wgpu::TextureView>,
    /// Bind group layout for texture arrays
    array_layout: Option<Arc<wgpu::BindGroupLayout>>,
    /// Sampler for the atlas
    sampler: Option<Arc<wgpu::Sampler>>,
}

impl AtlasConnector {
    /// Create a new atlas connector
    pub fn new() -> Self {
        Self {
            atlas_texture: None,
            atlas_view: None,
            array_layout: None,
            sampler: None,
        }
    }

    /// Initialize with GPU resources
    pub fn init(&mut self, device: &wgpu::Device) {
        // Create bind group layout for texture arrays
        let array_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Atlas Array Layout"),
            entries: &[
                // Sampler at binding 0
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Texture array at binding 1
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
        });

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        self.array_layout = Some(Arc::new(array_layout));
        self.sampler = Some(Arc::new(sampler));
    }

    /// Request an image upload through iced's cache
    pub fn request_upload(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        cache: &mut wgpu_image::Cache,
        handle: &image::Handle,
    ) -> Option<AtlasEntryInfo> {
        // Use iced's cache to upload the image to the atlas
        let entry = cache.upload_raster(device, encoder, handle)?;
        
        // Extract information from the atlas entry
        Some(self.extract_entry_info(entry))
    }

    /// Extract information from an atlas entry
    fn extract_entry_info(&self, entry: &wgpu_image::atlas::Entry) -> AtlasEntryInfo {
        match entry {
            wgpu_image::atlas::Entry::Contiguous(allocation) => {
                let (x, y) = allocation.position();
                let size = allocation.size();
                let layer = allocation.layer() as u32;
                
                // Convert to UV coordinates
                const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                
                AtlasEntryInfo {
                    uv_min: [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE],
                    uv_max: [(x + size.width) as f32 / ATLAS_SIZE, 
                            (y + size.height) as f32 / ATLAS_SIZE],
                    layer,
                    size: [size.width, size.height],
                }
            }
            wgpu_image::atlas::Entry::Fragmented { size, fragments } => {
                // For fragmented entries, we need special handling
                // For now, just use the first fragment
                if let Some(first) = fragments.first() {
                    let (x, y) = first.position;
                    let layer = first.allocation.layer() as u32;
                    const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                    
                    AtlasEntryInfo {
                        uv_min: [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE],
                        uv_max: [(x + size.width) as f32 / ATLAS_SIZE,
                                (y + size.height) as f32 / ATLAS_SIZE],
                        layer,
                        size: [size.width, size.height],
                    }
                } else {
                    // Fallback for empty fragmented entry
                    AtlasEntryInfo {
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        layer: 0,
                        size: [1, 1],
                    }
                }
            }
        }
    }

    /// Connect to iced's atlas texture
    pub fn connect_to_atlas(&mut self, atlas: &wgpu_image::atlas::Atlas) {
        // Get the texture view from the atlas
        // This requires access to the atlas's internal texture
        // We'd need to expose this from iced's atlas
        
        // For now, we'll need to modify iced to expose the texture
        // or find another way to access it
    }

    /// Create a bind group for the atlas texture array
    pub fn create_bind_group(
        &self,
        device: &wgpu::Device,
        atlas_view: &wgpu::TextureView,
    ) -> Option<wgpu::BindGroup> {
        let layout = self.array_layout.as_ref()?;
        let sampler = self.sampler.as_ref()?;

        Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Atlas Array Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(atlas_view),
                },
            ],
        }))
    }

    /// Check if connected to atlas
    pub fn is_connected(&self) -> bool {
        self.atlas_texture.is_some()
    }
}

/// Information extracted from an atlas entry
#[derive(Debug, Clone)]
pub struct AtlasEntryInfo {
    /// UV coordinates in the atlas (0.0-1.0)
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Which texture array layer
    pub layer: u32,
    /// Size in pixels
    pub size: [u32; 2],
}

impl AtlasEntryInfo {
    /// Get UV width
    pub fn uv_width(&self) -> f32 {
        self.uv_max[0] - self.uv_min[0]
    }

    /// Get UV height
    pub fn uv_height(&self) -> f32 {
        self.uv_max[1] - self.uv_min[1]
    }

    /// Check if this is a valid entry
    pub fn is_valid(&self) -> bool {
        self.size[0] > 0 && self.size[1] > 0
    }
}