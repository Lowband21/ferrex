//! Shader-based rounded image widget for Iced
//!
//! This implementation uses GPU shaders for true rounded rectangle clipping
//! with anti-aliasing, providing better performance than Canvas-based approaches.

pub mod rounded_image_batch_state;

use crate::domains::ui::messages::Message;

use crate::infrastructure::constants::animation;
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::Viewport;
use iced::wgpu;
use iced::widget::image::Handle;
use iced::widget::shader::Program;
use iced::widget::shader::{Primitive, Storage};
use iced::{mouse, Color, Element, Event, Length, Point, Rectangle};
use iced_wgpu::image as wgpu_image;
use iced_wgpu::primitive::PrimitiveBatchState;
use rounded_image_batch_state::RoundedImageInstance;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
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
            animated_bounds: self.bounds.clone(),
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
        match event {
            Event::Mouse(mouse_event) => {
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
                            else if norm_x >= 0.79
                                && norm_x <= 0.91
                                && norm_y >= 0.09
                                && norm_y <= 0.21
                            {
                                if let Some(on_edit) = &self.on_edit {
                                    log::debug!("Edit button clicked!");
                                    return Some(iced::widget::Action::publish(on_edit.clone()));
                                }
                            }
                            // Bottom-right options button (radius 0.06 at 0.85, 0.85)
                            else if norm_x >= 0.79
                                && norm_x <= 0.91
                                && norm_y >= 0.79
                                && norm_y <= 0.91
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
                        if let Some(position) = cursor.position() {
                            if bounds.contains(position) {
                                let relative_pos =
                                    Point::new(position.x - bounds.x, position.y - bounds.y);
                                state.mouse_position = Some(relative_pos);
                                state.is_hovered = true;
                                //log::debug!("Cursor entered widget at: {:?}", relative_pos);
                            }
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
            _ => {}
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
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    atlas_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
}

/// Per-primitive render data
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PrimitiveData {
    instance_buffer: wgpu::Buffer,
}

/// Batched render data for all primitives in a frame
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

impl Default for State {
    fn default() -> Self {
        Self {
            globals_buffer: None,
            globals_bind_group: None,
            primitive_data: HashMap::new(),
        }
    }
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
fn create_instance(
    atlas_entry: Option<&wgpu_image::atlas::Entry>,
    bounds: &Rectangle,
    radius: f32,
    animation: AnimationType,
    load_time: Option<Instant>,
    opacity: f32,
    theme_color: Color,
    animated_bounds: Option<&AnimatedPosterBounds>,
    is_hovered: bool,
    mouse_position: Option<Point>,
    progress: Option<f32>,
    progress_color: Color,
) -> Instance {
    // Extract UV coordinates and layer from the atlas entry
    let (uv_min, uv_max, layer) = if let Some(entry) = atlas_entry {
        match entry {
            wgpu_image::atlas::Entry::Contiguous(allocation) => {
                let (x, y) = allocation.position();
                let size = allocation.size();
                let layer = allocation.layer() as u32;
                const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                let uv_min = [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE];
                let uv_max = [
                    (x + size.width) as f32 / ATLAS_SIZE,
                    (y + size.height) as f32 / ATLAS_SIZE,
                ];
                (uv_min, uv_max, layer)
            }
            wgpu_image::atlas::Entry::Fragmented { size, fragments } => {
                if let Some(first) = fragments.first() {
                    let (x, y) = first.position;
                    let layer = first.allocation.layer() as u32;
                    const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                    let uv_min = [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE];
                    let uv_max = [
                        (x + size.width) as f32 / ATLAS_SIZE,
                        (y + size.height) as f32 / ATLAS_SIZE,
                    ];
                    (uv_min, uv_max, layer)
                } else {
                    ([0.0, 0.0], [0.001, 0.001], 0)
                }
            }
        }
    } else {
        ([0.0, 0.0], [0.001, 0.001], 0)
    };

    let (
        actual_opacity,
        rotation_y,
        animation_progress,
        z_depth,
        scale,
        shadow_intensity,
        border_glow,
    ) = if let Some(load_time) = load_time {
        let elapsed = std::time::Instant::now().duration_since(load_time);
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
            0.7 as f32,
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

        if norm_x >= -0.01 && norm_x <= 1.01 && norm_y >= -0.01 && norm_y <= 1.01 {
            [norm_x.clamp(0.0, 1.0), norm_y.clamp(0.0, 1.0)]
        } else {
            [-1.0, -1.0]
        }
    } else {
        [-1.0, -1.0]
    };

    // Create instance data
    Instance {
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
fn create_batch_instance(
    atlas_entry: Option<&wgpu_image::atlas::Entry>,
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
    calc_anim: bool,
) -> rounded_image_batch_state::RoundedImageInstance {
    // Extract UV coordinates and layer from the atlas entry
    let (uv_min, uv_max, layer) = if let Some(entry) = atlas_entry {
        match entry {
            wgpu_image::atlas::Entry::Contiguous(allocation) => {
                let (x, y) = allocation.position();
                let size = allocation.size();
                let layer = allocation.layer() as u32;
                const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                let uv_min = [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE];
                let uv_max = [
                    (x + size.width) as f32 / ATLAS_SIZE,
                    (y + size.height) as f32 / ATLAS_SIZE,
                ];
                (uv_min, uv_max, layer)
            }
            wgpu_image::atlas::Entry::Fragmented { size, fragments } => {
                if let Some(first) = fragments.first() {
                    let (x, y) = first.position;
                    let layer = first.allocation.layer() as u32;
                    const ATLAS_SIZE: f32 = wgpu_image::atlas::SIZE as f32;
                    let uv_min = [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE];
                    let uv_max = [
                        (x + size.width) as f32 / ATLAS_SIZE,
                        (y + size.height) as f32 / ATLAS_SIZE,
                    ];
                    (uv_min, uv_max, layer)
                } else {
                    ([0.0, 0.0], [0.001, 0.001], 0)
                }
            }
        }
    } else {
        ([0.0, 0.0], [0.001, 0.001], 0)
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
            0.7 as f32,
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

        if norm_x >= -0.01 && norm_x <= 1.01 && norm_y >= -0.01 && norm_y <= 1.01 {
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
fn create_placeholder_instance(
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
        0.7 as f32,
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
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn prepare_batched(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        format: wgpu::TextureFormat,
        storage: &mut Storage,
        bounds: &Rectangle,
        viewport: &Viewport,
        image_cache: &mut wgpu_image::Cache,
    ) {
        // Register batch state on first use if batching is enabled
        let type_id = TypeId::of::<RoundedImagePrimitive>();

        let has_batch = storage.has_batch_state(&type_id);

        if !has_batch {
            // Register the batch state for this primitive type
            let batch_state =
                rounded_image_batch_state::RoundedImageBatchState::new(device, format);
            storage.store_batch_state(type_id, Box::new(batch_state));
            log::debug!("Registered RoundedImagePrimitive for batched rendering");
        }

        if has_batch {
            let cached = image_cache.contains(&self.handle);

            // Normal path with atlas upload
            if let Some(batch_state) = storage.get_batch_state_mut(&type_id) {
                // Downcast to our specific batch state type
                if let Some(rounded_batch) = batch_state
                    .as_any_mut()
                    .downcast_mut::<rounded_image_batch_state::RoundedImageBatchState>(
                ) {
                    let load_time = if self.animation != AnimationType::None {
                        rounded_batch.loaded_times.get(&self.id)
                    } else {
                        None
                    };

                    let instance = if cached {
                        create_batch_instance(
                            None, // No atlas entry yet
                            bounds,
                            self.radius,
                            self.animation,
                            load_time,
                            self.opacity,
                            self.theme_color,
                            self.animated_bounds.as_ref(),
                            self.is_hovered,
                            self.mouse_position,
                            self.progress,
                            self.progress_color,
                            cached,
                        )
                    } else {
                        create_placeholder_instance(
                            bounds,
                            self.radius,
                            self.theme_color,
                            self.animated_bounds.as_ref(),
                            self.progress,
                            self.progress_color,
                        )
                    };

                    rounded_batch.add_instance(
                        self.id,
                        instance,
                        &self.handle,
                        image_cache,
                        device,
                        encoder,
                        cached,
                    );
                }
            }

            return; // Don't create individual buffers when batching
        }

        // Fallback to individual rendering
        // Initialize pipeline if needed
        if !storage.has::<Pipeline>() {
            storage.store(Pipeline::new(device, format));
        }

        // Initialize state if needed
        if !storage.has::<State>() {
            storage.store(State::default());
        }

        // Setup globals if needed - extract what we need from pipeline first
        let (globals_bind_group_layout, sampler) = {
            let pipeline = storage.get::<Pipeline>().unwrap();
            (
                pipeline.globals_bind_group_layout.clone(),
                pipeline.sampler.clone(),
            )
        };

        let state = storage.get_mut::<State>().unwrap();

        if state.globals_buffer.is_none() {
            let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rounded_image_globals"),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rounded_image_globals"),
                layout: &globals_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            state.globals_buffer = Some(globals_buffer);
            state.globals_bind_group = Some(globals_bind_group);
        }

        // Update globals with current viewport
        let transform: [f32; 16] = viewport.projection().into();
        let globals = Globals {
            transform,
            scale_factor: viewport.scale_factor() as f32,
            _padding: [0.0; 7], // 7 floats = 28 bytes padding to reach 96 bytes total
        };
        // Use write_buffer_with to avoid intermediate copy
        match queue.write_buffer_with(
            state.globals_buffer.as_ref().unwrap(),
            0,
            wgpu::BufferSize::new(std::mem::size_of::<Globals>() as u64).unwrap(),
        ) { Some(mut view) => {
            view.copy_from_slice(bytemuck::cast_slice(&[globals]));
        } _ => {
            log::error!("Failed to map globals buffer for writing");
        }}

        // Profile texture upload to atlas
        #[cfg(feature = "profile-with-tracy")]
        let upload_span = crate::infrastructure::gpu_profiling::gpu_span("TextureUpload", encoder);

        let atlas_entry = image_cache.upload_raster(device, encoder, &self.handle);

        #[cfg(feature = "profile-with-tracy")]
        if let Some(span) = upload_span {
            crate::infrastructure::gpu_profiling::end_gpu_span(span, encoder);
        }

        // Create instance for this primitive
        let instance = create_instance(
            atlas_entry,
            bounds,
            self.radius,
            self.animation,
            self.load_time,
            self.opacity,
            self.theme_color,
            self.animated_bounds.as_ref(),
            self.is_hovered,
            self.mouse_position,
            self.progress,
            self.progress_color,
        );

        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("UI::RoundedImageShader::BufferAlloc");
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rounded Image Instance Buffer"),
            size: std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("UI::RoundedImageShader::BufferWrite");
        // Write instance data to the buffer using write_buffer_with to avoid intermediate copy
        match queue.write_buffer_with(
            &instance_buffer,
            0,
            wgpu::BufferSize::new(std::mem::size_of::<Instance>() as u64).unwrap(),
        ) { Some(mut view) => {
            view.copy_from_slice(bytemuck::cast_slice(&[instance]));
        } _ => {
            log::error!("Failed to map instance buffer for writing");
        }}

        let key = self as *const _ as usize;
        state
            .primitive_data
            .insert(key, PrimitiveData { instance_buffer });
    }

    fn render_with_cache(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        storage: &Storage,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
        image_cache: &wgpu_image::Cache,
    ) {
        // Skip individual rendering if batching is enabled
        let type_id = TypeId::of::<RoundedImagePrimitive>();
        if storage.has_batch_state(&type_id) {
            return; // Batch state will handle rendering
        }

        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!(crate::infrastructure::profiling_scopes::scopes::VIEW_DRAW);

        // Start GPU profiling span
        #[cfg(feature = "profile-with-tracy")]
        let gpu_span =
            crate::infrastructure::gpu_profiling::gpu_span("RoundedImageRender", encoder);

        let pipeline = storage.get::<Pipeline>().unwrap();
        let state = storage.get::<State>().unwrap();

        // Get globals bind group
        let Some(globals_bind_group) = &state.globals_bind_group else {
            log::warn!("Globals bind group not initialized");
            #[cfg(feature = "profile-with-tracy")]
            if let Some(span) = gpu_span {
                crate::infrastructure::gpu_profiling::end_gpu_span(span, encoder);
            }
            return;
        };

        // Skip if no instances to render
        if state.primitive_data.is_empty() {
            #[cfg(feature = "profile-with-tracy")]
            if let Some(span) = gpu_span {
                crate::infrastructure::gpu_profiling::end_gpu_span(span, encoder);
            }
            return;
        }

        // Get per-primitive data using primitive address as key
        let key = self as *const _ as usize;
        let Some(primitive_data) = state.primitive_data.get(&key) else {
            log::warn!("No data for primitive {:p}", self);
            #[cfg(feature = "profile-with-tracy")]
            if let Some(span) = gpu_span {
                crate::infrastructure::gpu_profiling::end_gpu_span(span, encoder);
            }
            return;
        };

        // Get the atlas bind group from the image cache
        let atlas_bind_group = image_cache.bind_group();

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rounded Image Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        // Set the render pipeline
        render_pass.set_pipeline(&pipeline.render_pipeline);

        // Bind the globals uniform buffer (group 0)
        render_pass.set_bind_group(0, globals_bind_group, &[]);

        // Bind the atlas texture array (group 1)
        render_pass.set_bind_group(1, atlas_bind_group, &[]);

        // Set the vertex buffer (instance data)
        render_pass.set_vertex_buffer(0, primitive_data.instance_buffer.slice(..));

        // Calculate proper scissor rect based on animated bounds
        let (scissor_x, scissor_y, scissor_width, scissor_height) = {
            (
                clip_bounds.x,
                clip_bounds.y,
                clip_bounds.width.max(1),
                clip_bounds.height.max(1),
            )
        };

        render_pass.set_scissor_rect(scissor_x, scissor_y, scissor_width, scissor_height);

        // Draw quad (4 vertices) with 1 instance
        render_pass.draw(0..4, 0..1);

        // Explicitly drop render pass before ending GPU span
        drop(render_pass);

        // End GPU profiling span
        #[cfg(feature = "profile-with-tracy")]
        if let Some(span) = gpu_span {
            crate::infrastructure::gpu_profiling::end_gpu_span(span, encoder);
        }
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
