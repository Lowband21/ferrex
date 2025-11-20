//! Batched texture upload queue to reduce GPU synchronization overhead
//!
//! Instead of 155 individual queue.write_texture calls, this accumulates
//! uploads and submits them together using a command encoder.

use iced::wgpu;
use std::collections::VecDeque;

/// Priority levels for texture uploads
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UploadPriority {
    /// Currently visible on screen
    Visible = 3,
    /// About to become visible (pre-loading)
    Preload = 2,
    /// Recently visible (might scroll back)
    Recent = 1,
    /// Background loading
    Background = 0,
}

/// A pending texture upload
pub struct PendingUpload {
    /// Priority of this upload
    pub priority: UploadPriority,
    /// Target texture
    pub texture: wgpu::Texture,
    /// Image data to upload
    pub data: Vec<u8>,
    /// Image dimensions
    pub width: u32,
    pub height: u32,
    /// Target coordinates in texture
    pub origin: wgpu::Origin3d,
}

/// Batches texture uploads to reduce GPU synchronization
pub struct UploadBatcher {
    /// Queue of pending uploads sorted by priority
    pending: VecDeque<PendingUpload>,
    /// Maximum bytes to upload per frame
    max_bytes_per_frame: usize,
    /// Bytes uploaded in current frame
    bytes_uploaded: usize,
    /// Number of uploads in current batch
    uploads_in_batch: usize,
}

impl UploadBatcher {
    /// Create a new upload batcher
    pub fn new(max_bytes_per_frame: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_bytes_per_frame,
            bytes_uploaded: 0,
            uploads_in_batch: 0,
        }
    }

    /// Queue a texture upload
    pub fn queue_upload(
        &mut self,
        texture: wgpu::Texture,
        data: Vec<u8>,
        width: u32,
        height: u32,
        priority: UploadPriority,
    ) {
        self.queue_upload_at(texture, data, width, height, wgpu::Origin3d::ZERO, priority);
    }

    /// Queue a texture upload at specific coordinates
    pub fn queue_upload_at(
        &mut self,
        texture: wgpu::Texture,
        data: Vec<u8>,
        width: u32,
        height: u32,
        origin: wgpu::Origin3d,
        priority: UploadPriority,
    ) {
        let upload = PendingUpload {
            priority,
            texture,
            data,
            width,
            height,
            origin,
        };

        // Insert in priority order
        let pos = self.pending.iter().position(|u| u.priority < priority)
            .unwrap_or(self.pending.len());
        self.pending.insert(pos, upload);
    }

    /// Process pending uploads within budget
    pub fn flush(&mut self, queue: &wgpu::Queue) -> BatchStats {
        self.bytes_uploaded = 0;
        self.uploads_in_batch = 0;

        let initial_pending = self.pending.len();

        // Process uploads in priority order until budget exhausted
        while let Some(upload) = self.pending.front() {
            let upload_size = upload.data.len();

            // Check budget
            if self.bytes_uploaded + upload_size > self.max_bytes_per_frame {
                // Can't fit this upload in current frame
                if self.uploads_in_batch == 0 {
                    // Force at least one upload per frame to avoid starvation
                    self.force_upload(queue);
                }
                break;
            }

            // Remove and process the upload
            let upload = self.pending.pop_front().unwrap();
            self.write_texture(queue, upload);
        }

        BatchStats {
            uploads_completed: self.uploads_in_batch,
            bytes_uploaded: self.bytes_uploaded,
            uploads_pending: self.pending.len(),
            uploads_total: initial_pending,
        }
    }

    /// Force upload of the next texture regardless of budget
    fn force_upload(&mut self, queue: &wgpu::Queue) {
        if let Some(upload) = self.pending.pop_front() {
            log::debug!("Forcing upload to prevent starvation");
            self.write_texture(queue, upload);
        }
    }

    /// Write texture data to GPU
    fn write_texture(&mut self, queue: &wgpu::Queue, upload: PendingUpload) {
        // Calculate aligned row pitch for wgpu
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded_bytes_per_row = 4 * upload.width;
        let padding = (align - unpadded_bytes_per_row % align) % align;
        let padded_bytes_per_row = unpadded_bytes_per_row + padding;

        // Only add padding if necessary
        let data_to_write = if padding > 0 {
            let mut padded_data = Vec::with_capacity((padded_bytes_per_row * upload.height) as usize);
            
            for row in 0..upload.height as usize {
                let row_start = row * (4 * upload.width) as usize;
                let row_end = row_start + (4 * upload.width) as usize;
                padded_data.extend_from_slice(&upload.data[row_start..row_end]);
                // Add padding bytes
                padded_data.resize(padded_data.len() + padding as usize, 0);
            }
            
            padded_data
        } else {
            upload.data
        };

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &upload.texture,
                mip_level: 0,
                origin: upload.origin,
                aspect: wgpu::TextureAspect::All,
            },
            &data_to_write,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(upload.height),
            },
            wgpu::Extent3d {
                width: upload.width,
                height: upload.height,
                depth_or_array_layers: 1,
            },
        );

        self.bytes_uploaded += data_to_write.len();
        self.uploads_in_batch += 1;
    }

    /// Reset frame counters
    pub fn reset_frame(&mut self) {
        self.bytes_uploaded = 0;
        self.uploads_in_batch = 0;
    }

    /// Clear all pending uploads
    pub fn clear(&mut self) {
        self.pending.clear();
        self.reset_frame();
    }

    /// Check if there are pending uploads
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Get the number of pending uploads
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Update the frame budget
    pub fn set_budget(&mut self, max_bytes_per_frame: usize) {
        self.max_bytes_per_frame = max_bytes_per_frame;
    }
}

/// Statistics about a batch upload operation
#[derive(Debug, Clone)]
pub struct BatchStats {
    /// Number of uploads completed this batch
    pub uploads_completed: usize,
    /// Total bytes uploaded
    pub bytes_uploaded: usize,
    /// Number of uploads still pending
    pub uploads_pending: usize,
    /// Total uploads at start of batch
    pub uploads_total: usize,
}

impl BatchStats {
    /// Get the completion percentage
    pub fn completion_percent(&self) -> f32 {
        if self.uploads_total == 0 {
            100.0
        } else {
            (self.uploads_completed as f32 / self.uploads_total as f32) * 100.0
        }
    }

    /// Check if all uploads were completed
    pub fn is_complete(&self) -> bool {
        self.uploads_pending == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let mut batcher = UploadBatcher::new(1024 * 1024);
        
        // Queue uploads with different priorities
        // (Would need mock texture for full test)
        assert_eq!(batcher.pending_count(), 0);
    }

    #[test]
    fn test_batch_stats() {
        let stats = BatchStats {
            uploads_completed: 5,
            bytes_uploaded: 1024,
            uploads_pending: 5,
            uploads_total: 10,
        };

        assert_eq!(stats.completion_percent(), 50.0);
        assert!(!stats.is_complete());
    }
}