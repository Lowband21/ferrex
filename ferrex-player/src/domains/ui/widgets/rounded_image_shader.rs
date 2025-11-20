//! Shader-based rounded image widget for Iced
//!
//! This implementation uses GPU shaders for true rounded rectangle clipping
//! with anti-aliasing, providing better performance than Canvas-based approaches.

pub mod rounded_image_batch_state;

use crate::domains::ui::messages::Message;

use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::Viewport;
use iced::wgpu;
use iced::widget::image::Handle;
use iced::widget::shader::Primitive;
use iced::widget::shader::Program;
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, mouse};
use iced_wgpu::AtlasRegion;
use iced_wgpu::primitive::{
    BatchEncodeContext, BatchPrimitive, PrimitiveBatchState, register_batchable_type,
};
use rounded_image_batch_state::{PendingPrimitive, RoundedImageBatchState, RoundedImageInstance};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Dynamic bounds for animated posters
#[derive(Debug, Clone, Copy)]
pub struct AnimatedPosterBounds {
    /// Base size of the poster
    pub base_width: f32,
    pub base_height: f32,
    /// Extra horizontal padding for animation overflow (e.g., scale and shadows)
    pub horizontal_padding: f32,
    /// Extra vertical padding for animation overflow
    pub vertical_padding: f32,
    /// Global UI scale factor for DPI independence
    pub ui_scale_factor: f32,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl AnimatedPosterBounds {
    /// Create new bounds with default padding
    pub fn new(width: f32, height: f32) -> Self {
        use crate::infrastructure::constants::animation;

        // Calculate padding using centralized constants
        //let horizontal_padding = animation::calculate_horizontal_padding(width);
        let horizontal_padding = animation::calculate_horizontal_padding(width);
        let vertical_padding = animation::calculate_vertical_padding(height);

        Self {
            base_width: width,
            base_height: height,
            horizontal_padding,
            vertical_padding,
            ui_scale_factor: 1.0,
        }
    }

    /// Get the layout bounds - includes padding for effects
    pub fn layout_bounds(&self) -> (f32, f32) {
        // Return size with padding included - this is what the layout system sees
        (
            (self.base_width + self.horizontal_padding * 2.0) * self.ui_scale_factor,
            (self.base_height + self.vertical_padding * 2.0) * self.ui_scale_factor,
        )
    }

    /// Get the render bounds including animation overflow space
    pub fn render_bounds(&self) -> Rectangle {
        // Center the base bounds within the padded area
        Rectangle {
            x: -self.horizontal_padding,
            y: -self.vertical_padding,
            width: self.base_width + (self.horizontal_padding * 2.0),
            height: self.base_height + (self.vertical_padding * 2.0),
        }
    }
}

// Image loading functions are in the image crate root

/// Animation type for poster loading
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationType {
    None,
    Fade {
        duration: Duration,
    },
    /// The enhanced flip is now the default and only flip variant
    Flip {
        total_duration: Duration,
        rise_end: f32,   // Phase end: 0.0-0.25
        emerge_end: f32, // Phase end: 0.25-0.5
        flip_end: f32,   // Phase end: 0.5-0.75
                         // Settle: 0.75-1.0
    },
    /// Special state for placeholders - shows backface in sunken state
    PlaceholderSunken,
}

impl AnimationType {
    fn as_u32(&self) -> u32 {
        match self {
            AnimationType::None => 0,
            AnimationType::Fade { .. } => 1,
            AnimationType::Flip { .. } => 2,
            AnimationType::PlaceholderSunken => 3,
        }
    }

    /// Create default flip animation with standard timings
    pub fn flip() -> Self {
        use crate::infrastructure::constants::animation;

        AnimationType::Flip {
            total_duration: Duration::from_millis(animation::DEFAULT_DURATION_MS),
            rise_end: 0.10,
            emerge_end: 0.20,
            flip_end: 0.80,
        }
    }

    fn effective_duration(&self) -> Duration {
        match self {
            AnimationType::None | AnimationType::PlaceholderSunken => Duration::ZERO,
            AnimationType::Fade { duration } => *duration,
            AnimationType::Flip { total_duration, .. } => *total_duration,
        }
    }
}

/// Describes how poster animations should behave across the first and subsequent renders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationBehavior {
    first: AnimationType,
    repeat: AnimationType,
    fresh_window: Duration,
}

impl AnimationBehavior {
    /// Always use the same animation for every render.
    pub fn constant(animation: AnimationType) -> Self {
        let window = (animation.effective_duration() * 2)
            .max(Duration::from_millis(50))
            .max(Duration::from_secs(10));
        Self {
            first: animation,
            repeat: animation,
            fresh_window: window,
        }
    }

    /// Use `first` for freshly loaded textures, then fall back to `repeat` after the window.
    pub fn first_then(first: AnimationType, repeat: AnimationType) -> Self {
        let window = std::cmp::max(first.effective_duration(), repeat.effective_duration())
            .saturating_mul(2)
            .max(Duration::from_millis(50))
            .max(Duration::from_secs(10));
        Self {
            first,
            repeat,
            fresh_window: window,
        }
    }

    /// Convenience for highlighting newly added media: flip once, then fade as normal.
    pub fn flip_then_fade() -> Self {
        Self::first_then(
            AnimationType::flip(),
            AnimationType::Fade {
                duration: Duration::from_millis(
                    crate::infrastructure::constants::layout::animation::TEXTURE_FADE_DURATION_MS,
                ),
            },
        )
    }

    /// Derive a behavior from a single animation intent.
    ///
    /// Flip animations degrade to flip-then-fade, other animations stay constant.
    pub fn from_primary(animation: AnimationType) -> Self {
        match animation {
            AnimationType::Flip { .. } | AnimationType::Fade { .. } => Self::flip_then_fade(),
            _ => Self::constant(animation),
        }
    }

    /// Select which animation should run given when the texture finished loading.
    pub fn select(&self, loaded_at: Option<Instant>) -> AnimationType {
        if let Some(loaded_at) = loaded_at
            && loaded_at.elapsed() <= self.fresh_window
        {
            return self.first;
        }
        self.repeat
    }
}

static BATCH_REGISTRATION: OnceLock<()> = OnceLock::new();

fn ensure_batch_registration() {
    BATCH_REGISTRATION.get_or_init(|| {
        register_batchable_type::<RoundedImagePrimitive>();
    });
}

/// A shader program for rendering rounded images
#[derive(Debug, Clone)]
pub struct RoundedImageProgram {
    pub id: u64,
    pub handle: Handle,
    pub radius: f32,
    pub animation: AnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub progress: Option<f32>,
    pub progress_color: Color,
    pub on_play: Option<Message>,
    pub on_edit: Option<Message>,
    pub on_options: Option<Message>,
    pub on_click: Option<Message>,
}

/// State for tracking mouse position within the shader widget
#[derive(Debug, Clone, Default)]
pub struct RoundedImageState {
    /// Current mouse position relative to widget bounds
    pub mouse_position: Option<Point>,
    /// Whether mouse is over the widget
    pub is_hovered: bool,
}

impl Program<Message> for RoundedImageProgram {
    type State = RoundedImageState;
    type Primitive = RoundedImagePrimitive;

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
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        ensure_batch_registration();

        // Use mouse position from state instead of cursor
        let mouse_position = state.mouse_position;

        /*
        log::info!(
            "RoundedImageProgram::draw called - state hover: {}, mouse_pos: {:?}",
            state.is_hovered,
            mouse_position
        ); */

        RoundedImagePrimitive {
            id: self.id,
            handle: self.handle.clone(),
            bounds,
            radius: self.radius,
            animation: self.animation,
            load_time: self.load_time,
            opacity: self.opacity,
            theme_color: self.theme_color,
            animated_bounds: self.bounds,
            is_hovered: self.is_hovered,
            mouse_position,
            progress: self.progress,
            progress_color: self.progress_color,
        }
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<Message>> {
        if let Event::Mouse(mouse_event) = event {
            //log::info!("Shader widget received mouse event: {:?}", mouse_event);

            match mouse_event {
                mouse::Event::CursorMoved { .. } => {
                    // Check if cursor position is available
                    if let Some(position) = cursor.position() {
                        if bounds.contains(position) {
                            // Convert to relative position within widget
                            let relative_pos =
                                Point::new(position.x - bounds.x, position.y - bounds.y);

                            let was_hovered = state.is_hovered;
                            state.mouse_position = Some(relative_pos);
                            state.is_hovered = true;

                            // Always request redraw when mouse state changes
                            return Some(iced::widget::Action::request_redraw());
                        } else {
                            // Mouse outside widget bounds
                            let was_hovered = state.is_hovered;
                            state.mouse_position = None;
                            state.is_hovered = false;

                            // Request redraw if state changed
                            if was_hovered {
                                return Some(iced::widget::Action::request_redraw());
                            }
                        }
                    } else {
                        // No cursor position available (cursor left window)
                        // Clear any stale mouse state
                        if state.is_hovered || state.mouse_position.is_some() {
                            state.mouse_position = None;
                            state.is_hovered = false;
                            return Some(iced::widget::Action::request_redraw());
                        }
                    }
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    // First verify cursor is actually within widget bounds
                    if let Some(cursor_pos) = cursor.position() {
                        if !bounds.contains(cursor_pos) {
                            // Click is outside widget bounds, ignore it
                            return None;
                        }
                    } else {
                        // No cursor position available, ignore click
                        return None;
                    }

                    // Verify state mouse position matches current cursor position
                    // This handles cases where the app lost/regained focus
                    if let Some(cursor_pos) = cursor.position() {
                        let current_relative =
                            Point::new(cursor_pos.x - bounds.x, cursor_pos.y - bounds.y);

                        // Update state if mouse position is stale
                        if let Some(old_pos) = state.mouse_position {
                            let delta = old_pos - current_relative;
                            let distance = (delta.x * delta.x + delta.y * delta.y).sqrt();
                            if distance > 1.0 {
                                state.mouse_position = Some(current_relative);
                            }
                        } else {
                            state.mouse_position = Some(current_relative);
                        }
                    }

                    // Handle click events based on mouse position
                    if let Some(mouse_pos) = state.mouse_position {
                        //log::debug!("Click in widget - cursor_pos: {:?}, widget bounds: {:?}, relative mouse_pos: {:?}",
                        //    cursor.position(), bounds, mouse_pos);

                        // Normalize mouse position to 0-1 range
                        let norm_x = mouse_pos.x / bounds.width;
                        let norm_y = mouse_pos.y / bounds.height;

                        // Check which button was clicked
                        // Center play button (circle with 8% radius at center)
                        // Note: Unlike shader, we don't need aspect ratio adjustment in click detection
                        // because norm_x and norm_y are already normalized to widget bounds
                        let center_x = 0.5;
                        let center_y = 0.5;
                        let radius = 0.08;
                        let dist_from_center =
                            ((norm_x - center_x).powi(2) + (norm_y - center_y).powi(2)).sqrt();
                        if dist_from_center <= radius {
                            if let Some(on_play) = &self.on_play {
                                log::debug!("Play button clicked!");
                                return Some(iced::widget::Action::publish(on_play.clone()));
                            }
                        }
                        // Top-right edit button (radius 0.06 at 0.85, 0.15)
                        else if (0.79..=0.91).contains(&norm_x) && (0.09..=0.21).contains(&norm_y)
                        {
                            if let Some(on_edit) = &self.on_edit {
                                log::debug!("Edit button clicked!");
                                return Some(iced::widget::Action::publish(on_edit.clone()));
                            }
                        }
                        // Bottom-right options button (radius 0.06 at 0.85, 0.85)
                        else if (0.79..=0.91).contains(&norm_x) && (0.79..=0.91).contains(&norm_y)
                        {
                            if let Some(on_options) = &self.on_options {
                                log::debug!("Options button clicked!");
                                return Some(iced::widget::Action::publish(on_options.clone()));
                            }
                        }
                        // Empty space - trigger on_click
                        else if let Some(on_click) = &self.on_click {
                            log::debug!("Empty space clicked!");
                            return Some(iced::widget::Action::publish(on_click.clone()));
                        }
                    }
                }
                mouse::Event::CursorEntered => {
                    // Handle cursor entering the widget
                    if let Some(position) = cursor.position()
                        && bounds.contains(position)
                    {
                        let relative_pos = Point::new(position.x - bounds.x, position.y - bounds.y);
                        state.mouse_position = Some(relative_pos);
                        state.is_hovered = true;
                        //log::debug!("Cursor entered widget at: {:?}", relative_pos);
                    }
                }
                mouse::Event::CursorLeft => {
                    // Clear mouse position when cursor leaves
                    state.mouse_position = None;
                    state.is_hovered = false;
                    log::debug!("Cursor left widget");
                }
                _ => {}
            }
        }

        None
    }
}

/// The primitive that actually renders the rounded image
#[derive(Debug, Clone)]
pub struct RoundedImagePrimitive {
    pub id: u64,
    pub handle: Handle,
    pub bounds: Rectangle,
    pub radius: f32,
    pub animation: AnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub animated_bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub mouse_position: Option<Point>, // Mouse position relative to widget
    pub progress: Option<f32>,
    pub progress_color: Color,
}

impl RoundedImagePrimitive {
    fn set_load_time(&mut self, load_time: Instant) {
        self.load_time = Some(load_time);
    }
}

/// Global uniform data (viewport transform)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16], // 4x4 matrix = 64 bytes
    scale_factor: f32,    // 4 bytes
    _padding: [f32; 7],   // Padding to make total 96 bytes (28 bytes padding)
}

/// Instance data for each rounded image
/// Packed into vec4s to reduce vertex attribute count
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    // vec4: position.xy, size.xy
    position_and_size: [f32; 4],
    // vec4: radius, opacity, rotation_y, animation_progress
    radius_opacity_rotation_anim: [f32; 4],
    // vec4: theme_color.rgb, z_depth
    theme_color_zdepth: [f32; 4],
    // vec4: scale, shadow_intensity, border_glow, animation_type
    scale_shadow_glow_type: [f32; 4],
    // vec4: is_hovered, show_overlay, show_border, progress
    hover_overlay_border_progress: [f32; 4],
    // vec4: mouse_position.xy, unused, unused
    mouse_pos_and_padding: [f32; 4],
    // vec4: progress_color.rgb, unused
    progress_color_and_padding: [f32; 4],
    // vec4: atlas_uv_min.xy, atlas_uv_max.xy
    atlas_uvs: [f32; 4],
    // vec4: atlas_layer, unused, unused, unused
    atlas_layer_and_padding: [f32; 4],
}

/// Pipeline state (immutable after creation)
#[allow(dead_code)]
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    atlas_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
}

/// Per-primitive render data
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
struct PrimitiveData {
    instance_buffer: wgpu::Buffer,
}

/// Batched render data for all primitives in a frame
#[allow(dead_code)]
struct BatchedData {
    instance_buffer: Option<wgpu::Buffer>,
    instances: Vec<Instance>, // Accumulate instances across prepare calls
}

/// Shared state for all rounded images
///
/// Batching Strategy:
/// - Multiple prepare_batched calls accumulate instances
/// - Single render call draws all instances at once
/// - Instances are cleared after render for next frame
#[derive(Default)]
#[allow(dead_code)]
struct State {
    // Globals buffer and bind group (shared by all)
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    // Per-primitive data for current frame
    primitive_data: HashMap<usize, PrimitiveData>,
    // Batched data for current frame
    //batch: BatchedData,
    // Track which primitives we've seen this frame
    //prepared_primitives: HashSet<usize>,
}

impl Pipeline {
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        log::debug!("Creating rounded image shader pipeline");

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rounded Image Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/rounded_image.wgsl").into()),
        });

        // Create globals bind group layout (includes sampler)
        log::debug!("Creating globals bind group layout");
        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Globals"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(std::num::NonZeroU64::new(96).unwrap()),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Create atlas bind group layout to match iced's atlas exactly
        let atlas_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("iced_wgpu::image texture atlas layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                }],
            });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rounded Image Pipeline Layout"),
            bind_group_layouts: &[&globals_bind_group_layout, &atlas_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create vertex buffer layout for instance data
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // position_and_size: vec4
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // radius_opacity_rotation_anim: vec4
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // theme_color_zdepth: vec4
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // scale_shadow_glow_type: vec4
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // hover_overlay_border_progress: vec4
                wgpu::VertexAttribute {
                    offset: 64,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // mouse_pos_and_padding: vec4
                wgpu::VertexAttribute {
                    offset: 80,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // progress_color_and_padding: vec4
                wgpu::VertexAttribute {
                    offset: 96,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // atlas_uvs: vec4
                wgpu::VertexAttribute {
                    offset: 112,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // atlas_layer_and_padding: vec4
                wgpu::VertexAttribute {
                    offset: 128,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        // Create render pipeline
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rounded Image Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rounded Image Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Pipeline {
            render_pipeline,
            atlas_bind_group_layout: Arc::new(atlas_bind_group_layout),
            globals_bind_group_layout: Arc::new(globals_bind_group_layout),
            sampler: Arc::new(sampler),
        }
    }
}

/// Helper function to create an instance from image data
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub(super) fn create_batch_instance(
    atlas_region: Option<AtlasRegion>,
    bounds: &Rectangle,
    radius: f32,
    animation: AnimationType,
    load_time: Option<&Instant>,
    opacity: f32,
    theme_color: Color,
    animated_bounds: Option<&AnimatedPosterBounds>,
    is_hovered: bool,
    mouse_position: Option<Point>,
    progress: Option<f32>,
    progress_color: Color,
) -> rounded_image_batch_state::RoundedImageInstance {
    // Extract UV coordinates and layer from the atlas entry
    let (uv_min, uv_max, layer) = if let Some(region) = atlas_region {
        (region.uv_min, region.uv_max, region.layer)
    } else {
        // Use out-of-range UVs to signal placeholder/invalid to the shader.
        ([-1.0, -1.0], [-1.0, -1.0], 0)
    };

    // Calculate animation state

    let (
        actual_opacity,
        rotation_y,
        animation_progress,
        z_depth,
        scale,
        shadow_intensity,
        border_glow,
    ) = if let Some(load_time) = load_time {
        let elapsed = load_time.elapsed();
        let animation = match animation {
            AnimationType::Flip {
                total_duration,
                emerge_end,
                flip_end,
                rise_end,
            } => {
                if elapsed > total_duration {
                    AnimationType::None
                } else {
                    AnimationType::Flip {
                        total_duration,
                        emerge_end,
                        flip_end,
                        rise_end,
                    }
                }
            }
            anim => anim,
        };

        calculate_animation_state(animation, elapsed, opacity)
    } else {
        (
            0.7_f32,
            std::f32::consts::PI,
            0.0f32,
            -10.0f32,
            1.0f32,
            0.0f32,
            0.0f32,
        )
    };

    // Calculate poster position and size
    let (poster_position, poster_size) = if let Some(animated_bounds) = animated_bounds {
        let offset_x = (bounds.width - animated_bounds.base_width) / 2.0;
        let offset_y = (bounds.height - animated_bounds.base_height) / 2.0;
        let poster_x = bounds.x + offset_x;
        let poster_y = bounds.y + offset_y;
        (
            [poster_x, poster_y],
            [animated_bounds.base_width, animated_bounds.base_height],
        )
    } else {
        let border_padding = 3.0;
        let poster_x = bounds.x + border_padding;
        let poster_y = bounds.y + border_padding;
        let poster_width = bounds.width - (border_padding * 2.0);
        let poster_height = bounds.height - (border_padding * 2.0);
        ([poster_x, poster_y], [poster_width, poster_height])
    };

    // Calculate overlay state
    let animation_complete = match animation {
        AnimationType::None => true,
        AnimationType::PlaceholderSunken => true,
        _ => animation_progress >= 0.999,
    };

    let show_overlay = if is_hovered && animation_complete {
        1.0
    } else {
        0.0
    };
    let show_border = 1.0; // Always show border

    // Calculate mouse position
    let mouse_pos_normalized = if let Some(mouse_pos) = mouse_position {
        let scaled_poster_width = poster_size[0] * scale;
        let scaled_poster_height = poster_size[1] * scale;
        let widget_to_poster_offset_x = if animated_bounds.is_some() {
            (bounds.width - scaled_poster_width) / 2.0
        } else {
            0.0
        };
        let widget_to_poster_offset_y = if animated_bounds.is_some() {
            (bounds.height - scaled_poster_height) / 2.0
        } else {
            0.0
        };
        let mouse_x_relative = mouse_pos.x - widget_to_poster_offset_x;
        let mouse_y_relative = mouse_pos.y - widget_to_poster_offset_y;
        let norm_x = mouse_x_relative / scaled_poster_width;
        let norm_y = mouse_y_relative / scaled_poster_height;

        if (-0.01..=1.01).contains(&norm_x) && (-0.01..=1.01).contains(&norm_y) {
            [norm_x.clamp(0.0, 1.0), norm_y.clamp(0.0, 1.0)]
        } else {
            [-1.0, -1.0]
        }
    } else {
        [-1.0, -1.0]
    };

    // Create instance data
    RoundedImageInstance {
        position_and_size: [
            poster_position[0],
            poster_position[1],
            poster_size[0],
            poster_size[1],
        ],
        radius_opacity_rotation_anim: [radius, actual_opacity, rotation_y, animation_progress],
        theme_color_zdepth: [theme_color.r, theme_color.g, theme_color.b, z_depth],
        scale_shadow_glow_type: [
            scale,
            shadow_intensity,
            border_glow,
            animation.as_u32() as f32,
        ],
        hover_overlay_border_progress: [
            if is_hovered { 1.0 } else { 0.0 },
            show_overlay,
            show_border,
            progress.unwrap_or(-1.0),
        ],
        mouse_pos_and_padding: [mouse_pos_normalized[0], mouse_pos_normalized[1], 0.0, 0.0],
        progress_color_and_padding: [progress_color.r, progress_color.g, progress_color.b, 0.0],
        atlas_uvs: [uv_min[0], uv_min[1], uv_max[0], uv_max[1]],
        atlas_layer_and_padding: [layer as f32, 0.0, 0.0, 0.0],
    }
}
/// Helper function to create an instance from image data
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub(super) fn create_placeholder_instance(
    bounds: &Rectangle,
    radius: f32,
    theme_color: Color,
    animated_bounds: Option<&AnimatedPosterBounds>,
    progress: Option<f32>,
    progress_color: Color,
) -> rounded_image_batch_state::RoundedImageInstance {
    let (
        actual_opacity,
        rotation_y,
        animation_progress,
        z_depth,
        scale,
        shadow_intensity,
        border_glow,
    ) = (
        0.7_f32,
        std::f32::consts::PI,
        0.0f32,
        -10.0f32,
        1.0f32,
        0.0f32,
        0.0f32,
    );

    // Calculate poster position and size
    let (poster_position, poster_size) =
        /*if let Some(animated_bounds) = animated_bounds {
        let offset_x = (bounds.width - animated_bounds.base_width) / 2.0;
        let offset_y = (bounds.height - animated_bounds.base_height) / 2.0;
        let poster_x = bounds.x + offset_x;
        let poster_y = bounds.y + offset_y;
        (
            [poster_x, poster_y],
            [animated_bounds.base_width, animated_bounds.base_height],
        )
    } else {*/
    {
        let border_padding = 3.0;
        let poster_x = bounds.x + border_padding;
        let poster_y = bounds.y + border_padding;
        let poster_width = bounds.width - (border_padding * 2.0);
        let poster_height = bounds.height - (border_padding * 2.0);
        ([poster_x, poster_y], [poster_width, poster_height])
    };

    let show_overlay = 0.0;
    let show_border = 1.0; // Always show border

    // Create instance data
    RoundedImageInstance {
        position_and_size: [
            poster_position[0],
            poster_position[1],
            poster_size[0],
            poster_size[1],
        ],
        radius_opacity_rotation_anim: [radius, actual_opacity, rotation_y, animation_progress],
        theme_color_zdepth: [theme_color.r, theme_color.g, theme_color.b, z_depth],
        scale_shadow_glow_type: [scale, shadow_intensity, border_glow, 0.0],
        hover_overlay_border_progress: [0.0, show_overlay, show_border, progress.unwrap_or(-1.0)],
        mouse_pos_and_padding: [0.0, 0.0, 0.0, 0.0],
        progress_color_and_padding: [progress_color.r, progress_color.g, progress_color.b, 0.0],
        atlas_uvs: [-1.0, -1.0, -1.0, -1.0],
        atlas_layer_and_padding: [0.0, 0.0, 0.0, 0.0],
    }
}

/// Helper function to calculate animation state
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn calculate_animation_state(
    animation: AnimationType,
    elapsed: Duration,
    opacity: f32,
) -> (f32, f32, f32, f32, f32, f32, f32) {
    match animation {
        AnimationType::None => (opacity, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0),
        AnimationType::PlaceholderSunken => (0.7, std::f32::consts::PI, 0.0, -10.0, 1.0, 0.0, 0.0),
        AnimationType::Fade { duration } => {
            let progress = (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0);
            (opacity * progress, 0.0, progress, 0.0, 1.0, 0.0, 0.0)
        }
        AnimationType::Flip {
            total_duration,
            rise_end,
            emerge_end,
            flip_end,
        } => {
            let overall_progress = (elapsed.as_secs_f32() / total_duration.as_secs_f32()).min(1.0);

            // Simplified easing functions
            let ease_out_cubic = |t: f32| -> f32 {
                let t = t - 1.0;
                t * t * t + 1.0
            };
            let ease_in_out_sine = |t: f32| -> f32 {
                let t = t.clamp(0.0, 1.0);
                -(t * std::f32::consts::PI).cos() / 2.0 + 0.5
            };

            let (z_depth, scale, shadow_intensity, border_glow, rotation_y, final_opacity) =
                if overall_progress < rise_end {
                    let phase_progress = overall_progress / rise_end;
                    let eased = ease_out_cubic(phase_progress);
                    let z = -10.0 * (1.0 - eased);
                    let shadow = 0.5 * eased;
                    let opacity = opacity * (0.7 + 0.2 * eased);
                    (z, 1.0, shadow, 0.0, std::f32::consts::PI, opacity)
                } else if overall_progress < emerge_end {
                    let phase_progress = (overall_progress - rise_end) / (emerge_end - rise_end);
                    let eased = ease_out_cubic(phase_progress);
                    let z = 10.0 * eased;
                    let scale = 1.0 + 0.05 * eased;
                    let shadow = 0.5 + 0.5 * eased;
                    let glow = 0.5 * eased;
                    (z, scale, shadow, glow, std::f32::consts::PI, opacity * 0.9)
                } else if overall_progress < flip_end {
                    let phase_progress = (overall_progress - emerge_end) / (flip_end - emerge_end);
                    let rotation_eased = ease_in_out_sine(phase_progress);
                    let rotation = std::f32::consts::PI * (1.0 - rotation_eased);
                    let glow = 0.5 * (1.0 - phase_progress);
                    (10.0, 1.05, 1.0, glow, rotation, opacity)
                } else {
                    let phase_progress = (overall_progress - flip_end) / (1.0 - flip_end);
                    let eased = ease_out_cubic(phase_progress);
                    let z = 10.0 * (1.0 - eased);
                    let scale = 1.0 + 0.05 * (1.0 - eased);
                    let shadow = 1.0 * (1.0 - eased) + 0.3;
                    (z, scale, shadow, 0.0, 0.0, opacity)
                };

            (
                final_opacity,
                rotation_y,
                overall_progress,
                z_depth,
                scale,
                shadow_intensity,
                border_glow,
            )
        }
    }
}

impl Primitive for RoundedImagePrimitive {
    type Renderer = ();

    fn initialize(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _format: wgpu::TextureFormat,
    ) -> Self::Renderer {
        ()
    }

    fn prepare(
        &self,
        _renderer: &mut Self::Renderer,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // Batched pipeline performs all rendering work.
    }
}

impl BatchPrimitive for RoundedImagePrimitive {
    type BatchState = RoundedImageBatchState;

    fn create_batch_state(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self::BatchState {
        RoundedImageBatchState::new(device, format)
    }

    fn encode_batch(&self, state: &mut Self::BatchState, context: &BatchEncodeContext<'_>) -> bool {
        let transformed_bounds = Rectangle::new(
            Point::new(context.bounds.x, context.bounds.y),
            Size::new(context.bounds.width, context.bounds.height),
        );

        state.enqueue(PendingPrimitive {
            id: self.id,
            handle: self.handle.clone(),
            // Use renderer-provided bounds so batched instances inherit scroll/scale transforms.
            bounds: transformed_bounds,
            radius: self.radius,
            animation: self.animation,
            load_time: self.load_time,
            opacity: self.opacity,
            theme_color: self.theme_color,
            animated_bounds: self.animated_bounds,
            is_hovered: self.is_hovered,
            mouse_position: self.mouse_position,
            progress: self.progress,
            progress_color: self.progress_color,
        });

        true
    }
}

/// A widget that displays an image with rounded corners using GPU shaders
pub struct RoundedImage {
    id: u64,
    handle: Handle,
    radius: f32,
    width: Length,
    height: Length,
    animation: AnimationType,
    load_time: Option<Instant>,
    opacity: f32,
    theme_color: Color,
    bounds: Option<AnimatedPosterBounds>,
    is_hovered: bool,
    on_play: Option<Message>,
    on_edit: Option<Message>,
    on_options: Option<Message>,
    on_click: Option<Message>, // For clicking empty space (details page)
    progress: Option<f32>,     // Progress percentage (0.0 to 1.0)
    progress_color: Color,     // Color for the progress bar
}

impl RoundedImage {
    /// Creates a new rounded image with a single handle
    pub fn new(handle: Handle, id: Option<u64>) -> Self {
        use crate::domains::ui::theme::MediaServerTheme;

        Self {
            id: id.unwrap_or(0),
            handle,
            radius: crate::infrastructure::constants::layout::poster::CORNER_RADIUS,
            width: Length::Fixed(200.0),
            height: Length::Fixed(300.0),
            animation: AnimationType::None,
            load_time: None,
            opacity: 1.0,
            theme_color: Color::from_rgb(0.1, 0.1, 0.1), // Default dark gray
            bounds: None,
            is_hovered: false,
            on_play: None,
            on_edit: None,
            on_options: None,
            on_click: None,
            progress: None,
            progress_color: MediaServerTheme::ACCENT_BLUE, // Default to theme blue
        }
    }

    /// Sets the corner radius
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Sets the width
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the animation type
    pub fn with_animation(mut self, animation: AnimationType) -> Self {
        self.animation = animation;
        /*
        if self.load_time.is_none() {
            self.load_time = Some(Instant::now());
        } */
        self
    }

    /// Sets the load time for animation
    pub fn with_load_time(mut self, load_time: Instant) -> Self {
        self.load_time = Some(load_time);
        self
    }

    /// Sets the opacity
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// Sets the theme color for backface
    pub fn theme_color(mut self, color: Color) -> Self {
        self.theme_color = color;
        self
    }

    /// Sets animated bounds with padding
    pub fn with_animated_bounds(mut self, bounds: AnimatedPosterBounds) -> Self {
        self.bounds = Some(bounds);
        // Use layout bounds for stable grid positioning
        let (width, height) = bounds.layout_bounds();
        self.width = Length::Fixed(width);
        self.height = Length::Fixed(height);
        self
    }

    /// Sets the hover state
    pub fn is_hovered(mut self, hovered: bool) -> Self {
        self.is_hovered = hovered;
        self
    }

    /// Sets the play button callback
    pub fn on_play(mut self, message: Message) -> Self {
        self.on_play = Some(message);
        self
    }

    /// Sets the edit button callback
    pub fn on_edit(mut self, message: Message) -> Self {
        self.on_edit = Some(message);
        self
    }

    /// Sets the options button callback
    pub fn on_options(mut self, message: Message) -> Self {
        self.on_options = Some(message);
        self
    }

    /// Sets the click callback (for clicking empty space)
    pub fn on_click(mut self, message: Message) -> Self {
        self.on_click = Some(message);
        self
    }

    /// Sets the progress percentage (0.0 to 1.0)
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    /// Sets the progress bar color
    pub fn progress_color(mut self, color: Color) -> Self {
        self.progress_color = color;
        self
    }
}

/// Helper function to create a rounded image widget
pub fn rounded_image_shader(handle: Handle, id: Option<u64>) -> RoundedImage {
    RoundedImage::new(handle, id)
}

impl<'a> From<RoundedImage> for Element<'a, Message> {
    fn from(image: RoundedImage) -> Self {
        let shader = iced::widget::shader(RoundedImageProgram {
            id: image.id,
            handle: image.handle,
            radius: image.radius,
            animation: image.animation,
            load_time: image.load_time,
            opacity: image.opacity,
            theme_color: image.theme_color,
            bounds: image.bounds,
            is_hovered: image.is_hovered,
            progress: image.progress,
            progress_color: image.progress_color,
            on_play: image.on_play,
            on_edit: image.on_edit,
            on_options: image.on_options,
            on_click: image.on_click,
        })
        .width(image.width)
        .height(image.height);

        shader.into()
    }
}
