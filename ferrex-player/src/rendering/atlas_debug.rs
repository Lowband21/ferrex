//! Debug visualization for atlas usage and performance metrics
//!
//! This module provides visual overlays to verify the atlas optimization
//! is working correctly and achieving performance targets.

use crate::rendering::{AtlasBridge, AtlasStorage, StorageEntry};
use iced::{Color, Element, Font, Length, Point, Rectangle, Size};
use iced::widget::{column, container, row, text, Column, Container, Row, Space};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Debug overlay for visualizing atlas usage
pub struct AtlasDebugOverlay {
    /// Whether debug visualization is enabled
    enabled: bool,
    /// Performance metrics tracking
    metrics: PerformanceMetrics,
    /// Visual indicators for each image
    image_indicators: HashMap<u64, AtlasIndicator>,
    /// Last update time
    last_update: Instant,
}

/// Visual indicator for an image's atlas status
#[derive(Debug, Clone, Copy)]
pub enum AtlasIndicator {
    /// Image is in the atlas (green)
    InAtlas,
    /// Image uses individual texture (red)
    IndividualTexture,
    /// Image upload pending (yellow)
    Pending,
}

impl AtlasIndicator {
    /// Get the border color for this indicator
    pub fn border_color(&self) -> Color {
        match self {
            AtlasIndicator::InAtlas => Color::from_rgb(0.0, 1.0, 0.0),          // Green
            AtlasIndicator::IndividualTexture => Color::from_rgb(1.0, 0.0, 0.0), // Red
            AtlasIndicator::Pending => Color::from_rgb(1.0, 1.0, 0.0),          // Yellow
        }
    }

    /// Get the indicator label
    pub fn label(&self) -> &'static str {
        match self {
            AtlasIndicator::InAtlas => "ATLAS",
            AtlasIndicator::IndividualTexture => "TEXTURE",
            AtlasIndicator::Pending => "LOADING",
        }
    }
}

/// Performance metrics for atlas system
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Number of texture operations this frame
    pub texture_ops_per_frame: usize,
    /// Bytes uploaded this frame
    pub bytes_uploaded: usize,
    /// Number of atlas layers in use
    pub atlas_layers: usize,
    /// Atlas utilization percentage
    pub atlas_utilization: f32,
    /// Number of images in atlas
    pub images_in_atlas: usize,
    /// Number of images using individual textures
    pub images_in_textures: usize,
    /// Number of pending uploads
    pub pending_uploads: usize,
    /// Average upload time
    pub avg_upload_time: Duration,
    /// Cache hit rate
    pub cache_hit_rate: f32,
    /// Frame time for texture operations
    pub texture_frame_time: Duration,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            texture_ops_per_frame: 0,
            bytes_uploaded: 0,
            atlas_layers: 0,
            atlas_utilization: 0.0,
            images_in_atlas: 0,
            images_in_textures: 0,
            pending_uploads: 0,
            avg_upload_time: Duration::ZERO,
            cache_hit_rate: 0.0,
            texture_frame_time: Duration::ZERO,
        }
    }
}

impl AtlasDebugOverlay {
    /// Create a new debug overlay
    pub fn new() -> Self {
        Self {
            enabled: false,
            metrics: PerformanceMetrics::default(),
            image_indicators: HashMap::new(),
            last_update: Instant::now(),
        }
    }

    /// Toggle debug visualization
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        if self.enabled {
            log::info!("Atlas debug visualization enabled");
        }
    }

    /// Check if debug mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Update metrics from storage stats
    pub fn update_metrics(&mut self, storage: &AtlasStorage) {
        let stats = storage.stats();
        
        self.metrics.images_in_atlas = stats.atlas_entries;
        self.metrics.images_in_textures = stats.texture_entries;
        self.metrics.pending_uploads = stats.pending_uploads;
        
        // Calculate utilization
        let total_entries = stats.total_entries;
        if total_entries > 0 {
            self.metrics.atlas_utilization = stats.atlas_usage_percent();
        }
        
        // Update indicators for each image
        self.image_indicators.clear();
        // This would iterate through storage entries and set indicators
        // Implementation depends on storage API
    }

    /// Record a texture operation
    pub fn record_texture_op(&mut self) {
        self.metrics.texture_ops_per_frame += 1;
    }

    /// Record bytes uploaded
    pub fn record_upload(&mut self, bytes: usize, duration: Duration) {
        self.metrics.bytes_uploaded += bytes;
        
        // Update average upload time (simple moving average)
        let alpha = 0.1; // Smoothing factor
        let current_ms = duration.as_secs_f64() * 1000.0;
        let avg_ms = self.metrics.avg_upload_time.as_secs_f64() * 1000.0;
        let new_avg_ms = avg_ms * (1.0 - alpha) + current_ms * alpha;
        self.metrics.avg_upload_time = Duration::from_secs_f64(new_avg_ms / 1000.0);
    }

    /// Reset per-frame counters
    pub fn reset_frame(&mut self) {
        self.metrics.texture_ops_per_frame = 0;
        self.metrics.bytes_uploaded = 0;
    }

    /// Get indicator for an image
    pub fn get_indicator(&self, image_id: u64) -> Option<AtlasIndicator> {
        self.image_indicators.get(&image_id).copied()
    }

    /// Create the debug overlay UI element
    pub fn view<'a, Message>(&self) -> Element<'a, Message> 
    where
        Message: 'a,
    {
        if !self.enabled {
            return Space::new(Length::Fixed(0.0), Length::Fixed(0.0)).into();
        }

        let title = text("Atlas Debug")
            .size(16)
            .font(Font::MONOSPACE);

        let metrics = column![
            text(format!("Texture Ops/Frame: {} (target: <3)", 
                self.metrics.texture_ops_per_frame))
                .size(12)
                .font(Font::MONOSPACE)
                .color(if self.metrics.texture_ops_per_frame <= 3 {
                    Color::from_rgb(0.0, 1.0, 0.0)
                } else {
                    Color::from_rgb(1.0, 0.0, 0.0)
                }),
            
            text(format!("Atlas Utilization: {:.1}%", 
                self.metrics.atlas_utilization))
                .size(12)
                .font(Font::MONOSPACE),
            
            text(format!("Atlas Layers: {}", 
                self.metrics.atlas_layers))
                .size(12)
                .font(Font::MONOSPACE),
            
            text(format!("Images: {} atlas / {} texture / {} pending",
                self.metrics.images_in_atlas,
                self.metrics.images_in_textures,
                self.metrics.pending_uploads))
                .size(12)
                .font(Font::MONOSPACE)
                .color(if self.metrics.images_in_textures == 0 {
                    Color::from_rgb(0.0, 1.0, 0.0)
                } else {
                    Color::from_rgb(1.0, 1.0, 0.0)
                }),
            
            text(format!("Upload: {:.2} MB/frame", 
                self.metrics.bytes_uploaded as f64 / 1_048_576.0))
                .size(12)
                .font(Font::MONOSPACE),
            
            text(format!("Avg Upload Time: {:.2}ms", 
                self.metrics.avg_upload_time.as_secs_f64() * 1000.0))
                .size(12)
                .font(Font::MONOSPACE),
            
            text(format!("Texture Frame Time: {:.2}ms (target: <2ms)",
                self.metrics.texture_frame_time.as_secs_f64() * 1000.0))
                .size(12)
                .font(Font::MONOSPACE)
                .color(if self.metrics.texture_frame_time.as_millis() < 2 {
                    Color::from_rgb(0.0, 1.0, 0.0)
                } else {
                    Color::from_rgb(1.0, 0.0, 0.0)
                }),
        ]
        .spacing(2);

        container(
            column![title, metrics]
                .spacing(5)
                .padding(10)
        )
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.8).into()),
            text_color: Some(Color::WHITE),
            border: iced::Border {
                color: Color::from_rgb(0.5, 0.5, 0.5),
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        })
        .into()
    }

    /// Create a visual representation of atlas pages
    pub fn render_atlas_visualization(&self, atlas_bridge: &AtlasBridge) -> Vec<DebugPrimitive> {
        if !self.enabled {
            return Vec::new();
        }

        let mut primitives = Vec::new();
        
        // This would create visual representations of atlas texture arrays
        // Showing how images are packed in each layer
        // Implementation depends on atlas bridge API
        
        primitives
    }

    /// Check if performance targets are met
    pub fn is_meeting_targets(&self) -> bool {
        self.metrics.texture_ops_per_frame <= 3
            && self.metrics.texture_frame_time.as_millis() < 2
            && self.metrics.images_in_textures == 0
    }

    /// Get a performance summary string
    pub fn performance_summary(&self) -> String {
        format!(
            "Atlas Performance: {} | Ops: {}/frame | Time: {:.2}ms | Atlas: {}% ({}/{} images)",
            if self.is_meeting_targets() { "✅ OPTIMAL" } else { "⚠️ SUBOPTIMAL" },
            self.metrics.texture_ops_per_frame,
            self.metrics.texture_frame_time.as_secs_f64() * 1000.0,
            self.metrics.atlas_utilization as u32,
            self.metrics.images_in_atlas,
            self.metrics.images_in_atlas + self.metrics.images_in_textures
        )
    }
}

/// Debug primitive for custom rendering
#[derive(Debug, Clone)]
pub enum DebugPrimitive {
    /// Draw a border around an element
    Border {
        bounds: Rectangle,
        color: Color,
        width: f32,
    },
    /// Draw text overlay
    Text {
        position: Point,
        content: String,
        size: f32,
        color: Color,
    },
    /// Draw a filled rectangle
    FilledRect {
        bounds: Rectangle,
        color: Color,
    },
}

/// Helper to add debug borders to images
pub fn add_debug_border(
    bounds: Rectangle,
    indicator: AtlasIndicator,
    primitives: &mut Vec<DebugPrimitive>,
) {
    primitives.push(DebugPrimitive::Border {
        bounds,
        color: indicator.border_color(),
        width: 2.0,
    });
    
    // Add label in corner
    primitives.push(DebugPrimitive::Text {
        position: Point::new(bounds.x + 2.0, bounds.y + 2.0),
        content: indicator.label().to_string(),
        size: 10.0,
        color: indicator.border_color(),
    });
}

impl Default for AtlasDebugOverlay {
    fn default() -> Self {
        Self::new()
    }
}