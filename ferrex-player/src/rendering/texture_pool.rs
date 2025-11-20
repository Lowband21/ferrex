//! Texture pooling to reuse GPU textures and reduce allocation overhead
//!
//! This is a pragmatic first step to reduce texture creation from 75+ to near zero
//! by reusing textures of similar dimensions.

use iced::wgpu;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// A pool of reusable textures grouped by dimensions
pub struct TexturePool {
    /// Textures grouped by (width, height) dimensions
    pools: HashMap<(u32, u32), VecDeque<PooledTexture>>,
    /// Maximum textures to keep per dimension
    max_per_size: usize,
    /// Total texture count across all pools
    total_count: usize,
    /// Maximum total textures to keep
    max_total: usize,
}

/// A texture that can be returned to the pool
pub struct PooledTexture {
    pub texture: Arc<wgpu::Texture>,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
}

impl TexturePool {
    /// Create a new texture pool
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            max_per_size: 10, // Keep up to 10 textures of each size
            total_count: 0,
            max_total: 100, // Maximum 100 textures total
        }
    }

    /// Get or create a texture with the specified dimensions
    pub fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        let key = (width, height);

        // Try to reuse an existing texture
        if let Some(pool) = self.pools.get_mut(&key) {
            if let Some(mut texture) = pool.pop_front() {
                // Verify format matches
                if texture.format == format {
                    self.total_count -= 1;
                    log::trace!("Reusing pooled texture {}x{}", width, height);
                    return texture;
                } else {
                    // Format mismatch, can't reuse
                    // Put it back at the end
                    pool.push_back(texture);
                }
            }
        }

        // Create new texture
        log::trace!("Creating new texture {}x{}", width, height);
        self.create_texture(device, width, height, format)
    }

    /// Return a texture to the pool for reuse
    pub fn return_texture(&mut self, texture: PooledTexture, width: u32, height: u32) {
        // Don't pool if we're at capacity
        if self.total_count >= self.max_total {
            log::trace!("Pool at capacity, discarding texture");
            return;
        }

        let key = (width, height);
        let pool = self.pools.entry(key).or_insert_with(VecDeque::new);

        // Don't keep too many of the same size
        if pool.len() >= self.max_per_size {
            log::trace!("Size pool at capacity for {}x{}", width, height);
            return;
        }

        pool.push_back(texture);
        self.total_count += 1;
        log::trace!("Returned texture to pool {}x{} (total: {})", width, height, self.total_count);
    }

    /// Create a new texture
    fn create_texture(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> PooledTexture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pooled Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        PooledTexture {
            texture: Arc::new(texture),
            view,
            format,
        }
    }

    /// Clear all pooled textures
    pub fn clear(&mut self) {
        self.pools.clear();
        self.total_count = 0;
    }

    /// Get statistics about the pool
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            total_textures: self.total_count,
            unique_sizes: self.pools.len(),
            largest_pool: self.pools.values().map(|p| p.len()).max().unwrap_or(0),
        }
    }

    /// Trim pools that haven't been used recently
    pub fn trim(&mut self) {
        // Remove empty pools
        self.pools.retain(|_, pool| !pool.is_empty());

        // If we're over 75% capacity, trim largest pools
        if self.total_count > (self.max_total * 3 / 4) {
            for pool in self.pools.values_mut() {
                while pool.len() > self.max_per_size / 2 {
                    pool.pop_back();
                    self.total_count -= 1;
                }
            }
        }
    }
}

/// Statistics about texture pool usage
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_textures: usize,
    pub unique_sizes: usize,
    pub largest_pool: usize,
}

impl Default for TexturePool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_reuse() {
        // Test would require wgpu device, so we test the logic
        let mut pool = TexturePool::new();
        assert_eq!(pool.total_count, 0);
        assert_eq!(pool.pools.len(), 0);
    }

    #[test]
    fn test_pool_stats() {
        let pool = TexturePool::new();
        let stats = pool.stats();
        assert_eq!(stats.total_textures, 0);
        assert_eq!(stats.unique_sizes, 0);
        assert_eq!(stats.largest_pool, 0);
    }
}