//! Shader-based rounded image widget for Iced
//!
//! This implementation uses GPU shaders for true rounded rectangle clipping
//! with anti-aliasing, providing better performance than Canvas-based approaches.

use crate::domains::ui::messages::Message;
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::core::image;
use iced::advanced::graphics::Viewport;
use iced::wgpu;
use iced::widget::image::Handle;
use iced::widget::shader::Program;
use iced::widget::shader::{Primitive, Storage};
use iced::{event, mouse, Color, Element, Event, Length, Point, Rectangle, Size};
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
#[derive(Debug, Clone, Copy)]
pub enum AnimationType {
    None,
    Fade {
        duration: Duration,
    },
    Flip {
        duration: Duration,
    },
    EnhancedFlip {
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
            AnimationType::EnhancedFlip { .. } => 3,
            AnimationType::PlaceholderSunken => 4,
        }
    }

    /// Create default enhanced flip animation with standard timings
    pub fn enhanced_flip() -> Self {
        use crate::infrastructure::constants::animation;

        AnimationType::EnhancedFlip {
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
            image_handle: self.handle.clone(),
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
                                log::debug!("Cursor entered widget at: {:?}", relative_pos);
                                return Some(iced::widget::Action::request_redraw());
                            }
                        }
                    }
                    mouse::Event::CursorLeft => {
                        // Clear mouse position when cursor leaves
                        state.mouse_position = None;
                        state.is_hovered = false;
                        log::debug!("Cursor left widget");
                        return Some(iced::widget::Action::request_redraw());
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
    pub image_handle: Handle,
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

/// Cached state for dirty checking
#[derive(Debug, Clone)]
struct CachedPrimitiveState {
    bounds: Rectangle,
    animation_progress: f32,
    is_hovered: bool,
    mouse_position: Option<Point>,
    opacity: f32,
    scale: f32,
    z_depth: f32,
}

/// Global uniform data (viewport transform)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16], // 4x4 matrix
    scale_factor: f32,
    _padding: [f32; 3], // Padding to align to 16 bytes
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
}

/// Pipeline state (immutable after creation)
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    texture_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
}

/// Per-primitive render data
struct PrimitiveData {
    instance_buffer: wgpu::Buffer,
    texture_bind_group: wgpu::BindGroup,
}

/// Texture upload tracking for frame budgeting
struct TextureUploadTracker {
    /// Bytes uploaded in current frame
    bytes_uploaded_this_frame: usize,
    /// Maximum bytes to upload per frame (2MB default)
    max_bytes_per_frame: usize,
    /// Queue of pending texture uploads
    pending_uploads: Vec<PendingTextureUpload>,
}

/// A texture upload that's been deferred
struct PendingTextureUpload {
    image_id: image::Id,
    texture: Arc<wgpu::Texture>,
    data: Vec<u8>,
    width: u32,
    height: u32,
}

impl Default for TextureUploadTracker {
    fn default() -> Self {
        Self {
            bytes_uploaded_this_frame: 0,
            max_bytes_per_frame: 10 * 1024 * 1024, // 10MB - enough for high-res detail view posters
            pending_uploads: Vec::new(),
        }
    }
}

/// Shared state for all rounded images
///
/// IMPORTANT: Frame Boundary Detection Issue & Solution
///
/// The original code tried to detect frame boundaries to clean up primitive data.
/// This failed because Iced's render pipeline interleaves prepare() and render() calls:
/// - Frame 1: prepare1, prepare2, render1, render2
/// - Frame 2: prepare3 (incorrectly detected as "new frame"), render3 FAILS!
///
/// Our solution: Don't try to detect frames at all. Instead:
/// 1. Keep all primitive data indefinitely (HashMap with primitive pointer as key)
/// 2. Let Iced's Storage lifecycle handle major cleanups (Storage may be recreated)
/// 3. Use dirty checking to skip redundant prepare() work for unchanged primitives
/// 4. Create buffers on-demand without pooling (GPU driver handles memory efficiently)
///
/// This approach is robust because:
/// - No assumptions about Iced's internal pipeline ordering
/// - Handles any interleaving of prepare/render calls
/// - Simple and predictable behavior
/// - Memory usage is bounded by number of active primitives
struct State {
    // Globals buffer and bind group (shared by all)
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    // Texture cache (shared)
    texture_cache: HashMap<image::Id, Arc<wgpu::Texture>>,
    // Texture bind groups (shared)
    texture_bind_groups: HashMap<image::Id, wgpu::BindGroup>,
    // Per-primitive data for current frame
    primitive_data: HashMap<usize, PrimitiveData>,

    // Cached state for dirty checking - skip updates for unchanged posters
    cached_states: HashMap<usize, CachedPrimitiveState>,

    // Texture upload tracking to prevent frame drops
    upload_tracker: TextureUploadTracker,

    // Default texture for binding when real texture is loading
    default_texture: Option<Arc<wgpu::Texture>>,
    default_bind_group: Option<wgpu::BindGroup>,

    // Counter for tracking prepare calls to manage budget resets
    prepare_call_count: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            globals_buffer: None,
            globals_bind_group: None,
            texture_cache: HashMap::new(),
            texture_bind_groups: HashMap::new(),
            primitive_data: HashMap::new(),
            cached_states: HashMap::new(),
            upload_tracker: TextureUploadTracker::default(),
            default_texture: None,
            default_bind_group: None,
            prepare_call_count: 0,
        }
    }
}

impl State {
    /// Get or create the default texture for binding when real textures are loading
    fn get_or_create_default_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> &wgpu::BindGroup {
        if self.default_texture.is_none() {
            // Create a simple 1x1 grey texture
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Default Texture"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,  // GPU auto-converts sRGB to linear
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            // Write a grey pixel
            let pixel_data = [128u8, 128u8, 128u8, 255u8]; // Grey with full alpha
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixel_data,
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

            let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Default Texture Bind Group"),
                layout: texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                ],
            });

            self.default_texture = Some(Arc::new(texture));
            self.default_bind_group = Some(bind_group);
        }

        self.default_bind_group.as_ref().unwrap()
    }

    /// Process pending texture uploads within budget
    fn process_pending_uploads(&mut self, queue: &wgpu::Queue) {
        let mut processed = Vec::new();

        for (i, upload) in self.upload_tracker.pending_uploads.iter().enumerate() {
            let upload_size = upload.data.len();

            // Check if we have budget for this upload
            if self.upload_tracker.bytes_uploaded_this_frame + upload_size
                > self.upload_tracker.max_bytes_per_frame
            {
                // No more budget this frame
                break;
            }

            // Upload the texture data
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &upload.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * upload.width),
                    rows_per_image: Some(upload.height),
                },
                wgpu::Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );

            self.upload_tracker.bytes_uploaded_this_frame += upload_size;
            processed.push(i);

            // Mark texture as ready in cache
            self.texture_cache
                .insert(upload.image_id, upload.texture.clone());
        }

        // Remove processed uploads (must sort then reverse to maintain correct indices)
        processed.sort_unstable();
        for i in processed.into_iter().rev() {
            self.upload_tracker.pending_uploads.remove(i);
        }

        // Log if we have a backlog
        if !self.upload_tracker.pending_uploads.is_empty() {
            log::debug!(
                "Texture upload backlog: {} pending uploads",
                self.upload_tracker.pending_uploads.len()
            );
        }
    }
}

impl Pipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        log::info!("Creating rounded image shader pipeline");

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rounded Image Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/rounded_image.wgsl").into()),
        });

        // Create globals bind group layout
        log::info!("Creating globals bind group layout");
        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Globals"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(std::num::NonZeroU64::new(96).unwrap()),
                    },
                    count: None,
                }],
            });

        // Create texture bind group layout
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Texture"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rounded Image Pipeline Layout"),
            bind_group_layouts: &[&globals_bind_group_layout, &texture_bind_group_layout],
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
            texture_bind_group_layout: Arc::new(texture_bind_group_layout),
            globals_bind_group_layout: Arc::new(globals_bind_group_layout),
            sampler: Arc::new(sampler),
        }
    }
}

/// Result of texture loading with budget
enum TextureLoadResult {
    Loaded(Arc<wgpu::Texture>),
    Deferred(PendingTextureUpload),
    Failed,
}

/// Load texture with upload budget checking
fn load_texture_budgeted(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    handle: &Handle,
    tracker: &mut TextureUploadTracker,
) -> TextureLoadResult {
    // First decode the image (this is still synchronous - could be improved)
    let (image_data, width, height) = match decode_image(handle) {
        Some(data) => data,
        None => return TextureLoadResult::Failed,
    };

    let upload_size = image_data.len();
    let image_id = handle.id();

    // Create the texture (lightweight operation)
    let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Rounded Image Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }));

    // Check if we have budget for immediate upload
    if tracker.bytes_uploaded_this_frame + upload_size <= tracker.max_bytes_per_frame {
        // Upload immediately
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
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        tracker.bytes_uploaded_this_frame += upload_size;
        TextureLoadResult::Loaded(texture)
    } else {
        // Defer upload to next frame
        TextureLoadResult::Deferred(PendingTextureUpload {
            image_id,
            texture,
            data: image_data,
            width,
            height,
        })
    }
}

/// Decode image data from handle
fn decode_image(handle: &Handle) -> Option<(Vec<u8>, u32, u32)> {
    match handle {
        Handle::Path(_, path) => {
            log::info!("Loading image from path: {:?}", path);
            match ::image::open(path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    Some((rgba.into_raw(), w, h))
                }
                Err(e) => {
                    log::error!("Failed to load image from path {:?}: {}", path, e);
                    None
                }
            }
        }
        Handle::Bytes(_, bytes) => match ::image::load_from_memory(bytes) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                Some((rgba.into_raw(), w, h))
            }
            Err(e) => {
                log::error!("Failed to load image from bytes: {}", e);
                None
            }
        },
        Handle::Rgba {
            pixels,
            width,
            height,
            ..
        } => Some((pixels.to_vec(), *width, *height)),
    }
}

impl Primitive for RoundedImagePrimitive {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        storage: &mut Storage,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
        profiling::scope!(crate::infrastructure::profiling_scopes::scopes::POSTER_GPU_UPLOAD);
        
        // Initialize pipeline if needed
        if !storage.has::<Pipeline>() {
            //log::info!("Creating rounded image shader pipeline");
            storage.store(Pipeline::new(device, format));
        }

        // Initialize state if needed
        if !storage.has::<State>() {
            storage.store(State::default());
        }

        // Extract pipeline resources before mutable borrow
        let (globals_layout, texture_layout, sampler) = {
            let pipeline = storage.get::<Pipeline>().unwrap();
            (
                pipeline.globals_bind_group_layout.clone(),
                pipeline.texture_bind_group_layout.clone(),
                pipeline.sampler.clone(),
            )
        };

        let state = storage.get_mut::<State>().unwrap();

        // Reset upload budget periodically to allow batched uploads
        // Every 5 primitives gets a fresh budget
        state.prepare_call_count += 1;
        if state.prepare_call_count % 5 == 0 {
            state.upload_tracker.bytes_uploaded_this_frame = 0;
        }

        // Don't clear primitive_data - it causes flickering!
        // The data will naturally be bounded by the number of unique poster positions on screen
        // Old entries become stale but harmless (just wasted memory, not visible flickering)

        // Process pending texture uploads from previous calls
        state.process_pending_uploads(queue);

        // Update globals if needed
        if state.globals_buffer.is_none() {
            let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Rounded Image Globals"),
                size: 96, // mat4x4 requires 96 bytes due to alignment
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Rounded Image Globals Bind Group"),
                layout: &globals_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buffer.as_entire_binding(),
                }],
            });

            state.globals_buffer = Some(globals_buffer);
            state.globals_bind_group = Some(globals_bind_group);
        }

        // Update globals with current viewport
        let transform: [f32; 16] = viewport.projection().into();
        let globals = Globals {
            transform,
            scale_factor: viewport.scale_factor() as f32,
            _padding: [0.0; 3],
        };
        queue.write_buffer(
            state.globals_buffer.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&[globals]),
        );

        // Process any pending texture uploads from previous frames
        state.process_pending_uploads(queue);

        // Load texture if needed (with budget checking)
        let image_id = self.image_handle.id();

        if !state.texture_cache.contains_key(&image_id) {
            // Check if this texture is already pending
            let is_pending = state
                .upload_tracker
                .pending_uploads
                .iter()
                .any(|u| u.image_id == image_id);

            if !is_pending {
                // Try to load with budget
                match load_texture_budgeted(
                    device,
                    queue,
                    &self.image_handle,
                    &mut state.upload_tracker,
                ) {
                    TextureLoadResult::Loaded(texture) => {
                        state.texture_cache.insert(image_id, texture);
                    }
                    TextureLoadResult::Deferred(pending) => {
                        // Add to pending queue for next frame
                        state.upload_tracker.pending_uploads.push(pending);
                        // Don't return - continue with default texture
                    }
                    TextureLoadResult::Failed => {
                        log::error!("Failed to load texture for image {:?}", image_id);
                        // Don't return - continue with default texture
                    }
                }
            } else {
                // Texture is pending, use default texture for now
                // Don't return - continue with default texture
            }
        }

        // Get or create the texture bind group
        let texture_bind_group = if let Some(texture) = state.texture_cache.get(&image_id) {
            // We have the real texture, create or get its bind group
            if !state.texture_bind_groups.contains_key(&image_id) {
                let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Rounded Image Texture Bind Group"),
                    layout: &texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&texture_view),
                        },
                    ],
                });

                state.texture_bind_groups.insert(image_id, bind_group);
            }
            state.texture_bind_groups.get(&image_id).unwrap().clone()
        } else {
            // No texture yet, use default
            state
                .get_or_create_default_texture(device, queue, &texture_layout, &sampler)
                .clone()
        };

        // Create instance data for this primitive
        // IMPORTANT: The bounds parameter might already be in screen space
        // Check if we need to adjust for viewport offset

        // Debug log the incoming state
        //log::debug!("RoundedImagePrimitive prepare:");
        //log::debug!("  - animation type: {:?}", self.animation);
        //log::debug!("  - load_time: {:?}", self.load_time);
        //log::debug!("  - opacity: {}", self.opacity);

        // Calculate animation values based on animation type
        #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
        profiling::scope!(crate::infrastructure::profiling_scopes::scopes::ANIM_TRANSITION);
        
        let (
            actual_opacity,
            rotation_y,
            animation_progress,
            z_depth,
            scale,
            shadow_intensity,
            border_glow,
        ) = match self.animation {
            AnimationType::None => (self.opacity, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0),
            AnimationType::PlaceholderSunken => {
                // Placeholder state: backface showing, sunken
                //log::debug!("PlaceholderSunken state - showing backface with theme color");
                // Dimmed opacity, rotation PI (backface), no progress, sunken z-depth
                (0.7, std::f32::consts::PI, 0.0, -10.0, 1.0, 0.0, 0.0)
            }
            AnimationType::Fade { duration } => {
                if let Some(load_time) = self.load_time {
                    let elapsed = load_time.elapsed().as_secs_f32();
                    let progress = (elapsed / duration.as_secs_f32()).min(1.0);
                    log::debug!(
                        "Fade animation - elapsed: {:.3}s, progress: {:.3}",
                        elapsed,
                        progress
                    );
                    (self.opacity * progress, 0.0, progress, 0.0, 1.0, 0.0, 0.0)
                } else {
                    (self.opacity, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0)
                }
            }
            AnimationType::Flip { duration } => {
                if let Some(load_time) = self.load_time {
                    let elapsed = load_time.elapsed().as_secs_f32();
                    let progress = (elapsed / duration.as_secs_f32()).min(1.0);

                    // Flip animation: Start at PI (backface) and rotate to 0 (front face)
                    let rotation = std::f32::consts::PI * (1.0 - progress); // PI to 0

                    log::debug!("Flip animation - load_time: {:?}, elapsed: {:.3}s, progress: {:.3}, rotation: {:.3} rad",
                        load_time, elapsed, progress, rotation);

                    (self.opacity, rotation, progress, 0.0, 1.0, 0.0, 0.0)
                } else {
                    log::debug!("Flip animation - no load_time, animation completed");
                    // Return completed state (visible, no rotation, full progress)
                    (self.opacity, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0)
                }
            }
            AnimationType::EnhancedFlip {
                total_duration,
                rise_end,
                emerge_end,
                flip_end,
            } => {
                if let Some(load_time) = self.load_time {
                    let elapsed = load_time.elapsed().as_secs_f32();
                    let overall_progress = (elapsed / total_duration.as_secs_f32()).min(1.0);

                    // Smooth easing function
                    let ease_out_cubic = |t: f32| -> f32 {
                        let t = t - 1.0;
                        t * t * t + 1.0
                    };

                    let ease_in_out_cubic = |t: f32| -> f32 {
                        if t < 0.5 {
                            4.0 * t * t * t
                        } else {
                            let t = 2.0 * t - 2.0;
                            1.0 + t * t * t / 2.0
                        }
                    };

                    // Calculate phase-specific values
                    let (z_depth, scale, shadow_intensity, border_glow, rotation_y, final_opacity) =
                        if overall_progress < rise_end {
                            // Phase 1: Rising (0.0 - 0.25)
                            let phase_progress = overall_progress / rise_end;
                            let eased = ease_out_cubic(phase_progress);

                            // Z-depth from -10 to 0
                            let z = -10.0 * (1.0 - eased);
                            // Shadow fades in more dramatically
                            let shadow = 0.5 * eased;
                            // Brightness increases (through opacity)
                            let opacity = self.opacity * (0.7 + 0.2 * eased);

                            (z, 1.0, shadow, 0.0, std::f32::consts::PI, opacity)
                        } else if overall_progress < emerge_end {
                            // Phase 2: Emerging (0.25 - 0.5)
                            let phase_progress =
                                (overall_progress - rise_end) / (emerge_end - rise_end);
                            let eased = ease_in_out_cubic(phase_progress);

                            // Z-depth from 0 to +10 (reduced for less dramatic effect)
                            let z = 10.0 * eased;
                            // Scale from 1.0 to 1.05 (reduced to stay within bounds)
                            let scale = 1.0 + 0.05 * eased;
                            // Shadow intensifies dramatically
                            let shadow = 0.5 + 0.5 * eased;
                            // Border glow appears
                            let glow = 0.5 * eased;

                            (
                                z,
                                scale,
                                shadow,
                                glow,
                                std::f32::consts::PI,
                                self.opacity * 0.9,
                            )
                        } else if overall_progress < flip_end {
                            // Phase 3: Flip (0.5 - 0.75)
                            let phase_progress =
                                (overall_progress - emerge_end) / (flip_end - emerge_end);
                            let eased = ease_in_out_cubic(phase_progress);

                            // Maintain elevated state
                            let z = 10.0;
                            let scale = 1.05;
                            let shadow = 1.0; // Full shadow
                            let glow = 0.5 * (1.0 - phase_progress); // Fade out glow

                            // Rotation from PI to 0
                            let rotation = std::f32::consts::PI * (1.0 - eased);

                            (z, scale, shadow, glow, rotation, self.opacity)
                        } else {
                            // Phase 4: Settling (0.75 - 1.0)
                            let phase_progress = (overall_progress - flip_end) / (1.0 - flip_end);
                            let eased = ease_out_cubic(phase_progress);

                            // Add subtle overshoot and bounce
                            let bounce_factor = if phase_progress < 0.6 {
                                1.0 - (phase_progress / 0.6) * 0.02 // Slight overshoot
                            } else {
                                0.98 + ((phase_progress - 0.6) / 0.4) * 0.02 // Bounce back
                            };

                            // Z-depth from +10 to 0 with bounce
                            let z = 10.0 * (1.0 - eased);
                            // Scale from 1.05 to 1.0 with bounce
                            let scale = 1.0 + 0.05 * (1.0 - eased) * bounce_factor;
                            // Shadow fades to final state
                            let shadow = 1.0 * (1.0 - eased) + 0.3; // Final shadow = 0.3

                            (z, scale, shadow, 0.0, 0.0, self.opacity)
                        };

                    /*
                    log::debug!("Enhanced flip - phase: {}, progress: {:.3}, z: {:.1}, scale: {:.3}, shadow: {:.3}",
                        if overall_progress < rise_end { "Rising" }
                        else if overall_progress < emerge_end { "Emerging" }
                        else if overall_progress < flip_end { "Flipping" }
                        else { "Settling" },
                        overall_progress, z_depth, scale, shadow_intensity); */

                    (
                        final_opacity,
                        rotation_y,
                        overall_progress,
                        z_depth,
                        scale,
                        shadow_intensity,
                        border_glow,
                    )
                } else {
                    // Animation completed
                    (self.opacity, 0.0, 1.0, 0.0, 1.0, 0.1, 0.0)
                }
            }
        };

        //log::debug!(
        //    "  - Calculated values: opacity={}, rotation_y={}, progress={}, z_depth={}, scale={}",
        //    actual_opacity,
        //    rotation_y,
        //    animation_progress,
        //    z_depth,
        //    scale
        //);

        // Don't try to cache per-primitive data - it causes issues with multiple primitives
        // Just ensure textures are loaded and create instance data fresh each time

        // The bounds parameter is provided by the renderer for layout positioning
        // For animations, we render within these bounds but allow overflow via clipping

        // Convert theme color to linear RGB
        let theme_color_array = [self.theme_color.r, self.theme_color.g, self.theme_color.b];

        //log::debug!("Instance data - theme_color: [{:.3}, {:.3}, {:.3}], opacity: {:.3}, rotation_y: {:.3}, scale: {:.3}, z_depth: {:.3}",
        //    theme_color_array[0], theme_color_array[1], theme_color_array[2], actual_opacity, rotation_y, scale, z_depth);

        // If we have animated bounds, render the poster centered within the larger container
        let (poster_position, poster_size) =
            if let Some(animated_bounds) = self.animated_bounds.as_ref() {
                // The widget bounds are the full container (e.g., 260x360)
                // The poster should be centered within this at its base size (e.g., 200x300)
                // This gives us 30px padding on each side for scaling

                // Calculate centering offset
                let offset_x = (bounds.width - animated_bounds.base_width) / 2.0;
                let offset_y = (bounds.height - animated_bounds.base_height) / 2.0;

                // Position the poster centered within the container
                let poster_x = bounds.x + offset_x;
                let poster_y = bounds.y + offset_y;

                //log::info!(
                //        "Shader render - Container bounds: {:?}, poster base size: {}x{}, centered at: ({}, {})",
                //        bounds,
                //        animated_bounds.base_width,
                //        animated_bounds.base_height,
                //        poster_x,
                //        poster_y
                //    );
                //log::info!(
                //    "  Padding: {}, Scale: {}, Scaled size: {}x{}",
                //    animated_bounds.animation_padding,
                //    scale,
                //    animated_bounds.base_width * scale,
                //    animated_bounds.base_height * scale
                //);

                (
                    [poster_x, poster_y],
                    [animated_bounds.base_width, animated_bounds.base_height],
                )
            } else {
                // No animated bounds - add small padding for border expansion
                // Border can expand up to 0.004 units (normalized), which is ~1% of poster size
                // For a 200px wide poster, that's 2px, so we need at least 2-3px padding
                let border_padding = 3.0; // 3px padding for border expansion

                // Shrink the poster slightly within bounds to allow border to expand outward
                let poster_x = bounds.x + border_padding;
                let poster_y = bounds.y + border_padding;
                let poster_width = bounds.width - (border_padding * 2.0);
                let poster_height = bounds.height - (border_padding * 2.0);

                ([poster_x, poster_y], [poster_width, poster_height])
            };

        // Calculate overlay state based on hover and animation
        // For PlaceholderSunken and no animation, consider animation complete
        let animation_complete = match self.animation {
            AnimationType::None => true,
            AnimationType::PlaceholderSunken => true,
            // Use epsilon comparison for floating point - animation is complete if within 0.001 of 1.0
            _ => animation_progress >= 0.999,
        };

        // For now, use the external hover state
        // TODO: Implement proper internal hover detection
        let show_overlay = if self.is_hovered && animation_complete {
            1.0
        } else {
            0.0
        };

        // Debug hover state when it might fail
        if self.is_hovered && !animation_complete {
            log::debug!(
                "Hover blocked by animation: progress={:.3}",
                animation_progress
            );
        }

        // Always show border regardless of animation state
        let show_border = 1.0;

        // Calculate normalized mouse position (0-1) relative to poster
        let mouse_pos_normalized = if let Some(mouse_pos) = self.mouse_position {
            //log::info!("Prepare: Mouse position available: {:?}", mouse_pos);
            // Account for the current scale of the poster
            let scaled_poster_width = poster_size[0] * scale;
            let scaled_poster_height = poster_size[1] * scale;

            // Calculate the offset from widget bounds to scaled poster position
            // The poster is centered, so we need to account for the scaled size
            let widget_to_poster_offset_x = if let Some(animated_bounds) = &self.animated_bounds {
                (bounds.width - scaled_poster_width) / 2.0
            } else {
                0.0
            };
            let widget_to_poster_offset_y = if let Some(animated_bounds) = &self.animated_bounds {
                (bounds.height - scaled_poster_height) / 2.0
            } else {
                0.0
            };

            // Adjust mouse position to be relative to scaled poster, not widget
            let mouse_x_relative_to_poster = mouse_pos.x - widget_to_poster_offset_x;
            let mouse_y_relative_to_poster = mouse_pos.y - widget_to_poster_offset_y;

            // Now normalize to 0-1 within scaled poster bounds
            let norm_x = mouse_x_relative_to_poster / scaled_poster_width;
            let norm_y = mouse_y_relative_to_poster / scaled_poster_height;

            // Only return valid position if mouse is within poster bounds
            // Add small tolerance for floating-point edge cases
            if norm_x >= -0.01 && norm_x <= 1.01 && norm_y >= -0.01 && norm_y <= 1.01 {
                // Clamp to valid range
                let result = [norm_x.clamp(0.0, 1.0), norm_y.clamp(0.0, 1.0)];
                // Debug individual button areas
                /*
                if self.is_hovered {
                    // Check if mouse is in button areas
                    let center_button =
                        norm_x >= 0.4 && norm_x <= 0.6 && norm_y >= 0.4 && norm_y <= 0.6;
                    let edit_button =
                        norm_x >= 0.79 && norm_x <= 0.91 && norm_y >= 0.09 && norm_y <= 0.21;
                    let dots_button =
                        norm_x >= 0.79 && norm_x <= 0.91 && norm_y >= 0.79 && norm_y <= 0.91;

                    if center_button || edit_button || dots_button {
                        log::info!(
                            "Mouse over button area - normalized: [{:.3}, {:.3}], scale: {:.3}",
                            result[0],
                            result[1],
                            scale
                        );
                    }
                } */
                result
            } else {
                [-1.0, -1.0] // Mouse outside poster
            }
        } else {
            [-1.0, -1.0] // Mouse not over widget
        };

        let instance = Instance {
            position_and_size: [
                poster_position[0],
                poster_position[1],
                poster_size[0],
                poster_size[1],
            ],
            radius_opacity_rotation_anim: [
                self.radius,
                actual_opacity,
                rotation_y,
                animation_progress,
            ],
            theme_color_zdepth: [
                theme_color_array[0],
                theme_color_array[1],
                theme_color_array[2],
                z_depth,
            ],
            scale_shadow_glow_type: [
                scale,
                shadow_intensity,
                border_glow,
                self.animation.as_u32() as f32,
            ],
            hover_overlay_border_progress: [
                if self.is_hovered { 1.0 } else { 0.0 },
                show_overlay,
                show_border,
                self.progress.unwrap_or(-1.0), // -1.0 means no progress bar
            ],
            mouse_pos_and_padding: [mouse_pos_normalized[0], mouse_pos_normalized[1], 0.0, 0.0],
            progress_color_and_padding: [
                self.progress_color.r,
                self.progress_color.g,
                self.progress_color.b,
                0.0,
            ],
        };

        // Create instance buffer - simple and robust
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rounded Image Instance Buffer"),
            size: std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Write instance data to the buffer
        queue.write_buffer(&instance_buffer, 0, bytemuck::cast_slice(&[instance]));

        // Store per-primitive data using primitive address as key
        // Within a single frame, each primitive has a unique address
        let key = self as *const _ as usize;
        state.primitive_data.insert(
            key,
            PrimitiveData {
                instance_buffer,
                texture_bind_group,
            },
        );

        //log::info!("Prepare: primitive {:p}", self);
        //log::info!("  - bounds param: {:?}", bounds);
        //log::info!("  - self.bounds: {:?}", self.bounds);
        //log::info!("  - viewport scale: {}", viewport.scale_factor());
        //log::info!("  - viewport logical_size: {:?}", viewport.logical_size());
        //log::info!("  - viewport physical_size: {:?}", viewport.physical_size());
        //let transform: [f32; 16] = viewport.projection().into();
        //log::info!("  - transform matrix: [{:.3}, {:.3}, {:.3}, {:.3}]", transform[0], transform[1], transform[2], transform[3]);
        //log::info!("                      [{:.3}, {:.3}, {:.3}, {:.3}]", transform[4], transform[5], transform[6], transform[7]);
        //log::info!("                      [{:.3}, {:.3}, {:.3}, {:.3}]", transform[8], transform[9], transform[10], transform[11]);
        //log::info!("                      [{:.3}, {:.3}, {:.3}, {:.3}]", transform[12], transform[13], transform[14], transform[15]);
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        storage: &Storage,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        #[cfg(any(feature = "profile-with-puffin", feature = "profile-with-tracy", feature = "profile-with-tracing"))]
        profiling::scope!(crate::infrastructure::profiling_scopes::scopes::VIEW_DRAW);
        
        let pipeline = storage.get::<Pipeline>().unwrap();
        let state = storage.get::<State>().unwrap();

        // Get globals bind group
        let Some(globals_bind_group) = &state.globals_bind_group else {
            log::warn!("Globals bind group not initialized");
            return;
        };

        // Get per-primitive data using primitive address as key
        let key = self as *const _ as usize;
        let Some(primitive_data) = state.primitive_data.get(&key) else {
            log::warn!("No data for primitive {:p}", self);
            return;
        };

        //log::info!("Render: primitive {:p} bounds = {:?}", self, self.bounds);

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

        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);
        render_pass.set_bind_group(1, &primitive_data.texture_bind_group, &[]);
        render_pass.set_vertex_buffer(0, primitive_data.instance_buffer.slice(..));

        // Calculate proper scissor rect based on animated bounds
        let (scissor_x, scissor_y, scissor_width, scissor_height) =
            if let Some(animated_bounds) = &self.animated_bounds {
                // Use expanded bounds for animations
                // Use the maximum padding to ensure scissor rect is large enough
                let padding = animated_bounds
                    .horizontal_padding
                    .max(animated_bounds.vertical_padding);

                // For animated posters, we simply use the full clip bounds
                // The animation overflow is handled by the larger container size
                // established in the layout phase (260x360 for a 200x300 poster)
                // This ensures the scissor rect is large enough for scaling without
                // trying to expand beyond the valid render target bounds

                // Use the full clip bounds - the container is already sized to accommodate scaling
                (
                    clip_bounds.x,
                    clip_bounds.y,
                    clip_bounds.width.max(1),
                    clip_bounds.height.max(1),
                )
            } else {
                // Use original clip bounds for non-animated posters
                (
                    clip_bounds.x,
                    clip_bounds.y,
                    clip_bounds.width.max(1),
                    clip_bounds.height.max(1),
                )
            };

        //log::info!(
        //    "Setting scissor rect: ({}, {}, {}, {}) for bounds {:?}",
        //    scissor_x,
        //    scissor_y,
        //    scissor_width,
        //    scissor_height,
        //    clip_bounds
        //);

        render_pass.set_scissor_rect(scissor_x, scissor_y, scissor_width, scissor_height);

        // Draw quad (4 vertices) with 1 instance
        render_pass.draw(0..4, 0..1);
    }
}

/// A widget that displays an image with rounded corners using GPU shaders
pub struct RoundedImage {
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
    /// Creates a new rounded image
    pub fn new(handle: Handle) -> Self {
        use crate::domains::ui::theme::MediaServerTheme;

        Self {
            handle,
            radius: 8.0,
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
        if self.load_time.is_none() {
            self.load_time = Some(Instant::now());
        }
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
pub fn rounded_image_shader(handle: Handle) -> RoundedImage {
    RoundedImage::new(handle)
}

impl<'a> From<RoundedImage> for Element<'a, Message> {
    fn from(image: RoundedImage) -> Self {
        // Debug log the widget dimensions
        //if let Some(bounds) = &image.bounds {
        //    let (layout_width, layout_height) = bounds.layout_bounds();
        //log::info!(
        //    "Creating shader widget with dimensions - width: {:?}, height: {:?}, layout_bounds: {}x{}",
        //    image.width, image.height, layout_width, layout_height
        //);
        //}

        let shader = iced::widget::shader(RoundedImageProgram {
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
