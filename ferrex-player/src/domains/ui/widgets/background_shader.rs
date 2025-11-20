//! GPU-accelerated background shader widget for Ferrex media player
//!
//! This widget creates visually appealing animated backgrounds with gradients,
//! depth effects, and subtle animations for a professional streaming app experience.

pub use crate::domains::ui::messages::Message;
use crate::domains::ui::types::BackdropAspectMode;
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::Viewport;
use iced::advanced::image::Id as ImageId;
use iced::widget::shader::{self, Primitive, Program, Storage};
use iced::{mouse, wgpu, Color, Element, Length, Point, Rectangle, Size, Vector};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

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
    /// Backdrop image with gradient fade
    BackdropGradient {
        image_handle: Option<iced::widget::image::Handle>,
        fade_start: f32, // Where fade starts (0.0 = top, 1.0 = bottom)
        fade_end: f32,   // Where fade ends completely
    },
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
            animation_fps: 60,
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

    // Transition colors
    prev_primary_color: [f32; 4],   // offset 144, size 16
    prev_secondary_color: [f32; 4], // offset 160, size 16

    // Transition parameters
    transition_params: [f32; 4], // transition_progress, backdrop_opacity, backdrop_slide_offset, backdrop_scale (offset 176, size 16)

    // Gradient and depth
    gradient_center: [f32; 4], // gradient_center.x, gradient_center.y, 0, 0 (offset 192, size 16)
    depth_params: [f32; 4], // region_count, base_depth, shadow_intensity, shadow_distance (offset 208, size 16)
    ambient_light: [f32; 4], // light_dir.x, light_dir.y, 0, 0 (offset 224, size 16)

    // Depth regions (up to 8)
    region1_bounds: [f32; 4], // x, y, width, height (offset 240, size 16)
    region1_depth_params: [f32; 4], // depth, edge_transition_type, edge_width, shadow_enabled (offset 256, size 16)
    region1_shadow_params: [f32; 4], // shadow_intensity, z_order, border_width, border_opacity (offset 272, size 16)
    region1_border_color: [f32; 4],  // r, g, b, a (offset 288, size 16)

    region2_bounds: [f32; 4],        // (offset 304, size 16)
    region2_depth_params: [f32; 4],  // (offset 320, size 16)
    region2_shadow_params: [f32; 4], // (offset 336, size 16)
    region2_border_color: [f32; 4],  // (offset 352, size 16)

    region3_bounds: [f32; 4],        // (offset 368, size 16)
    region3_depth_params: [f32; 4],  // (offset 384, size 16)
    region3_shadow_params: [f32; 4], // (offset 400, size 16)
    region3_border_color: [f32; 4],  // (offset 416, size 16)

    region4_bounds: [f32; 4],        // (offset 432, size 16)
    region4_depth_params: [f32; 4],  // (offset 448, size 16)
    region4_shadow_params: [f32; 4], // (offset 464, size 16)
    region4_border_color: [f32; 4],  // (offset 480, size 16)

                                     // Total: 496 bytes (31 * 16)
}

// Compile-time assertion to verify our struct size
const _: () = {
    let size = std::mem::size_of::<Globals>();
    assert!(size == 496, "Globals struct size mismatch");
};

/// Pipeline state
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    texture_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
    default_texture: Arc<wgpu::Texture>,
    default_texture_bind_group: Arc<wgpu::BindGroup>,
}

/// Per-primitive render data
struct PrimitiveData {
    texture_bind_group: Option<wgpu::BindGroup>,
}

/// Texture info
struct TextureInfo {
    texture: Arc<wgpu::Texture>,
    aspect_ratio: f32, // width / height
}

/// Shared state
struct State {
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    // Texture cache for backdrops
    texture_cache: HashMap<ImageId, TextureInfo>,
    texture_bind_groups: HashMap<ImageId, wgpu::BindGroup>,
    // Per-primitive data for current frame
    primitive_data: HashMap<usize, PrimitiveData>,
    // Track which primitives were prepared this frame
    prepared_primitives: HashSet<usize>,
    // Track if default texture has been initialized
    default_texture_initialized: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            globals_buffer: None,
            globals_bind_group: None,
            texture_cache: HashMap::new(),
            texture_bind_groups: HashMap::new(),
            primitive_data: HashMap::new(),
            prepared_primitives: HashSet::new(),
            default_texture_initialized: false,
        }
    }
}

impl State {
    /// Remove primitive data for primitives that weren't prepared this frame
    fn trim(&mut self) {
        self.primitive_data
            .retain(|key, _| self.prepared_primitives.contains(key));
        // Clear the prepared set for the next frame
        self.prepared_primitives.clear();
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&shader_label),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/background.wgsl").into()),
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

        // Create texture bind group layout
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Background Texture"),
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
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Create default texture bind group
        let default_texture_view =
            default_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let default_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Default Texture Bind Group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&default_texture_view),
                },
            ],
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[&globals_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
            default_texture_bind_group: Arc::new(default_texture_bind_group),
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
                log::error!("Failed to load backdrop from path {:?}: {}", path, e);
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
        format: wgpu::TextureFormat::Rgba8Unorm, // Match rounded_image_shader format
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

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
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let aspect_ratio = width as f32 / height as f32;
    Some((Arc::new(texture), aspect_ratio))
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
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        storage: &mut Storage,
        _bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        // Initialize pipeline if needed
        if !storage.has::<Pipeline>() {
            storage.store(Pipeline::new(device, format));
        }

        // Initialize state if needed
        if !storage.has::<State>() {
            storage.store(State::default());
        }

        // Write transparent pixel to default texture on first prepare
        let pipeline = storage.get::<Pipeline>().unwrap();
        if !storage.get::<State>().unwrap().default_texture_initialized {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &pipeline.default_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[0u8, 0u8, 0u8, 0u8], // Transparent black pixel
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
            storage
                .get_mut::<State>()
                .unwrap()
                .default_texture_initialized = true;
        }

        let (globals_layout, texture_layout, sampler) = {
            let pipeline = storage.get::<Pipeline>().unwrap();
            (
                pipeline.globals_bind_group_layout.clone(),
                pipeline.texture_bind_group_layout.clone(),
                pipeline.sampler.clone(),
            )
        };

        let state = storage.get_mut::<State>().unwrap();

        // Create globals buffer if needed
        if state.globals_buffer.is_none() {
            // WGSL uniform buffer alignment
            // Using all vec4 types ensures proper alignment
            const EXPECTED_SIZE: u64 = 496; // 31 * 16 bytes

            let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Background Globals"),
                size: EXPECTED_SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Background Globals Bind Group"),
                layout: &globals_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buffer.as_entire_binding(),
                }],
            });

            state.globals_buffer = Some(globals_buffer);
            state.globals_bind_group = Some(globals_bind_group);
        }

        // Handle texture loading for backdrop (either from BackdropGradient effect or from backdrop_handle)
        let backdrop_handle = match &self.effect {
            BackgroundEffect::BackdropGradient {
                image_handle: Some(handle),
                ..
            } => Some(handle),
            _ => self.backdrop_handle.as_ref(),
        };

        if let Some(handle) = backdrop_handle {
            let image_id = handle.id();

            // Load texture if not cached
            if !state.texture_cache.contains_key(&image_id) {
                if let Some((texture, aspect_ratio)) = load_texture(device, queue, handle) {
                    state.texture_cache.insert(
                        image_id,
                        TextureInfo {
                            texture,
                            aspect_ratio,
                        },
                    );
                }
            }

            // Create texture bind group if texture is available
            if state.texture_cache.contains_key(&image_id)
                && !state.texture_bind_groups.contains_key(&image_id)
            {
                let texture_info = state.texture_cache.get(&image_id).unwrap();
                let texture_view = texture_info
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Background Texture Bind Group"),
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
        }

        // Calculate animation time
        let time = self.start_time.elapsed().as_secs_f32();

        // Convert effect type to float
        let effect_type = match &self.effect {
            BackgroundEffect::Solid => 0.0,
            BackgroundEffect::Gradient => 1.0,
            BackgroundEffect::SubtleNoise { .. } => 2.0,
            BackgroundEffect::FloatingParticles { .. } => 3.0,
            BackgroundEffect::WaveRipple { .. } => 4.0,
            BackgroundEffect::BackdropGradient { image_handle, .. } => 5.0,
        };

        // Extract effect parameters
        let (effect_param1, effect_param2) = match &self.effect {
            BackgroundEffect::SubtleNoise { scale, speed } => (*scale, *speed),
            BackgroundEffect::FloatingParticles { count, size } => (*count as f32, *size),
            BackgroundEffect::WaveRipple {
                frequency,
                amplitude,
            } => (*frequency, *amplitude),
            BackgroundEffect::BackdropGradient {
                fade_start,
                fade_end,
                ..
            } => (*fade_start, *fade_end),
            _ => (0.0, 0.0),
        };

        // Update globals
        let transform: [f32; 16] = viewport.projection().into();

        // Debug log scroll offset
        if self.scroll_offset != 0.0 {
            log::debug!(
                "BackgroundShader prepare: scroll_offset = {}",
                self.scroll_offset
            );
        }

        let mut globals = Globals {
            transform,
            time_and_resolution: [
                time,
                0.0,
                viewport.logical_size().width as f32,
                viewport.logical_size().height as f32,
            ],
            scale_and_effect: [
                viewport.scale_factor() as f32,
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
            texture_params: [
                1.0, // Default texture aspect ratio, will be updated below
                self.scroll_offset,
                self.header_offset,
                match self.backdrop_aspect_mode {
                    BackdropAspectMode::Auto => 0.0,
                    BackdropAspectMode::Force21x9 => 1.0,
                },
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
            gradient_center: [self.gradient_center.0, self.gradient_center.1, 0.0, 0.0],
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
            // Initialize regions (all zeros)
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

        // Populate depth regions (up to 4)
        //log::debug!(
        //    "Populating {} depth regions into shader globals",
        //    self.depth_layout.regions.len()
        //);
        for (i, region) in self.depth_layout.regions.iter().take(4).enumerate() {
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
                _ => unreachable!(),
            }
        }

        // Update texture aspect ratio for any backdrop
        let backdrop_handle = match &self.effect {
            BackgroundEffect::BackdropGradient {
                image_handle: Some(handle),
                ..
            } => Some(handle),
            _ => self.backdrop_handle.as_ref(),
        };

        if let Some(handle) = backdrop_handle {
            let image_id = handle.id();
            if let Some(texture_info) = state.texture_cache.get(&image_id) {
                globals.texture_params[0] = texture_info.aspect_ratio;
            }
        }

        queue.write_buffer(
            state.globals_buffer.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&[globals]),
        );

        // Store per-primitive data using stable program ID
        let key = self.program_id;

        // Determine which texture bind group to use
        let backdrop_handle = match &self.effect {
            BackgroundEffect::BackdropGradient {
                image_handle: Some(handle),
                ..
            } => Some(handle),
            _ => self.backdrop_handle.as_ref(),
        };

        let texture_bind_group = backdrop_handle.and_then(|handle| {
            let image_id = handle.id();
            state.texture_bind_groups.get(&image_id).cloned()
        });

        state
            .primitive_data
            .insert(key, PrimitiveData { texture_bind_group });
        state.prepared_primitives.insert(key);

        // Clean up stale primitive data from previous frames
        state.trim();
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        storage: &Storage,
        target: &wgpu::TextureView,
        _clip_bounds: &Rectangle<u32>,
    ) {
        let pipeline = storage.get::<Pipeline>().unwrap();
        let state = storage.get::<State>().unwrap();

        let Some(globals_bind_group) = &state.globals_bind_group else {
            return;
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Background Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load, // Preserve existing content
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);

        // Get per-primitive data using stable program ID
        let key = self.program_id;

        let primitive_data = state.primitive_data.get(&key);

        // Bind texture based on per-primitive data
        if let Some(data) = primitive_data {
            if let Some(texture_bind_group) = &data.texture_bind_group {
                render_pass.set_bind_group(1, texture_bind_group, &[]);
            } else {
                // Use default texture
                render_pass.set_bind_group(1, &*pipeline.default_texture_bind_group, &[]);
            }
        } else {
            // No primitive data found
            render_pass.set_bind_group(1, &*pipeline.default_texture_bind_group, &[]);
        }

        // Draw full-screen quad (4 vertices)
        render_pass.draw(0..4, 0..1);
    }
}

/// Background shader widget
pub struct BackgroundShader {
    effect: BackgroundEffect,
    theme: BackgroundTheme,
    quality: QualitySettings,
    primary_color: Color,
    secondary_color: Color,
    start_time: Instant,
    scroll_offset: f32,
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
        use crate::domains::ui::theme::MediaServerTheme;
        use crate::domains::ui::transitions::generate_random_gradient_center;

        let primary = MediaServerTheme::SOFT_GREY_DARK;
        let secondary = MediaServerTheme::SOFT_GREY_LIGHT;

        Self {
            effect: BackgroundEffect::Gradient,
            theme: BackgroundTheme::Cinematic,
            quality: QualitySettings::default(),
            primary_color: primary,
            secondary_color: secondary,
            start_time: Instant::now(),
            scroll_offset: 0.0,
            // Initialize transitions
            prev_primary_color: primary,
            prev_secondary_color: secondary,
            transition_progress: 1.0,
            backdrop_opacity: 1.0,
            backdrop_slide_offset: 0.0,
            backdrop_scale: 1.0,
            gradient_center: generate_random_gradient_center(),
            backdrop_handle: None,
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

    /// Set backdrop image with gradient fade
    pub fn backdrop(mut self, handle: iced::widget::image::Handle) -> Self {
        self.backdrop_handle = Some(handle);
        self
    }

    /// Set the backdrop aspect mode
    pub fn backdrop_aspect_mode(mut self, mode: BackdropAspectMode) -> Self {
        self.backdrop_aspect_mode = mode;
        self
    }

    /// Set backdrop with custom fade parameters
    pub fn backdrop_with_fade(
        mut self,
        handle: iced::widget::image::Handle,
        fade_start: f32,
        fade_end: f32,
    ) -> Self {
        self.effect = BackgroundEffect::BackdropGradient {
            image_handle: Some(handle),
            fade_start,
            fade_end,
        };
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
            let secondary =
                Color::from_rgb((r * 1.2).min(1.0), (g * 1.2).min(1.0), (b * 1.2).min(1.0));

            self.secondary_color = secondary;
        } else {
            // Fallback to default theme colors
            use crate::domains::ui::theme::MediaServerTheme;
            self.primary_color = MediaServerTheme::BLACK;
            self.secondary_color = MediaServerTheme::ACCENT_BLUE.scale_alpha(0.2);
        }
        self
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

impl<'a> From<BackgroundShader> for Element<'a, Message> {
    fn from(background: BackgroundShader) -> Self {
        // Generate a unique ID for this background shader instance
        let id = BACKGROUND_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        iced::widget::shader(BackgroundShaderProgram {
            effect: background.effect,
            theme: background.theme,
            quality: background.quality,
            primary_color: background.primary_color,
            secondary_color: background.secondary_color,
            start_time: background.start_time,
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
            id,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
