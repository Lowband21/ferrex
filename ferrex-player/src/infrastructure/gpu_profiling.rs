//! GPU profiling infrastructure using Tracy
//!
//! This module provides GPU profiling capabilities for wgpu rendering,
//! allowing us to track actual GPU execution time and identify bottlenecks.

use iced::wgpu;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(feature = "profile-with-tracy")]
use tracy_client::{GpuContext, GpuContextType, GpuSpan};

/// Global GPU profiling context
lazy_static::lazy_static! {
    static ref GPU_PROFILER: Arc<Mutex<Option<GpuProfiler>>> = Arc::new(Mutex::new(None));
}

/// Pending GPU span that needs timestamp resolution
#[cfg(feature = "profile-with-tracy")]
struct PendingGpuSpan {
    span: GpuSpan,
    start_query_index: u32,
    end_query_index: u32,
    submitted: bool,
}

/// GPU profiling state
pub struct GpuProfiler {
    #[cfg(feature = "profile-with-tracy")]
    context: GpuContext,
    
    #[cfg(feature = "profile-with-tracy")]
    pending_spans: HashMap<u64, Vec<PendingGpuSpan>>,
    
    /// Query set for timestamp queries
    query_set: Option<wgpu::QuerySet>,
    
    /// Buffer to resolve query results into
    query_buffer: Option<wgpu::Buffer>,
    
    /// Staging buffer for reading back timestamps
    staging_buffer: Option<wgpu::Buffer>,
    
    /// Current query index
    next_query_index: u32,
    
    /// Maximum number of queries
    max_queries: u32,
    
    /// GPU timestamp period in nanoseconds
    timestamp_period: f32,
    
    /// Whether timestamp queries are supported
    timestamps_enabled: bool,
}

impl GpuProfiler {
    /// Create a new GPU profiler
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        // Check if timestamp queries are supported
        let timestamps_enabled = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        
        if !timestamps_enabled {
            log::warn!("GPU timestamp queries not supported - GPU profiling disabled");
            // We can't create a valid Tracy context without timestamps, so we panic if Tracy is enabled
            #[cfg(feature = "profile-with-tracy")]
            {
                panic!("Tracy GPU profiling enabled but GPU timestamp queries not supported");
            }
            
            #[cfg(not(feature = "profile-with-tracy"))]
            return Self {
                query_set: None,
                query_buffer: None,
                staging_buffer: None,
                next_query_index: 0,
                max_queries: 0,
                timestamp_period: 1.0,
                timestamps_enabled: false,
            };
        }
        
        log::info!("GPU timestamp queries supported - enabling GPU profiling");
        
        // Get timestamp period from queue
        let timestamp_period = queue.get_timestamp_period();
        log::info!("GPU timestamp period: {} ns", timestamp_period);
        
        // Create query set for timestamps
        let max_queries = 256; // Should be enough for most frames
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("GPU Profiling Query Set"),
            ty: wgpu::QueryType::Timestamp,
            count: max_queries,
        });
        
        // Create buffer to resolve queries into
        let query_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU Profiling Query Buffer"),
            size: (max_queries * 8) as u64, // 8 bytes per timestamp
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        
        // Create staging buffer for reading back timestamps
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU Profiling Staging Buffer"),
            size: (max_queries * 8) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        
        #[cfg(feature = "profile-with-tracy")]
        {
            // Get initial GPU timestamp
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("GPU Profiling Init"),
            });
            encoder.write_timestamp(&query_set, 0);
            encoder.resolve_query_set(&query_set, 0..1, &query_buffer, 0);
            encoder.copy_buffer_to_buffer(&query_buffer, 0, &staging_buffer, 0, 8);
            queue.submit(Some(encoder.finish()));
            
            // Wait for initial timestamp
            let buffer_slice = staging_buffer.slice(..8);
            let (sender, receiver) = std::sync::mpsc::channel();
            buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
            
            device.poll(wgpu::Maintain::Wait);
            receiver.recv().unwrap().unwrap();
            
            let data = buffer_slice.get_mapped_range();
            let initial_timestamp = u64::from_le_bytes(data[0..8].try_into().unwrap()) as i64;
            drop(data);
            staging_buffer.unmap();
            
            // Create Tracy GPU context
            let client = tracy_client::Client::running()
                .expect("Tracy client must be started before GPU profiling");
            
            let context = client.new_gpu_context(
                Some("wgpu"),
                GpuContextType::Vulkan, // wgpu abstracts over multiple backends
                initial_timestamp,
                timestamp_period,
            ).expect("Failed to create Tracy GPU context");
            
            log::info!("Tracy GPU context created with initial timestamp: {}", initial_timestamp);
            
            Self {
                context,
                pending_spans: HashMap::new(),
                query_set: Some(query_set),
                query_buffer: Some(query_buffer),
                staging_buffer: Some(staging_buffer),
                next_query_index: 1, // We used index 0 for init
                max_queries,
                timestamp_period,
                timestamps_enabled: true,
            }
        }
        
        #[cfg(not(feature = "profile-with-tracy"))]
        {
            Self {
                query_set: Some(query_set),
                query_buffer: Some(query_buffer),
                staging_buffer: Some(staging_buffer),
                next_query_index: 0,
                max_queries,
                timestamp_period,
                timestamps_enabled: true,
            }
        }
    }
    
    /// Allocate query indices for a GPU span
    pub fn allocate_queries(&mut self) -> Option<(u32, u32)> {
        if !self.timestamps_enabled || self.next_query_index + 2 > self.max_queries {
            return None;
        }
        
        let start = self.next_query_index;
        let end = self.next_query_index + 1;
        self.next_query_index += 2;
        
        Some((start, end))
    }
    
    /// Begin a GPU span
    #[cfg(feature = "profile-with-tracy")]
    pub fn begin_span(&mut self, name: &str, encoder: &mut wgpu::CommandEncoder) -> Option<GpuSpanHandle> {
        if !self.timestamps_enabled {
            return None;
        }
        
        let (start_idx, end_idx) = self.allocate_queries()?;
        
        // Write start timestamp
        if let Some(query_set) = &self.query_set {
            encoder.write_timestamp(query_set, start_idx);
        }
        
        // Create Tracy GPU span
        let span = self.context.span_alloc(name, "GPU", file!(), line!())
            .ok()?;
        
        Some(GpuSpanHandle {
            span: Some(span),
            start_query_index: start_idx,
            end_query_index: end_idx,
        })
    }
    
    /// End a GPU span
    #[cfg(feature = "profile-with-tracy")]
    pub fn end_span(&mut self, mut handle: GpuSpanHandle, encoder: &mut wgpu::CommandEncoder) {
        if !self.timestamps_enabled {
            return;
        }
        
        // Write end timestamp
        if let Some(query_set) = &self.query_set {
            encoder.write_timestamp(query_set, handle.end_query_index);
        }
        
        // End the Tracy span
        if let Some(mut span) = handle.span.take() {
            span.end_zone();
            
            // Store for later timestamp resolution
            let frame_id = self.next_query_index as u64; // Simple frame tracking
            self.pending_spans.entry(frame_id).or_insert_with(Vec::new).push(PendingGpuSpan {
                span,
                start_query_index: handle.start_query_index,
                end_query_index: handle.end_query_index,
                submitted: false,
            });
        }
    }
    
    /// Resolve timestamps after GPU work completes
    pub fn resolve_timestamps(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.timestamps_enabled || self.next_query_index == 0 {
            return;
        }
        
        if let (Some(query_set), Some(query_buffer), Some(staging_buffer)) = 
            (&self.query_set, &self.query_buffer, &self.staging_buffer) {
            
            // Resolve all queries used in this frame
            let query_count = self.next_query_index;
            encoder.resolve_query_set(query_set, 0..query_count, query_buffer, 0);
            encoder.copy_buffer_to_buffer(
                query_buffer, 
                0, 
                staging_buffer, 
                0, 
                (query_count * 8) as u64
            );
        }
    }
    
    /// Upload resolved timestamps to Tracy
    #[cfg(feature = "profile-with-tracy")]
    pub fn upload_timestamps(&mut self, device: &wgpu::Device) {
        if !self.timestamps_enabled {
            return;
        }
        
        let Some(staging_buffer) = &self.staging_buffer else { return };
        
        let buffer_slice = staging_buffer.slice(..(self.next_query_index * 8) as u64);
        let (sender, receiver) = std::sync::mpsc::channel();
        
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        
        device.poll(wgpu::Maintain::Wait);
        
        if receiver.recv().unwrap().is_ok() {
            let data = buffer_slice.get_mapped_range();
            
            // Extract timestamps
            let mut timestamps = Vec::new();
            for i in 0..self.next_query_index {
                let offset = (i * 8) as usize;
                let timestamp = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as i64;
                timestamps.push(timestamp);
            }
            
            drop(data);
            staging_buffer.unmap();
            
            // Upload to Tracy
            for (_frame_id, spans) in self.pending_spans.iter() {
                for span in spans {
                    if !span.submitted {
                        let start_timestamp = timestamps[span.start_query_index as usize];
                        let end_timestamp = timestamps[span.end_query_index as usize];
                        
                        span.span.upload_timestamp_start(start_timestamp);
                        span.span.upload_timestamp_end(end_timestamp);
                    }
                }
            }
            
            // Clear pending spans
            self.pending_spans.clear();
        }
        
        // Reset for next frame
        self.next_query_index = 0;
    }
}

/// Handle for a GPU span
#[cfg(feature = "profile-with-tracy")]
pub struct GpuSpanHandle {
    span: Option<GpuSpan>,
    start_query_index: u32,
    end_query_index: u32,
}

#[cfg(not(feature = "profile-with-tracy"))]
pub struct GpuSpanHandle;

/// Initialize the global GPU profiler
pub fn init_gpu_profiler(device: &wgpu::Device, queue: &wgpu::Queue) {
    let profiler = GpuProfiler::new(device, queue);
    *GPU_PROFILER.lock().unwrap() = Some(profiler);
}

/// Begin a GPU span using the global profiler
#[cfg(feature = "profile-with-tracy")]
pub fn gpu_span(name: &str, encoder: &mut wgpu::CommandEncoder) -> Option<GpuSpanHandle> {
    let mut profiler_guard = GPU_PROFILER.lock().unwrap();
    if profiler_guard.is_none() {
        // GPU profiler not initialized yet - this can happen if we don't have access to the wgpu queue
        // Log once to avoid spam
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            log::warn!("GPU profiler not initialized - GPU profiling disabled. Need access to wgpu queue for initialization.");
        });
        return None;
    }
    profiler_guard.as_mut()?.begin_span(name, encoder)
}

#[cfg(not(feature = "profile-with-tracy"))]
pub fn gpu_span(_name: &str, _encoder: &mut wgpu::CommandEncoder) -> Option<GpuSpanHandle> {
    None
}

/// End a GPU span using the global profiler
#[cfg(feature = "profile-with-tracy")]
pub fn end_gpu_span(handle: GpuSpanHandle, encoder: &mut wgpu::CommandEncoder) {
    if let Some(profiler) = GPU_PROFILER.lock().unwrap().as_mut() {
        profiler.end_span(handle, encoder);
    }
}

#[cfg(not(feature = "profile-with-tracy"))]
pub fn end_gpu_span(_handle: GpuSpanHandle, _encoder: &mut wgpu::CommandEncoder) {}

/// Resolve and upload timestamps for the current frame
pub fn resolve_frame_timestamps(encoder: &mut wgpu::CommandEncoder, device: &wgpu::Device) {
    if let Some(profiler) = GPU_PROFILER.lock().unwrap().as_mut() {
        profiler.resolve_timestamps(encoder);
        profiler.upload_timestamps(device);
    }
}

/// Macro for creating a GPU profiling scope
#[macro_export]
macro_rules! gpu_scope {
    ($name:expr, $encoder:expr) => {
        let _gpu_span = $crate::infrastructure::gpu_profiling::gpu_span($name, $encoder);
        // Span will be ended when it goes out of scope
    };
}