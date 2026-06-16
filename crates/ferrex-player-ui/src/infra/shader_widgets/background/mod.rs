//! GPU-accelerated background shader widget for Ferrex media player
//!
//! This widget creates visually appealing animated backgrounds with gradients,
//! depth effects, and subtle animations for a professional streaming app experience.

pub mod state;
pub mod transitions;
pub mod types;

pub use crate::domains::ui::messages::UiMessage;
pub use types::ContentOffsetPx;

use crate::{
    domains::ui::{theme::MediaServerTheme, types::BackdropAspectMode},
    infra::shader_widgets::background::transitions::generate_random_gradient_center,
};

use bytemuck::{Pod, Zeroable};
use ferrex_core::player_prelude::{
    TheaterPlateAnalysis, TheaterPlateAnalyzer, TheaterPlateColor,
    TheaterPlateGrade, TheaterPlateSourceContext, TheaterPlateViewport,
};

use iced::{
    Color, Element, Length, Rectangle, Vector,
    advanced::{graphics::Viewport, image::Id as ImageId},
    mouse, wgpu,
    widget::shader::{Pipeline as ShaderPipeline, Primitive, Program},
};

use std::{collections::HashMap, sync::Arc, time::Instant};

/// Background effect types
#[derive(Debug, Clone)]
pub enum BackgroundEffect {
    /// Simple solid color (for testing)
    Solid,
    /// Animated gradient
    Gradient,
    /// Subtle noise pattern
    SubtleNoise { scale: f32, speed: f32 },
    /// Floating particles
    FloatingParticles { count: u32, size: f32 },
    /// Wave ripple effect
    WaveRipple { frequency: f32, amplitude: f32 },
    /// Backdrop image composited over a gradient background
    BackdropGradient,
    /// Theater Plate detail background with downsampled ambient color field and soft masks
    TheaterPlate,
}

/// Background theme presets
#[derive(Debug, Clone, Copy)]
pub enum BackgroundTheme {
    /// Minimal - subtle gradients, no effects
    Minimal,
    /// Professional - deep shadows, clean lines
    Professional,
    /// Vibrant - rich colors, active particles
    Vibrant,
    /// Cinematic - dark with spotlighting
    Cinematic,
    /// Adaptive - responds to content
    Adaptive,
}

/// Uniform byte size shared with the WGSL `TheaterPlateUniforms` struct.
pub const THEATER_PLATE_UNIFORM_SIZE: u64 = 144;

/// Uniforms for the dedicated Theater Plate layer stack.
///
/// Every field maps to one WGSL `vec4<f32>` so field offsets stay on 16-byte
/// boundaries for uniform-buffer layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct TheaterPlateUniforms {
    /// Base stage color: rgb, reserved.
    pub base_stage: [f32; 4],
    /// Ambient field: opacity, texture strength, average luma, p95 luma.
    pub ambient_field: [f32; 4],
    /// Focused plate: center x/y and half extents in UV space.
    pub focused_plate: [f32; 4],
    /// Plate mask: opacity, corner radius px, feather px, side falloff.
    pub plate_mask: [f32; 4],
    /// Scrim masks: opacity, scrim top UV, scrim bottom UV, side falloff.
    pub scrim_masks: [f32; 4],
    /// Hero art rectangle: normalized viewport x, y, width, height.
    pub hero_art_rect: [f32; 4],
    /// Vignette/grain controls: vignette opacity, grain opacity, radius, softness.
    pub vignette_grain: [f32; 4],
    /// Grade controls: highlight compression, desaturation, saturation, edge density.
    pub highlight_grade: [f32; 4],
    /// Transition controls: progress, backdrop opacity, ambient transition, reserved.
    pub transition: [f32; 4],
}

impl Default for TheaterPlateUniforms {
    fn default() -> Self {
        Self {
            base_stage: color_to_vec4(TheaterPlateColor::DEFAULT_STAGE),
            ambient_field: [0.52, 0.85, 0.18, 0.45],
            focused_plate: [0.58, 0.46, 0.50, 0.34],
            plate_mask: [0.44, 34.0, 76.0, 0.34],
            scrim_masks: [0.46, 0.10, 0.32, 0.34],
            hero_art_rect: [0.0, 0.0, 0.0, 0.0],
            vignette_grain: [0.42, 0.016, 0.24, 0.82],
            highlight_grade: [0.34, 0.08, 0.0, 0.0],
            transition: [1.0, 1.0, 1.0, 0.0],
        }
    }
}

/// Shader-ready Theater Plate geometry derived from a detail route layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TheaterPlateGeometry {
    pub focused_plate: [f32; 4],
    pub plate_mask: [f32; 4],
    pub scrim_masks: [f32; 4],
    pub hero_art_rect: [f32; 4],
    pub ambient_opacity_scale: f32,
    pub vignette_opacity: f32,
    pub grain_opacity_scale: f32,
    pub backdrop_opacity: f32,
}

impl Default for TheaterPlateGeometry {
    fn default() -> Self {
        Self {
            focused_plate: TheaterPlateUniforms::default().focused_plate,
            plate_mask: TheaterPlateUniforms::default().plate_mask,
            scrim_masks: TheaterPlateUniforms::default().scrim_masks,
            hero_art_rect: TheaterPlateUniforms::default().hero_art_rect,
            ambient_opacity_scale: 1.0,
            vignette_opacity: TheaterPlateUniforms::default().vignette_grain[0],
            grain_opacity_scale: 1.0,
            backdrop_opacity: 1.0,
        }
    }
}

/// Tiny downsampled ambient texture consumed by the Theater Plate shader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheaterPlateAmbientImage {
    pub cache_key: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl TheaterPlateAmbientImage {
    fn solid(cache_key: u64, color: TheaterPlateColor) -> Self {
        Self {
            cache_key,
            width: 1,
            height: 1,
            rgba: Arc::<[u8]>::from([color.r, color.g, color.b, 255]),
        }
    }
}

/// CPU-side Theater Plate scene data passed to the shader widget.
#[derive(Debug, Clone, PartialEq)]
pub struct TheaterPlateScene {
    pub uniforms: TheaterPlateUniforms,
    pub ambient: TheaterPlateAmbientImage,
}

impl TheaterPlateScene {
    /// Build shader inputs from the cached CPU analysis sidecar.
    pub fn from_analysis(
        cache_key: u64,
        analysis: &TheaterPlateAnalysis,
    ) -> Self {
        let ambient = ambient_image_from_analysis(cache_key, analysis);
        let uniforms = uniforms_from_analysis(analysis);
        Self { uniforms, ambient }
    }

    /// Build a fallback Theater Plate stage when no usable backdrop texture is available.
    pub fn missing_backdrop_from_colors(
        cache_key: u64,
        viewport: TheaterPlateViewport,
        poster_color: Option<TheaterPlateColor>,
        theme_color: Option<TheaterPlateColor>,
        default_color: TheaterPlateColor,
    ) -> Self {
        let context = TheaterPlateSourceContext::missing_backdrop(viewport)
            .with_poster_color(poster_color)
            .with_theme_color(theme_color)
            .with_default_color(default_color);
        let analysis =
            TheaterPlateAnalyzer::default().analyze_missing_backdrop(context);
        Self::from_analysis(cache_key, &analysis)
    }

    /// Apply layout-derived readability geometry while preserving image grade decisions.
    pub fn with_geometry(mut self, geometry: TheaterPlateGeometry) -> Self {
        self.uniforms.focused_plate = geometry.focused_plate;
        self.uniforms.plate_mask = [
            self.uniforms.plate_mask[0]
                .max(geometry.plate_mask[0])
                .clamp(0.0, 1.0),
            geometry.plate_mask[1].max(0.0),
            geometry.plate_mask[2].max(1.0),
            geometry.plate_mask[3].clamp(0.0, 0.85),
        ];
        self.uniforms.scrim_masks = [
            self.uniforms.scrim_masks[0]
                .max(geometry.scrim_masks[0])
                .clamp(0.0, 1.0),
            geometry.scrim_masks[1].clamp(0.001, 1.0),
            geometry.scrim_masks[2].clamp(0.001, 1.0),
            geometry.scrim_masks[3].clamp(0.0, 0.85),
        ];
        let art_x = geometry.hero_art_rect[0].clamp(0.0, 1.0);
        let art_y = geometry.hero_art_rect[1].clamp(0.0, 1.0);
        self.uniforms.hero_art_rect = [
            art_x,
            art_y,
            geometry.hero_art_rect[2].clamp(0.0, 1.0 - art_x),
            geometry.hero_art_rect[3].clamp(0.0, 1.0 - art_y),
        ];
        self.uniforms.ambient_field[0] = (self.uniforms.ambient_field[0]
            * geometry.ambient_opacity_scale)
            .clamp(0.0, 1.0);
        self.uniforms.vignette_grain[0] = self.uniforms.vignette_grain[0]
            .max(geometry.vignette_opacity)
            .clamp(0.0, 1.0);
        self.uniforms.vignette_grain[1] = (self.uniforms.vignette_grain[1]
            * geometry.grain_opacity_scale)
            .clamp(0.0, 0.08);
        self.uniforms.transition[1] = geometry.backdrop_opacity.clamp(0.0, 1.0);
        self
    }

    /// Build a cheap solid ambient field while the analysis sidecar is not ready.
    pub fn fallback_from_colors(
        cache_key: u64,
        primary: Color,
        secondary: Color,
    ) -> Self {
        let stage = color_from_iced(primary)
            .unwrap_or(TheaterPlateColor::DEFAULT_STAGE)
            .stage_wash();
        let accent = color_from_iced(secondary).unwrap_or(stage);
        let ambient = TheaterPlateAmbientImage {
            cache_key,
            width: 2,
            height: 2,
            rgba: Arc::<[u8]>::from([
                stage.r, stage.g, stage.b, 255, accent.r, accent.g, accent.b,
                255, stage.r, stage.g, stage.b, 255, accent.r, accent.g,
                accent.b, 255,
            ]),
        };
        Self {
            uniforms: TheaterPlateUniforms {
                base_stage: color_to_vec4(stage),
                ambient_field: [0.50, 0.72, stage.luminance(), 0.45],
                highlight_grade: [0.30, 0.06, accent.saturation(), 0.0],
                ..TheaterPlateUniforms::default()
            },
            ambient,
        }
    }
}

fn color_to_vec4(color: TheaterPlateColor) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        1.0,
    ]
}

fn color_from_iced(color: Color) -> Option<TheaterPlateColor> {
    if !color.r.is_finite() || !color.g.is_finite() || !color.b.is_finite() {
        return None;
    }

    Some(TheaterPlateColor::rgb(
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    ))
}

fn ambient_image_from_analysis(
    cache_key: u64,
    analysis: &TheaterPlateAnalysis,
) -> TheaterPlateAmbientImage {
    let downsample = &analysis.downsample;
    if downsample.width == 0
        || downsample.height == 0
        || downsample.pixels.is_empty()
    {
        return TheaterPlateAmbientImage::solid(
            cache_key,
            analysis.grade.stage_color,
        );
    }

    let expected_len = downsample.width as usize * downsample.height as usize;
    let mut rgba = Vec::with_capacity(expected_len * 4);
    for color in downsample.pixels.iter().take(expected_len) {
        rgba.extend_from_slice(&[color.r, color.g, color.b, 255]);
    }

    if rgba.len() < expected_len * 4 {
        let fill = analysis.grade.stage_color;
        while rgba.len() < expected_len * 4 {
            rgba.extend_from_slice(&[fill.r, fill.g, fill.b, 255]);
        }
    }

    TheaterPlateAmbientImage {
        cache_key,
        width: downsample.width.max(1),
        height: downsample.height.max(1),
        rgba: rgba.into(),
    }
}

fn uniforms_from_analysis(
    analysis: &TheaterPlateAnalysis,
) -> TheaterPlateUniforms {
    let grade = analysis.grade;
    let side_falloff = side_falloff_for_grade(&grade);
    let vignette = (0.28 + grade.scrim_opacity * 0.36).clamp(0.0, 0.78);

    TheaterPlateUniforms {
        base_stage: color_to_vec4(grade.stage_color),
        ambient_field: [
            grade.ambient_opacity,
            0.92,
            analysis.average_luminance,
            analysis.p95_luminance,
        ],
        focused_plate: [0.58, 0.46, 0.50, 0.34],
        plate_mask: [grade.plate_opacity, 34.0, 76.0, side_falloff],
        scrim_masks: [grade.scrim_opacity, 0.10, 0.32, side_falloff],
        hero_art_rect: [0.0, 0.0, 0.0, 0.0],
        vignette_grain: [vignette, grade.grain_opacity, 0.24, 0.82],
        highlight_grade: [
            grade.highlight_compression,
            grade.desaturation,
            analysis.average_saturation,
            analysis.edge_density,
        ],
        transition: [1.0, 1.0, 1.0, 0.0],
    }
}

fn side_falloff_for_grade(grade: &TheaterPlateGrade) -> f32 {
    if grade.is_busy || grade.is_bright {
        0.46
    } else if grade.is_dark {
        0.22
    } else if grade.is_saturated {
        0.38
    } else {
        0.30
    }
}

#[cfg(test)]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
fn soft_vertical_band_alpha(
    y: f32,
    top: f32,
    bottom: f32,
    feather: f32,
) -> f32 {
    if bottom <= top {
        return 0.0;
    }
    let feather = feather.min((bottom - top) * 0.5).max(0.0001);
    smoothstep(top, top + feather, y)
        * (1.0 - smoothstep(bottom - feather, bottom, y))
}

#[cfg(test)]
fn theater_plate_soft_rect_alpha(
    uv: [f32; 2],
    center: [f32; 2],
    half_size: [f32; 2],
    radius_px: f32,
    feather_px: f32,
    resolution: [f32; 2],
) -> f32 {
    let p = [uv[0] * resolution[0], uv[1] * resolution[1]];
    let c = [center[0] * resolution[0], center[1] * resolution[1]];
    let h = [half_size[0] * resolution[0], half_size[1] * resolution[1]];
    let q = [
        (p[0] - c[0]).abs() - h[0] + radius_px,
        (p[1] - c[1]).abs() - h[1] + radius_px,
    ];
    let outside = [q[0].max(0.0), q[1].max(0.0)];
    let outside_len =
        (outside[0] * outside[0] + outside[1] * outside[1]).sqrt();
    let inside = q[0].max(q[1]).min(0.0);
    let distance = outside_len + inside - radius_px;
    1.0 - smoothstep(0.0, feather_px.max(0.0001), distance)
}

#[cfg(test)]
fn theater_plate_lobe_alpha(
    uv: [f32; 2],
    center: [f32; 2],
    half_size: [f32; 2],
    feather: f32,
) -> f32 {
    fn ellipse(
        uv: [f32; 2],
        center: [f32; 2],
        half_size: [f32; 2],
        feather: f32,
    ) -> f32 {
        let dx = (uv[0] - center[0]) / half_size[0].max(0.001);
        let dy = (uv[1] - center[1]) / half_size[1].max(0.001);
        let d = (dx * dx + dy * dy).sqrt();
        1.0 - smoothstep(1.0 - feather * 0.35, 1.0 + feather, d)
    }

    let feather = feather.clamp(0.18, 0.85);
    let title = ellipse(
        uv,
        [
            center[0] - half_size[0] * 0.16,
            center[1] - half_size[1] * 0.24,
        ],
        [half_size[0] * 0.92, half_size[1] * 0.58],
        feather,
    );
    let metadata = ellipse(
        uv,
        [
            center[0] + half_size[0] * 0.18,
            center[1] + half_size[1] * 0.02,
        ],
        [half_size[0] * 0.72, half_size[1] * 0.48],
        feather,
    ) * 0.86;
    let actions = ellipse(
        uv,
        [
            center[0] - half_size[0] * 0.08,
            center[1] + half_size[1] * 0.36,
        ],
        [half_size[0] * 0.82, half_size[1] * 0.40],
        feather,
    ) * 0.78;
    let floor = ellipse(
        uv,
        [center[0], center[1] + half_size[1] * 0.56],
        [half_size[0] * 1.10, half_size[1] * 0.34],
        feather,
    ) * 0.50;

    title.max(metadata).max(actions).max(floor).clamp(0.0, 1.0)
}

// ===== Region-Based Depth System =====

/// Edge transition style for regions
#[derive(Debug, Clone, Copy)]
pub enum EdgeTransition {
    /// Sharp edge with no transition
    Sharp,
    /// Soft gradient transition
    Soft {
        /// Width of the transition zone in pixels
        width: f32,
    },
    /// Beveled edge with 45-degree angle
    Beveled {
        /// Width of the bevel in pixels
        width: f32,
    },
}

/// A rectangular region with depth properties
#[derive(Debug, Clone)]
pub struct DepthRegion {
    /// Bounds of the region (x, y, width, height)
    pub bounds: Rectangle,

    /// Depth of this region (negative = sunken, 0 = surface, positive = raised)
    pub depth: f32,

    /// Edge transition style for all edges
    pub edge_transition: EdgeTransition,

    /// Individual edge overrides (optional)
    pub edge_overrides: EdgeOverrides,

    /// Whether this region casts/receives shadows
    pub shadow_enabled: bool,

    /// Shadow intensity multiplier for this region (0.0 to 1.0)
    pub shadow_intensity: f32,

    /// Z-order for overlapping regions (higher = on top)
    pub z_order: i32,

    /// Optional visible border
    pub border: Option<RegionBorder>,
}

/// Override edge transitions for specific sides
#[derive(Debug, Clone, Default)]
pub struct EdgeOverrides {
    pub top: Option<EdgeTransition>,
    pub right: Option<EdgeTransition>,
    pub bottom: Option<EdgeTransition>,
    pub left: Option<EdgeTransition>,
}

/// Visible border for a region
#[derive(Debug, Clone)]
pub struct RegionBorder {
    /// Border width in pixels
    pub width: f32,
    /// Border color
    pub color: Color,
    /// Border opacity (0.0 to 1.0)
    pub opacity: f32,
}

/// Complete depth layout using regions
#[derive(Debug, Clone)]
pub struct DepthLayout {
    /// All depth regions in the layout
    pub regions: Vec<DepthRegion>,
    /// Global light direction for consistent shadows (normalized 2D vector)
    pub ambient_light_direction: Vector,
    /// Base depth for areas without region effects
    pub base_depth: f32,
    /// Global shadow intensity (0.0 to 1.0)
    pub shadow_intensity: f32,
    /// Maximum shadow distance in pixels
    pub shadow_distance: f32,
}

/// Quality settings for performance control
#[derive(Debug, Clone, Copy)]
pub struct QualitySettings {
    /// Resolution scale (0.5 to 1.0)
    pub resolution_scale: f32,
    /// Effect complexity (1-4 levels)
    pub effect_complexity: u32,
    /// Animation FPS (30 or 60)
    pub animation_fps: u32,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            resolution_scale: 1.0,
            effect_complexity: 3,
            animation_fps: 120,
        }
    }
}

impl QualitySettings {
    /// Auto-detect quality based on hardware
    pub fn auto_detect() -> Self {
        // TODO: Implement actual hardware detection
        Self::default()
    }
}

/// The shader program for rendering backgrounds
#[derive(Debug, Clone)]
pub struct BackgroundShaderProgram {
    pub effect: BackgroundEffect,
    pub theme: BackgroundTheme,
    pub quality: QualitySettings,
    pub primary_color: Color,
    pub secondary_color: Color,
    pub start_time: Instant,
    pub scroll_offset: f32,
    pub content_offset_px: ContentOffsetPx,
    /// Transition data
    pub prev_primary_color: Color,
    pub prev_secondary_color: Color,
    pub transition_progress: f32,
    pub backdrop_opacity: f32,
    pub backdrop_slide_offset: f32,
    pub backdrop_scale: f32,
    /// Stable ID for this program instance
    id: usize,
    /// Gradient center position
    pub gradient_center: (f32, f32),
    /// Backdrop handle for overlay rendering
    pub backdrop_handle: Option<iced::widget::image::Handle>,
    /// Depth layout for visual hierarchy
    pub depth_layout: DepthLayout,
    /// Header offset for detail views
    pub header_offset: f32,
    /// Backdrop aspect ratio mode
    pub backdrop_aspect_mode: BackdropAspectMode,
    /// Optional fallback aspect ratio for the backdrop texture
    pub backdrop_aspect_ratio: Option<f32>,
    /// Theater Plate layer-stack uniforms and ambient texture.
    pub theater_plate: Option<TheaterPlateScene>,
}

// Global counter for generating unique IDs
static BACKGROUND_ID_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl<Message> Program<Message> for BackgroundShaderProgram {
    type State = ();
    type Primitive = BackgroundPrimitive;

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        BackgroundPrimitive {
            bounds,
            effect: self.effect.clone(),
            theme: self.theme,
            quality: self.quality,
            primary_color: self.primary_color,
            secondary_color: self.secondary_color,
            start_time: self.start_time,
            program_id: self.id,
            scroll_offset: self.scroll_offset,
            content_offset_px: self.content_offset_px,
            // Pass through transition data
            prev_primary_color: self.prev_primary_color,
            prev_secondary_color: self.prev_secondary_color,
            transition_progress: self.transition_progress,
            backdrop_opacity: self.backdrop_opacity,
            backdrop_slide_offset: self.backdrop_slide_offset,
            backdrop_scale: self.backdrop_scale,
            gradient_center: self.gradient_center,
            backdrop_handle: self.backdrop_handle.clone(),
            depth_layout: self.depth_layout.clone(),
            header_offset: self.header_offset,
            backdrop_aspect_mode: self.backdrop_aspect_mode,
            backdrop_aspect_ratio: self.backdrop_aspect_ratio,
            theater_plate: self.theater_plate.clone(),
        }
    }
}

/// The primitive that renders the background
#[derive(Debug, Clone)]
pub struct BackgroundPrimitive {
    pub bounds: Rectangle,
    pub effect: BackgroundEffect,
    pub theme: BackgroundTheme,
    pub quality: QualitySettings,
    pub primary_color: Color,
    pub secondary_color: Color,
    pub start_time: Instant,
    /// Stable ID from the program that created this primitive
    pub program_id: usize,
    /// Scroll offset for fixed backdrop positioning
    pub scroll_offset: f32,
    /// Content-space offset for deterministic noise anchoring
    pub content_offset_px: ContentOffsetPx,
    /// Transition data
    pub prev_primary_color: Color,
    pub prev_secondary_color: Color,
    pub transition_progress: f32,
    pub backdrop_opacity: f32,
    pub backdrop_slide_offset: f32,
    pub backdrop_scale: f32,
    /// Gradient center position
    pub gradient_center: (f32, f32),
    /// Backdrop handle for overlay rendering
    pub backdrop_handle: Option<iced::widget::image::Handle>,
    /// Depth layout for visual hierarchy
    pub depth_layout: DepthLayout,
    /// Header offset for detail views
    pub header_offset: f32,
    /// Backdrop aspect ratio mode
    pub backdrop_aspect_mode: BackdropAspectMode,
    /// Optional fallback aspect ratio for the backdrop texture
    pub backdrop_aspect_ratio: Option<f32>,
    /// Theater Plate layer-stack uniforms and ambient texture.
    pub theater_plate: Option<TheaterPlateScene>,
}

/// Global uniform data
/// Note: Alignment must match WGSL expectations
/// WGSL vec3 requires 16-byte alignment, causing implicit padding
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    // Transform and time
    transform: [f32; 16],          // offset 0, size 64
    time_and_resolution: [f32; 4], // time, 0, resolution.x, resolution.y (offset 64, size 16)
    scale_and_effect: [f32; 4], // scale_factor, effect_type, effect_param1, effect_param2 (offset 80, size 16)

    // Colors
    primary_color: [f32; 4],   // offset 96, size 16
    secondary_color: [f32; 4], // offset 112, size 16

    // Texture and scroll
    texture_params: [f32; 4], // texture_aspect, scroll_offset, header_offset, 0 (offset 128, size 16)

    // Content-space offset (for deterministic noise anchoring)
    content_offset_px: [f32; 4], // content_offset_px.x, content_offset_px.y, 0, 0 (offset 144, size 16)

    // Transition colors
    prev_primary_color: [f32; 4], // offset 160, size 16
    prev_secondary_color: [f32; 4], // offset 176, size 16

    // Transition parameters
    transition_params: [f32; 4], // transition_progress, backdrop_opacity, backdrop_slide_offset, backdrop_scale (offset 192, size 16)

    // Gradient and depth
    gradient_center: [f32; 4], // gradient_center.x, gradient_center.y, 0, 0 (offset 208, size 16)
    depth_params: [f32; 4], // region_count, base_depth, shadow_intensity, shadow_distance (offset 224, size 16)
    ambient_light: [f32; 4], // light_dir.x, light_dir.y, 0, 0 (offset 240, size 16)

    // Depth regions (up to 8)
    region1_bounds: [f32; 4], // x, y, width, height (offset 256, size 16)
    region1_depth_params: [f32; 4], // depth, edge_transition_type, edge_width, shadow_enabled (offset 272, size 16)
    region1_shadow_params: [f32; 4], // shadow_intensity, z_order, border_width, border_opacity (offset 288, size 16)
    region1_border_color: [f32; 4],  // r, g, b, a (offset 304, size 16)

    region2_bounds: [f32; 4], // (offset 320, size 16)
    region2_depth_params: [f32; 4], // (offset 336, size 16)
    region2_shadow_params: [f32; 4], // (offset 352, size 16)
    region2_border_color: [f32; 4], // (offset 368, size 16)

    region3_bounds: [f32; 4], // (offset 384, size 16)
    region3_depth_params: [f32; 4], // (offset 400, size 16)
    region3_shadow_params: [f32; 4], // (offset 416, size 16)
    region3_border_color: [f32; 4], // (offset 432, size 16)

    region4_bounds: [f32; 4], // (offset 448, size 16)
    region4_depth_params: [f32; 4], // (offset 464, size 16)
    region4_shadow_params: [f32; 4], // (offset 480, size 16)
    region4_border_color: [f32; 4], // (offset 496, size 16)

                              // Total: 512 bytes (32 * 16)
}

const GLOBALS_UNIFORM_SIZE: u64 = 512;

// Compile-time assertions to verify our struct sizes
const _: () = {
    let size = std::mem::size_of::<Globals>();
    assert!(
        size == GLOBALS_UNIFORM_SIZE as usize,
        "Globals struct size mismatch"
    );
    let theater_size = std::mem::size_of::<TheaterPlateUniforms>();
    assert!(
        theater_size == THEATER_PLATE_UNIFORM_SIZE as usize,
        "TheaterPlateUniforms struct size mismatch"
    );
};

/// Pipeline state
#[derive(Debug)]
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    texture_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
    default_texture: Arc<wgpu::Texture>,
    default_ambient_texture: Arc<wgpu::Texture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextureBindGroupKey {
    backdrop_id: Option<ImageId>,
    ambient_cache_key: Option<u64>,
}

/// Per-primitive render data
#[derive(Debug)]
struct PrimitiveData {
    texture_bind_group: Option<wgpu::BindGroup>,
}

/// Texture info
#[derive(Debug)]
struct TextureInfo {
    texture: Arc<wgpu::Texture>,
    aspect_ratio: f32, // width / height
}

/// Cached ambient field texture info.
#[derive(Debug)]
struct AmbientTextureInfo {
    texture: Arc<wgpu::Texture>,
}

/// Shared state
#[derive(Debug, Default)]
struct State {
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    theater_uniform_buffer: Option<wgpu::Buffer>,
    default_texture_bind_group: Option<wgpu::BindGroup>,
    // Texture cache for backdrops
    texture_cache: HashMap<ImageId, TextureInfo>,
    texture_bind_groups: HashMap<TextureBindGroupKey, wgpu::BindGroup>,
    // Texture cache for Theater Plate ambient fields
    ambient_texture_cache: HashMap<u64, AmbientTextureInfo>,
    // Per-primitive data for current frame
    primitive_data: HashMap<usize, PrimitiveData>,
    // Track if default texture has been initialized
    default_texture_initialized: bool,
}

#[derive(Debug)]
pub struct BackgroundRenderer {
    pipeline: Pipeline,
    state: State,
}

impl ShaderPipeline for BackgroundRenderer {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        BackgroundRenderer {
            pipeline: Pipeline::new(device, format),
            state: State::default(),
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl Pipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Load shader - add timestamp to force recompilation
        let shader_label = format!(
            "Background Shader {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&shader_label),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../../shaders/background.wgsl").into(),
                ),
            });

        // Create globals bind group layout
        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Globals"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None, // Let WGPU calculate the size
                    },
                    count: None,
                }],
            });

        // Create a combined texture bind group layout. Keep Theater Plate in
        // group 1 with the backdrop texture so the shader only needs two bind
        // groups total; some screenshot/emulator adapters expose exactly two.
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Textures and Theater Plate"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Background Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Create default 1x1 transparent texture for when no backdrop is available
        let default_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Default Background Texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB texture so sampling returns linear values for composition
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let default_ambient_texture =
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Default Theater Plate Ambient Texture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

        // Create provider layout
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Background Pipeline Layout"),
                bind_group_layouts: &[
                    &globals_bind_group_layout,
                    &texture_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });

        // Create render provider
        let render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Background Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[], // No vertex buffers - we generate vertices in shader
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING), // Use alpha blending like rounded_image_shader
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            });

        Pipeline {
            render_pipeline,
            globals_bind_group_layout: Arc::new(globals_bind_group_layout),
            texture_bind_group_layout: Arc::new(texture_bind_group_layout),
            sampler: Arc::new(sampler),
            default_texture: Arc::new(default_texture),
            default_ambient_texture: Arc::new(default_ambient_texture),
        }
    }
}

/// Load texture from image handle
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    handle: &iced::widget::image::Handle,
) -> Option<(Arc<wgpu::Texture>, f32)> {
    use iced::widget::image::Handle;

    let (image_data, width, height) = match handle {
        Handle::Path(_, path) => match ::image::open(path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                (rgba.into_raw(), w, h)
            }
            Err(e) => {
                log::error!(
                    "Failed to load backdrop from path {:?}: {}",
                    path,
                    e
                );
                return None;
            }
        },
        Handle::Bytes(_, bytes) => match ::image::load_from_memory(bytes) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                (rgba.into_raw(), w, h)
            }
            Err(e) => {
                log::error!("Failed to load backdrop from bytes: {}", e);
                return None;
            }
        },
        Handle::Rgba {
            pixels,
            width,
            height,
            ..
        } => (pixels.to_vec(), *width, *height),
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Background Backdrop Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Use sRGB so sampling yields linear values in shader
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // wgpu requires bytes_per_row to be COPY_BYTES_PER_ROW_ALIGNMENT-aligned
    // when copying multiple rows. Pad rows if necessary.
    let bytes_per_pixel: u32 = 4;
    let row_stride = (width * bytes_per_pixel) as usize;
    let padded_row_stride =
        crate::infra::render::row_padding::compute_padded_stride(
            width,
            bytes_per_pixel,
        );

    if padded_row_stride == row_stride {
        log::debug!(
            "Background upload: {}x{} RGBA, row_bytes={}, bytes_per_row={}, rows_per_image={}, extent=({}, {}, 1)",
            width,
            height,
            row_stride,
            row_stride,
            height,
            width,
            height
        );
        debug_assert!(
            height == 1
                || row_stride.is_multiple_of(
                    wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize
                ),
            "Multi-row upload without padding should be aligned",
        );
        // Already aligned; we can upload directly
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes_per_pixel),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    } else {
        log::debug!(
            "Background upload: {}x{} RGBA, row_bytes={} padded_bytes_per_row={}, rows_per_image={}, extent=({}, {}, 1)",
            width,
            height,
            row_stride,
            padded_row_stride,
            height,
            width,
            height
        );
        // Create a padded buffer and copy each row with padding
        let (padded, padded_row_stride) =
            crate::infra::render::row_padding::pad_rows_rgba(
                &image_data,
                width,
                height,
            );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &padded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_stride as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    let aspect_ratio = width as f32 / height as f32;
    Some((Arc::new(texture), aspect_ratio))
}

fn load_ambient_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ambient: &TheaterPlateAmbientImage,
) -> Arc<wgpu::Texture> {
    let width = ambient.width.max(1);
    let height = ambient.height.max(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Theater Plate Ambient Field Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes_per_pixel: u32 = 4;
    let row_stride = (width * bytes_per_pixel) as usize;
    let expected_len = row_stride * height as usize;
    let mut rgba = Vec::with_capacity(expected_len);
    rgba.extend_from_slice(
        &ambient.rgba[..ambient.rgba.len().min(expected_len)],
    );
    if rgba.len() < expected_len {
        rgba.resize(expected_len, 255);
    }

    let padded_row_stride =
        crate::infra::render::row_padding::compute_padded_stride(
            width,
            bytes_per_pixel,
        );

    if padded_row_stride == row_stride {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes_per_pixel),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    } else {
        let (padded, padded_row_stride) =
            crate::infra::render::row_padding::pad_rows_rgba(
                &rgba, width, height,
            );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &padded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_stride as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    Arc::new(texture)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl Primitive for BackgroundPrimitive {
    type Pipeline = BackgroundRenderer;

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn prepare(
        &self,
        renderer: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let pipeline = &renderer.pipeline;
        let state = &mut renderer.state;

        if !state.default_texture_initialized {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: pipeline.default_texture.as_ref(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[0u8, 0u8, 0u8, 0u8],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: pipeline.default_ambient_texture.as_ref(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[18u8, 20u8, 24u8, 255u8],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            state.default_texture_initialized = true;
        }

        if state.globals_buffer.is_none() {
            let globals_buffer =
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Background Globals"),
                    size: GLOBALS_UNIFORM_SIZE,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

            let globals_bind_group =
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Background Globals Bind Group"),
                    layout: pipeline.globals_bind_group_layout.as_ref(),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals_buffer.as_entire_binding(),
                    }],
                });

            state.globals_buffer = Some(globals_buffer);
            state.globals_bind_group = Some(globals_bind_group);
        }

        if state.theater_uniform_buffer.is_none() {
            let theater_uniform_buffer =
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Theater Plate Uniforms"),
                    size: THEATER_PLATE_UNIFORM_SIZE,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

            state.theater_uniform_buffer = Some(theater_uniform_buffer);
        }

        if state.default_texture_bind_group.is_none()
            && let Some(theater_uniform_buffer) =
                state.theater_uniform_buffer.as_ref()
        {
            let default_texture_view = pipeline
                .default_texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let default_ambient_view = pipeline
                .default_ambient_texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let default_texture_bind_group =
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Default Background Texture Bind Group"),
                    layout: pipeline.texture_bind_group_layout.as_ref(),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: theater_uniform_buffer
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(
                                pipeline.sampler.as_ref(),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                &default_texture_view,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(
                                &default_ambient_view,
                            ),
                        },
                    ],
                });

            state.default_texture_bind_group = Some(default_texture_bind_group);
        }

        let backdrop_handle = self.backdrop_handle.as_ref();
        let backdrop_id = backdrop_handle.map(|handle| handle.id());
        let ambient_cache_key = self
            .theater_plate
            .as_ref()
            .map(|scene| scene.ambient.cache_key);

        if let Some(handle) = backdrop_handle {
            let image_id = handle.id();

            if !state.texture_cache.contains_key(&image_id)
                && let Some((texture, aspect_ratio)) =
                    load_texture(device, queue, handle)
            {
                state.texture_cache.insert(
                    image_id,
                    TextureInfo {
                        texture,
                        aspect_ratio,
                    },
                );
            }
        }

        if let Some(scene) = self.theater_plate.as_ref() {
            let cache_key = scene.ambient.cache_key;
            if !state.ambient_texture_cache.contains_key(&cache_key) {
                let texture =
                    load_ambient_texture(device, queue, &scene.ambient);
                state
                    .ambient_texture_cache
                    .insert(cache_key, AmbientTextureInfo { texture });
            }
        }

        let texture_bind_group_key = TextureBindGroupKey {
            backdrop_id: backdrop_id.clone(),
            ambient_cache_key,
        };
        if !state
            .texture_bind_groups
            .contains_key(&texture_bind_group_key)
            && let Some(theater_uniform_buffer) =
                state.theater_uniform_buffer.as_ref()
        {
            let backdrop_view = backdrop_id
                .as_ref()
                .and_then(|image_id| state.texture_cache.get(image_id))
                .map(|texture_info| {
                    texture_info
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default())
                })
                .unwrap_or_else(|| {
                    pipeline
                        .default_texture
                        .create_view(&wgpu::TextureViewDescriptor::default())
                });
            let ambient_view = ambient_cache_key
                .and_then(|cache_key| {
                    state.ambient_texture_cache.get(&cache_key)
                })
                .map(|texture_info| {
                    texture_info
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default())
                })
                .unwrap_or_else(|| {
                    pipeline
                        .default_ambient_texture
                        .create_view(&wgpu::TextureViewDescriptor::default())
                });

            let bind_group =
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Background Texture Bind Group"),
                    layout: pipeline.texture_bind_group_layout.as_ref(),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: theater_uniform_buffer
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(
                                pipeline.sampler.as_ref(),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                &backdrop_view,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(
                                &ambient_view,
                            ),
                        },
                    ],
                });
            state
                .texture_bind_groups
                .insert(texture_bind_group_key.clone(), bind_group);
        }

        let time = self.start_time.elapsed().as_secs_f32();
        log::trace!(
            "BackgroundPrimitive::prepare effect={:?} time={:.3} scroll_offset={} content_offset_px=({}, {}) viewport={}x{}",
            self.effect,
            time,
            self.scroll_offset,
            self.content_offset_px.x,
            self.content_offset_px.y,
            viewport.logical_size().width,
            viewport.logical_size().height
        );
        let effect_type = match &self.effect {
            BackgroundEffect::Solid => 0.0,
            BackgroundEffect::Gradient => 1.0,
            BackgroundEffect::SubtleNoise { .. } => 2.0,
            BackgroundEffect::FloatingParticles { .. } => 3.0,
            BackgroundEffect::WaveRipple { .. } => 4.0,
            BackgroundEffect::BackdropGradient => 5.0,
            BackgroundEffect::TheaterPlate => 6.0,
        };

        let (effect_param1, effect_param2) = match &self.effect {
            BackgroundEffect::Solid | BackgroundEffect::Gradient => (0.0, 0.0),
            BackgroundEffect::SubtleNoise { scale, speed } => (*scale, *speed),
            BackgroundEffect::FloatingParticles { count, size } => {
                (*count as f32, *size)
            }
            BackgroundEffect::WaveRipple {
                frequency,
                amplitude,
            } => (*frequency, *amplitude),
            BackgroundEffect::BackdropGradient
            | BackgroundEffect::TheaterPlate => {
                // For backdrop effects, effect_param2 holds aspect_mode (0.0 = Auto, 1.0 = Force21x9)
                let aspect_mode = match self.backdrop_aspect_mode {
                    BackdropAspectMode::Auto => 0.0,
                    BackdropAspectMode::Force21x9 => 1.0,
                };
                (0.0, aspect_mode)
            }
        };

        let transform: [f32; 16] = viewport.projection().into();

        let mut globals = Globals {
            transform,
            time_and_resolution: [
                time,
                0.0,
                viewport.logical_size().width,
                viewport.logical_size().height,
            ],
            scale_and_effect: [
                viewport.scale_factor(),
                effect_type,
                effect_param1,
                effect_param2,
            ],
            primary_color: [
                self.primary_color.r,
                self.primary_color.g,
                self.primary_color.b,
                self.primary_color.a,
            ],
            secondary_color: [
                self.secondary_color.r,
                self.secondary_color.g,
                self.secondary_color.b,
                self.secondary_color.a,
            ],
            texture_params: {
                // Calculate backdrop coverage in UV space (single source of truth)
                let viewport_width = viewport.logical_size().width;
                let viewport_height = viewport.logical_size().height;
                let display_aspect = match self.backdrop_aspect_mode {
                    BackdropAspectMode::Auto => {
                        if viewport_width >= viewport_height {
                            30.0 / 9.0 // Ultra-wide for wide windows
                        } else {
                            21.0 / 9.0 // 21:9 for tall windows
                        }
                    }
                    BackdropAspectMode::Force21x9 => 21.0 / 9.0,
                };
                let backdrop_height = viewport_width / display_aspect;
                let backdrop_coverage_uv =
                    (backdrop_height / viewport_height).min(1.0);

                [
                    self.backdrop_aspect_ratio.unwrap_or(1.0),
                    self.scroll_offset,
                    self.header_offset,
                    backdrop_coverage_uv, // Pre-calculated coverage for shader
                ]
            },
            content_offset_px: [
                self.content_offset_px.x,
                self.content_offset_px.y,
                0.0,
                0.0,
            ],
            prev_primary_color: [
                self.prev_primary_color.r,
                self.prev_primary_color.g,
                self.prev_primary_color.b,
                self.prev_primary_color.a,
            ],
            prev_secondary_color: [
                self.prev_secondary_color.r,
                self.prev_secondary_color.g,
                self.prev_secondary_color.b,
                self.prev_secondary_color.a,
            ],
            transition_params: [
                self.transition_progress,
                self.backdrop_opacity,
                self.backdrop_slide_offset,
                self.backdrop_scale,
            ],
            gradient_center: [
                self.gradient_center.0,
                self.gradient_center.1,
                0.0,
                0.0,
            ],
            depth_params: [
                self.depth_layout.regions.len().min(4) as f32,
                self.depth_layout.base_depth,
                self.depth_layout.shadow_intensity,
                self.depth_layout.shadow_distance,
            ],
            ambient_light: [
                self.depth_layout.ambient_light_direction.x,
                self.depth_layout.ambient_light_direction.y,
                0.0,
                0.0,
            ],
            region1_bounds: [0.0; 4],
            region1_depth_params: [0.0; 4],
            region1_shadow_params: [0.0; 4],
            region1_border_color: [0.0; 4],
            region2_bounds: [0.0; 4],
            region2_depth_params: [0.0; 4],
            region2_shadow_params: [0.0; 4],
            region2_border_color: [0.0; 4],
            region3_bounds: [0.0; 4],
            region3_depth_params: [0.0; 4],
            region3_shadow_params: [0.0; 4],
            region3_border_color: [0.0; 4],
            region4_bounds: [0.0; 4],
            region4_depth_params: [0.0; 4],
            region4_shadow_params: [0.0; 4],
            region4_border_color: [0.0; 4],
        };

        for (i, region) in self.depth_layout.regions.iter().take(4).enumerate()
        {
            let bounds = [
                region.bounds.x,
                region.bounds.y,
                region.bounds.width,
                region.bounds.height,
            ];

            let (edge_type, edge_width) = match region.edge_transition {
                EdgeTransition::Sharp => (0.0, 0.0),
                EdgeTransition::Soft { width } => (1.0, width),
                EdgeTransition::Beveled { width } => (2.0, width),
            };

            let depth_params = [
                region.depth,
                edge_type,
                edge_width,
                if region.shadow_enabled { 1.0 } else { 0.0 },
            ];

            let shadow_params = [
                region.shadow_intensity,
                region.z_order as f32,
                region.border.as_ref().map(|b| b.width).unwrap_or(0.0),
                region.border.as_ref().map(|b| b.opacity).unwrap_or(0.0),
            ];

            let border_color = if let Some(border) = &region.border {
                [
                    border.color.r,
                    border.color.g,
                    border.color.b,
                    border.color.a,
                ]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };

            match i {
                0 => {
                    globals.region1_bounds = bounds;
                    globals.region1_depth_params = depth_params;
                    globals.region1_shadow_params = shadow_params;
                    globals.region1_border_color = border_color;
                }
                1 => {
                    globals.region2_bounds = bounds;
                    globals.region2_depth_params = depth_params;
                    globals.region2_shadow_params = shadow_params;
                    globals.region2_border_color = border_color;
                }
                2 => {
                    globals.region3_bounds = bounds;
                    globals.region3_depth_params = depth_params;
                    globals.region3_shadow_params = shadow_params;
                    globals.region3_border_color = border_color;
                }
                3 => {
                    globals.region4_bounds = bounds;
                    globals.region4_depth_params = depth_params;
                    globals.region4_shadow_params = shadow_params;
                    globals.region4_border_color = border_color;
                }
                _ => {}
            }
        }

        if let Some(handle) = backdrop_handle {
            let image_id = handle.id();
            if let Some(texture_info) = state.texture_cache.get(&image_id) {
                globals.texture_params[0] = texture_info.aspect_ratio;
            }
        }

        if let Some(buffer) = state.globals_buffer.as_ref() {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[globals]));
        }

        let mut theater_uniforms = self
            .theater_plate
            .as_ref()
            .map(|scene| scene.uniforms)
            .unwrap_or_default();
        theater_uniforms.transition[0] = self.transition_progress;
        theater_uniforms.transition[1] = (theater_uniforms.transition[1]
            * self.backdrop_opacity)
            .clamp(0.0, 1.0);
        if let Some(buffer) = state.theater_uniform_buffer.as_ref() {
            queue.write_buffer(
                buffer,
                0,
                bytemuck::cast_slice(&[theater_uniforms]),
            );
        }

        let texture_bind_group = state
            .texture_bind_groups
            .get(&texture_bind_group_key)
            .cloned();

        state
            .primitive_data
            .insert(self.program_id, PrimitiveData { texture_bind_group });
    }

    fn draw(
        &self,
        renderer: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        let Some(globals_bind_group) =
            renderer.state.globals_bind_group.as_ref()
        else {
            return false;
        };

        render_pass.set_pipeline(&renderer.pipeline.render_pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);

        let bind_group = renderer
            .state
            .primitive_data
            .get(&self.program_id)
            .and_then(|data| data.texture_bind_group.as_ref());

        match bind_group {
            Some(group) => render_pass.set_bind_group(1, group, &[]),
            None => {
                let Some(default_group) =
                    renderer.state.default_texture_bind_group.as_ref()
                else {
                    return false;
                };
                render_pass.set_bind_group(1, default_group, &[]);
            }
        }

        render_pass.draw(0..4, 0..1);
        true
    }
}

/// Background shader widget
#[derive(Debug)]
pub struct BackgroundShader {
    effect: BackgroundEffect,
    theme: BackgroundTheme,
    quality: QualitySettings,
    primary_color: Color,
    secondary_color: Color,
    start_time: Instant,
    program_id: usize,
    scroll_offset: f32,
    content_offset_px: ContentOffsetPx,
    // Transition data
    prev_primary_color: Color,
    prev_secondary_color: Color,
    transition_progress: f32,
    backdrop_opacity: f32,
    backdrop_slide_offset: f32,
    backdrop_scale: f32,
    gradient_center: (f32, f32),
    backdrop_aspect_mode: BackdropAspectMode,
    backdrop_handle: Option<iced::widget::image::Handle>,
    backdrop_aspect_ratio: Option<f32>,
    theater_plate: Option<TheaterPlateScene>,
    // Depth layout for visual hierarchy
    depth_layout: DepthLayout,
    // Header offset for detail views
    header_offset: f32,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl BackgroundShader {
    /// Create a new background shader with default settings
    pub fn new() -> Self {
        let primary = MediaServerTheme::SOFT_GREY_DARK;
        let secondary = MediaServerTheme::SOFT_GREY_LIGHT;

        Self {
            effect: BackgroundEffect::Gradient,
            theme: BackgroundTheme::Cinematic,
            quality: QualitySettings::default(),
            primary_color: primary,
            secondary_color: secondary,
            start_time: Instant::now(),
            program_id: 0,
            scroll_offset: 0.0,
            content_offset_px: ContentOffsetPx::default(),
            // Initialize transitions
            prev_primary_color: primary,
            prev_secondary_color: secondary,
            transition_progress: 1.0,
            backdrop_opacity: 1.0,
            backdrop_slide_offset: 0.0,
            backdrop_scale: 1.0,
            gradient_center: generate_random_gradient_center(),
            backdrop_handle: None,
            backdrop_aspect_ratio: None,
            theater_plate: None,
            depth_layout: DepthLayout {
                regions: Vec::new(),
                ambient_light_direction: iced::Vector::new(0.707, 0.707), // Light from bottom-right
                base_depth: 0.0,
                shadow_intensity: 0.4,
                shadow_distance: 40.0,
            },
            header_offset: 0.0,
            backdrop_aspect_mode: BackdropAspectMode::Auto,
        }
    }

    /// Set the background effect
    pub fn effect(mut self, effect: BackgroundEffect) -> Self {
        self.effect = effect;
        self
    }

    /// Set the background theme
    pub fn theme(mut self, theme: BackgroundTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Set the quality settings
    pub fn quality(mut self, quality: QualitySettings) -> Self {
        self.quality = quality;
        self
    }

    /// Set custom colors
    pub fn colors(mut self, primary: Color, secondary: Color) -> Self {
        self.primary_color = primary;
        self.secondary_color = secondary;
        self
    }

    /// Set the scroll offset for fixed backdrop positioning
    pub fn scroll_offset(mut self, offset: f32) -> Self {
        self.scroll_offset = offset;
        self
    }

    /// Set the content-space offset (in logical pixels) used to anchor noise to scroll input.
    pub fn content_offset_px(mut self, offset: ContentOffsetPx) -> Self {
        self.content_offset_px = offset;
        self
    }

    /// Set the stable program id for this background shader instance.
    pub fn program_id(mut self, id: usize) -> Self {
        self.program_id = id;
        self
    }

    /// Set the stable start time for time-based effects.
    pub fn start_time(mut self, start_time: Instant) -> Self {
        self.start_time = start_time;
        self
    }

    /// Set the header offset for detail views
    pub fn header_offset(mut self, offset: f32) -> Self {
        self.header_offset = offset;
        self
    }

    /// Set the gradient center position
    pub fn gradient_center(mut self, center: (f32, f32)) -> Self {
        self.gradient_center = center;
        self
    }

    /// Set the backdrop image to be composited by the shader program.
    pub fn backdrop(mut self, handle: iced::widget::image::Handle) -> Self {
        self.backdrop_handle = Some(handle);
        self
    }

    /// Set the Theater Plate layer stack inputs.
    pub fn theater_plate(mut self, scene: TheaterPlateScene) -> Self {
        self.theater_plate = Some(scene);
        self
    }

    /// Provide a fallback aspect ratio for the backdrop when precise metadata isn’t available yet.
    pub fn backdrop_aspect_ratio(mut self, ratio: Option<f32>) -> Self {
        self.backdrop_aspect_ratio = ratio;
        self
    }

    /// Set the backdrop aspect mode
    pub fn backdrop_aspect_mode(mut self, mode: BackdropAspectMode) -> Self {
        self.backdrop_aspect_mode = mode;
        self
    }

    /// Set depth regions for visual hierarchy
    pub fn with_depth_layout(mut self, layout: DepthLayout) -> Self {
        self.depth_layout = layout;
        self
    }

    /// Set colors from media theme color
    pub fn media_colors(mut self, theme_color: Option<Color>) -> Self {
        if let Some(color) = theme_color {
            // Primary color is the theme color
            self.primary_color = color;

            // Secondary color is a lighter, more saturated variant
            let r = color.r;
            let g = color.g;
            let b = color.b;

            // Increase brightness and saturation slightly for secondary
            let secondary = Color::from_rgb(
                (r * 1.2).min(1.0),
                (g * 1.2).min(1.0),
                (b * 1.2).min(1.0),
            );

            self.secondary_color = secondary;
        } else {
            // Fallback to default theme colors
            use crate::domains::ui::theme::MediaServerTheme;
            use crate::infra::theme::{accent, with_alpha};
            self.primary_color = MediaServerTheme::BLACK;
            self.secondary_color = with_alpha(accent(), 0.2);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn uniform_structs_match_wgsl_layout_sizes() {
        assert_eq!(size_of::<Globals>(), GLOBALS_UNIFORM_SIZE as usize);
        assert_eq!(align_of::<Globals>(), 4);
        assert_eq!(
            size_of::<TheaterPlateUniforms>(),
            THEATER_PLATE_UNIFORM_SIZE as usize
        );
        assert_eq!(align_of::<TheaterPlateUniforms>(), 4);

        assert_eq!(offset_of!(TheaterPlateUniforms, base_stage), 0);
        assert_eq!(offset_of!(TheaterPlateUniforms, ambient_field), 16);
        assert_eq!(offset_of!(TheaterPlateUniforms, focused_plate), 32);
        assert_eq!(offset_of!(TheaterPlateUniforms, plate_mask), 48);
        assert_eq!(offset_of!(TheaterPlateUniforms, scrim_masks), 64);
        assert_eq!(offset_of!(TheaterPlateUniforms, hero_art_rect), 80);
        assert_eq!(offset_of!(TheaterPlateUniforms, vignette_grain), 96);
        assert_eq!(offset_of!(TheaterPlateUniforms, highlight_grade), 112);
        assert_eq!(offset_of!(TheaterPlateUniforms, transition), 128);
    }

    #[test]
    fn theater_plate_soft_plate_mask_has_feathered_edges() {
        let resolution = [1920.0, 1080.0];
        let center = [0.5, 0.5];
        let half = [0.25, 0.20];
        let radius = 32.0;
        let feather = 80.0;

        let inside = theater_plate_soft_rect_alpha(
            [0.5, 0.5],
            center,
            half,
            radius,
            feather,
            resolution,
        );
        let edge = theater_plate_soft_rect_alpha(
            [0.5 + half[0] + 20.0 / resolution[0], 0.5],
            center,
            half,
            radius,
            feather,
            resolution,
        );
        let outside = theater_plate_soft_rect_alpha(
            [0.5 + half[0] + 96.0 / resolution[0], 0.5],
            center,
            half,
            radius,
            feather,
            resolution,
        );

        assert!(inside > 0.99);
        assert!((0.0..1.0).contains(&edge), "edge alpha was {edge}");
        assert!(outside < 0.05);
        assert!(inside > edge && edge > outside);
    }

    #[test]
    fn theater_plate_lobe_mask_uses_gradient_scene_shadows() {
        let center = [0.58, 0.42];
        let half = [0.28, 0.22];
        let center_alpha = theater_plate_lobe_alpha(center, center, half, 0.64);
        let shoulder_alpha = theater_plate_lobe_alpha(
            [center[0] + half[0] * 0.92, center[1]],
            center,
            half,
            0.64,
        );
        let corner_alpha = theater_plate_lobe_alpha(
            [center[0] + half[0] * 0.92, center[1] + half[1] * 0.92],
            center,
            half,
            0.64,
        );
        let outside_alpha = theater_plate_lobe_alpha(
            [center[0] + half[0] * 1.95, center[1] + half[1] * 1.50],
            center,
            half,
            0.64,
        );

        assert!(center_alpha > 0.90);
        assert!((0.0..1.0).contains(&shoulder_alpha));
        assert!(corner_alpha < shoulder_alpha);
        assert!(outside_alpha < 0.10);
    }

    #[test]
    fn theater_plate_geometry_applies_layout_controls_to_scene() {
        let scene = TheaterPlateScene::fallback_from_colors(
            42,
            Color::from_rgb(0.1, 0.2, 0.3),
            Color::from_rgb(0.5, 0.4, 0.3),
        )
        .with_geometry(TheaterPlateGeometry {
            focused_plate: [0.64, 0.44, 0.26, 0.18],
            plate_mask: [0.72, 52.0, 144.0, 0.58],
            scrim_masks: [0.70, 0.16, 0.44, 0.58],
            hero_art_rect: [0.12, 0.18, 0.24, 0.56],
            ambient_opacity_scale: 0.7,
            vignette_opacity: 0.62,
            grain_opacity_scale: 1.2,
            backdrop_opacity: 0.0,
        });

        assert_eq!(scene.uniforms.focused_plate, [0.64, 0.44, 0.26, 0.18]);
        assert_eq!(scene.uniforms.plate_mask[1], 52.0);
        assert_eq!(scene.uniforms.plate_mask[2], 144.0);
        assert_eq!(scene.uniforms.plate_mask[3], 0.58);
        assert!(scene.uniforms.plate_mask[0] >= 0.72);
        assert!(scene.uniforms.scrim_masks[0] >= 0.70);
        assert_eq!(scene.uniforms.hero_art_rect, [0.12, 0.18, 0.24, 0.56]);
        assert!(scene.uniforms.ambient_field[0] < 0.52);
        assert_eq!(scene.uniforms.transition[1], 0.0);
    }

    #[test]
    fn theater_plate_backdrop_band_fades_instead_of_cutting_hard_edges() {
        let top = 0.08;
        let bottom = 0.52;
        let feather = 0.08;

        let before = soft_vertical_band_alpha(top - 0.02, top, bottom, feather);
        let entering =
            soft_vertical_band_alpha(top + 0.03, top, bottom, feather);
        let middle = soft_vertical_band_alpha(0.30, top, bottom, feather);
        let leaving =
            soft_vertical_band_alpha(bottom - 0.03, top, bottom, feather);
        let after =
            soft_vertical_band_alpha(bottom + 0.02, top, bottom, feather);

        assert_eq!(before, 0.0);
        assert!((0.0..1.0).contains(&entering));
        assert!(middle > 0.99);
        assert!((0.0..1.0).contains(&leaving));
        assert_eq!(after, 0.0);
    }
}

impl Default for BackgroundShader {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to create a background shader widget
pub fn background_shader() -> BackgroundShader {
    BackgroundShader::new()
}

impl<'a> From<BackgroundShader> for Element<'a, UiMessage> {
    fn from(background: BackgroundShader) -> Self {
        iced::widget::shader(BackgroundShaderProgram {
            effect: background.effect,
            theme: background.theme,
            quality: background.quality,
            primary_color: background.primary_color,
            secondary_color: background.secondary_color,
            start_time: background.start_time,
            content_offset_px: background.content_offset_px,
            scroll_offset: background.scroll_offset,
            // Pass through transition data
            prev_primary_color: background.prev_primary_color,
            prev_secondary_color: background.prev_secondary_color,
            transition_progress: background.transition_progress,
            backdrop_opacity: background.backdrop_opacity,
            backdrop_slide_offset: background.backdrop_slide_offset,
            backdrop_scale: background.backdrop_scale,
            gradient_center: background.gradient_center,
            backdrop_handle: background.backdrop_handle,
            depth_layout: background.depth_layout,
            header_offset: background.header_offset,
            backdrop_aspect_mode: background.backdrop_aspect_mode,
            backdrop_aspect_ratio: background.backdrop_aspect_ratio,
            theater_plate: background.theater_plate,
            id: background.program_id,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
