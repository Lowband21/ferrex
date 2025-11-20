//! Shader-based rounded image widget for Iced
//!
//! This implementation uses GPU shaders for true rounded rectangle clipping
//! with anti-aliasing, providing better performance than Canvas-based approaches.

use crate::Message;
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::core::image;
use iced::advanced::graphics::Viewport;
use iced::wgpu;
use iced::widget::image::Handle;
use iced::widget::shader::Program;
use iced::widget::shader::{Primitive, Storage};
use iced::{mouse, Element, Length, Rectangle};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

// Image loading functions are in the image crate root

/// Animation type for poster loading
#[derive(Debug, Clone, Copy)]
pub enum AnimationType {
    None,
    Fade { duration: Duration },
    Flip { duration: Duration },
}

impl AnimationType {
    fn as_u32(&self) -> u32 {
        match self {
            AnimationType::None => 0,
            AnimationType::Fade { .. } => 1,
            AnimationType::Flip { .. } => 2,
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
}

impl<Message> Program<Message> for RoundedImageProgram {
    type State = ();
    type Primitive = RoundedImagePrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        RoundedImagePrimitive {
            image_handle: self.handle.clone(),
            bounds,
            radius: self.radius,
            animation: self.animation,
            load_time: self.load_time,
            opacity: self.opacity,
        }
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
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    position: [f32; 2],    // Top-left position
    size: [f32; 2],        // Width and height
    radius: f32,           // Corner radius
    opacity: f32,          // Opacity for fade animations (0.0 to 1.0)
    rotation_y: f32,       // Y-axis rotation for flip animation (in radians)
    animation_progress: f32, // Animation progress (0.0 to 1.0)
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

/// Shared state for all rounded images
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
    // Track which primitives were prepared this frame
    prepared_primitives: HashSet<usize>,
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
        }
    }
}

impl State {
    fn trim(&mut self) {
        // Clear data for primitives that weren't prepared this frame
        self.primitive_data
            .retain(|key, _| self.prepared_primitives.contains(key));
        self.prepared_primitives.clear();
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
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // size
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // radius
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                // opacity
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
                // rotation_y
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
                // animation_progress
                wgpu::VertexAttribute {
                    offset: 28,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
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

/// Load texture helper function
fn load_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    handle: &Handle,
) -> Option<Arc<wgpu::Texture>> {
    log::trace!("Loading texture for handle {:?}", handle.id());

    // Load the actual image data from the handle
    let (image_data, width, height) = match handle {
        Handle::Path(_, path) => {
            log::info!("Loading image from path: {:?}", path);
            match ::image::open(path) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    (rgba.into_raw(), w, h)
                }
                Err(e) => {
                    log::error!("Failed to load image from path {:?}: {}", path, e);
                    return None;
                }
            }
        }
        Handle::Bytes(_, bytes) => {
            log::info!("Loading image from {} bytes", bytes.len());
            match ::image::load_from_memory(bytes) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    (rgba.into_raw(), w, h)
                }
                Err(e) => {
                    log::error!("Failed to load image from bytes: {}", e);
                    return None;
                }
            }
        }
        Handle::Rgba {
            pixels,
            width,
            height,
            ..
        } => {
            log::info!("Loading RGBA image {}x{}", width, height);
            // RGBA format is always 4 bytes per pixel
            (pixels.to_vec(), *width, *height)
        }
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Rounded Image Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
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

    Some(Arc::new(texture))
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
        // Initialize pipeline if needed
        if !storage.has::<Pipeline>() {
            log::info!("Creating rounded image shader pipeline");
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

        // Load texture if needed
        let image_id = self.image_handle.id();

        if !state.texture_cache.contains_key(&image_id) {
            if let Some(texture) = load_texture(device, queue, &self.image_handle) {
                state.texture_cache.insert(image_id, texture);
            } else {
                log::error!("Failed to load texture for image {:?}", image_id);
                return;
            }
        }

        // Create texture bind group if needed
        if !state.texture_bind_groups.contains_key(&image_id) {
            let texture = state.texture_cache.get(&image_id).unwrap();
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

        // Create instance data for this primitive
        // Use the bounds parameter which includes scroll transformation
        
        // Calculate animation values based on animation type
        let (actual_opacity, rotation_y, animation_progress) = match self.animation {
            AnimationType::None => (self.opacity, 0.0, 1.0),
            AnimationType::Fade { duration } => {
                if let Some(load_time) = self.load_time {
                    let elapsed = load_time.elapsed().as_secs_f32();
                    let progress = (elapsed / duration.as_secs_f32()).min(1.0);
                    (self.opacity, 0.0, progress)
                } else {
                    (self.opacity, 0.0, 1.0)
                }
            }
            AnimationType::Flip { duration } => {
                if let Some(load_time) = self.load_time {
                    let elapsed = load_time.elapsed().as_secs_f32();
                    let progress = (elapsed / duration.as_secs_f32()).min(1.0);
                    let rotation = progress * std::f32::consts::PI; // 0 to PI (180 degrees)
                    // Opacity is handled by AnimatePoster, just pass through
                    (self.opacity, rotation, progress)
                } else {
                    (self.opacity, 0.0, 0.0)
                }
            }
        };
        
        let instance = Instance {
            position: [bounds.x, bounds.y],
            size: [bounds.width, bounds.height],
            radius: self.radius,
            opacity: actual_opacity,
            rotation_y,
            animation_progress,
        };

        // Create or update instance buffer for this primitive
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rounded Image Instance Buffer"),
            size: std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Write instance data
        queue.write_buffer(&instance_buffer, 0, bytemuck::cast_slice(&[instance]));

        // Store per-primitive data
        let key = self as *const _ as usize;
        let texture_bind_group = state.texture_bind_groups.get(&image_id).unwrap().clone();
        state.primitive_data.insert(
            key,
            PrimitiveData {
                instance_buffer,
                texture_bind_group,
            },
        );
        state.prepared_primitives.insert(key);

        //log::info!("Prepare: primitive {:p} using bounds = {:?} (was self.bounds = {:?}), viewport scale = {}", self, bounds, self.bounds, viewport.scale_factor());
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        storage: &Storage,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let pipeline = storage.get::<Pipeline>().unwrap();
        let state = storage.get::<State>().unwrap();

        // Get globals bind group
        let Some(globals_bind_group) = &state.globals_bind_group else {
            log::warn!("Globals bind group not initialized");
            return;
        };

        // Get per-primitive data
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
        render_pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );

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
}

impl RoundedImage {
    /// Creates a new rounded image
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            radius: 8.0,
            width: Length::Fixed(200.0),
            height: Length::Fixed(300.0),
            animation: AnimationType::None,
            load_time: None,
            opacity: 1.0,
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
}

/// Helper function to create a rounded image widget
pub fn rounded_image_shader(handle: Handle) -> RoundedImage {
    RoundedImage::new(handle)
}

impl<'a> From<RoundedImage> for Element<'a, Message> {
    fn from(image: RoundedImage) -> Self {
        iced::widget::shader(RoundedImageProgram {
            handle: image.handle,
            radius: image.radius,
            animation: image.animation,
            load_time: image.load_time,
            opacity: image.opacity,
        })
        .width(image.width)
        .height(image.height)
        .into()
    }
}
