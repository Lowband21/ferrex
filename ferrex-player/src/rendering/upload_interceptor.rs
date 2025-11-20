//! Interceptor for routing custom shader images through iced's atlas system
//!
//! This module hooks into iced's image upload pipeline to ensure our custom
//! shader images are allocated in the atlas rather than as individual textures.

use crate::rendering::atlas_bridge::{AtlasBridge, AtlasEntry, AtlasTracker};
use iced::advanced::graphics::core::image;
use iced::wgpu;
use iced_wgpu::image as wgpu_image;  // Direct access to image module
use std::sync::{Arc, Mutex};

/// Priority-based upload queue for batching texture uploads
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

/// A pending upload to be routed through the atlas
pub struct PendingUpload {
    /// Image handle
    pub handle: image::Handle,
    /// Decoded image data (RGBA8)
    pub data: Vec<u8>,
    /// Image dimensions
    pub width: u32,
    pub height: u32,
    /// Upload priority
    pub priority: UploadPriority,
}

/// Interceptor that routes custom shader uploads through iced's atlas
pub struct UploadInterceptor {
    /// Bridge to iced's atlas system
    bridge: Arc<AtlasBridge>,
    /// Tracker for atlas entries
    tracker: Arc<Mutex<AtlasTracker>>,
    /// Queue of pending uploads
    pending_uploads: Vec<PendingUpload>,
    /// Maximum uploads per frame
    max_uploads_per_frame: usize,
    /// Maximum bytes to upload per frame
    max_bytes_per_frame: usize,
}

impl UploadInterceptor {
    /// Create a new upload interceptor
    pub fn new(bridge: Arc<AtlasBridge>) -> Self {
        Self {
            bridge,
            tracker: Arc::new(Mutex::new(AtlasTracker::new())),
            pending_uploads: Vec::new(),
            max_uploads_per_frame: 10,
            max_bytes_per_frame: 4 * 1024 * 1024, // 4MB per frame
        }
    }

    /// Queue an image for upload through the atlas
    pub fn queue_upload(
        &mut self,
        handle: image::Handle,
        data: Vec<u8>,
        width: u32,
        height: u32,
        priority: UploadPriority,
    ) {
        let upload = PendingUpload {
            handle,
            data,
            width,
            height,
            priority,
        };

        // Insert in priority order
        let pos = self.pending_uploads
            .iter()
            .position(|u| u.priority < priority)
            .unwrap_or(self.pending_uploads.len());
        
        self.pending_uploads.insert(pos, upload);
    }

    /// Process pending uploads through iced's atlas system
    pub fn process_uploads(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        cache: &mut wgpu_image::Cache,
    ) -> ProcessResult {
        let mut uploads_completed = 0;
        let mut bytes_uploaded = 0;
        let initial_count = self.pending_uploads.len();

        // Process uploads in priority order within budget
        while !self.pending_uploads.is_empty() 
            && uploads_completed < self.max_uploads_per_frame
            && bytes_uploaded < self.max_bytes_per_frame 
        {
            let upload = self.pending_uploads.remove(0);
            let upload_size = upload.data.len();

            // Try to upload through iced's cache (which uses the atlas)
            if let Some(atlas_entry) = cache.upload_raster(device, encoder, &upload.handle) {
                // Convert iced's atlas entry to our format
                let our_entry = self.convert_atlas_entry(atlas_entry);
                
                // Track the atlas entry for this image
                let mut tracker = self.tracker.lock().unwrap();
                tracker.register(upload.handle.id(), our_entry);

                uploads_completed += 1;
                bytes_uploaded += upload_size;
            } else {
                // Atlas allocation failed, put back in queue
                self.pending_uploads.insert(0, upload);
                break;
            }
        }

        ProcessResult {
            uploads_completed,
            bytes_uploaded,
            uploads_pending: self.pending_uploads.len(),
            uploads_total: initial_count,
        }
    }

    /// Convert iced's atlas entry to our format
    fn convert_atlas_entry(&self, iced_entry: &wgpu_image::atlas::Entry) -> AtlasEntry {
        // Extract UV coordinates and layer from iced's entry
        match iced_entry {
            wgpu_image::atlas::Entry::Contiguous(allocation) => {
                let (x, y) = allocation.position();
                let size = allocation.size();
                let layer = allocation.layer() as u32;
                
                // Convert pixel coordinates to UV coordinates (0.0-1.0)
                const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                
                AtlasEntry {
                    uv_min: [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE],
                    uv_max: [(x + size.width) as f32 / ATLAS_SIZE, (y + size.height) as f32 / ATLAS_SIZE],
                    layer,
                    size: [size.width, size.height],
                }
            }
            wgpu_image::atlas::Entry::Fragmented { size, fragments } => {
                // For fragmented entries, we'd need a different approach
                // For now, just use the first fragment as a fallback
                if let Some(first_fragment) = fragments.first() {
                    let (x, y) = first_fragment.position;
                    let frag_size = first_fragment.allocation.size();
                    let layer = first_fragment.allocation.layer() as u32;
                    const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                    
                    AtlasEntry {
                        uv_min: [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE],
                        uv_max: [(x + frag_size.width) as f32 / ATLAS_SIZE, 
                                (y + frag_size.height) as f32 / ATLAS_SIZE],
                        layer,
                        size: [size.width, size.height],
                    }
                } else {
                    // Fallback entry
                    AtlasEntry {
                        uv_min: [0.0, 0.0],
                        uv_max: [1.0, 1.0],
                        layer: 0,
                        size: [size.width, size.height],
                    }
                }
            }
        }
    }

    /// Get an atlas entry for an image if it exists
    pub fn get_atlas_entry(&self, image_id: image::Id) -> Option<AtlasEntry> {
        let tracker = self.tracker.lock().unwrap();
        tracker.get(image_id).cloned()
    }

    /// Check if an image is in the atlas
    pub fn has_atlas_entry(&self, image_id: image::Id) -> bool {
        let tracker = self.tracker.lock().unwrap();
        tracker.get(image_id).is_some()
    }

    /// Clear all pending uploads
    pub fn clear_pending(&mut self) {
        self.pending_uploads.clear();
    }

    /// Get the number of pending uploads
    pub fn pending_count(&self) -> usize {
        self.pending_uploads.len()
    }

    /// Get the tracker for external access
    pub fn tracker(&self) -> Arc<Mutex<AtlasTracker>> {
        self.tracker.clone()
    }

    /// Update upload limits
    pub fn set_limits(&mut self, max_uploads: usize, max_bytes: usize) {
        self.max_uploads_per_frame = max_uploads;
        self.max_bytes_per_frame = max_bytes;
    }
}

/// Result of processing uploads
#[derive(Debug, Clone)]
pub struct ProcessResult {
    /// Number of uploads completed
    pub uploads_completed: usize,
    /// Total bytes uploaded
    pub bytes_uploaded: usize,
    /// Number of uploads still pending
    pub uploads_pending: usize,
    /// Total uploads at start
    pub uploads_total: usize,
}

impl ProcessResult {
    /// Check if all uploads were completed
    pub fn is_complete(&self) -> bool {
        self.uploads_pending == 0
    }

    /// Get completion percentage
    pub fn completion_percent(&self) -> f32 {
        if self.uploads_total == 0 {
            100.0
        } else {
            (self.uploads_completed as f32 / self.uploads_total as f32) * 100.0
        }
    }
}