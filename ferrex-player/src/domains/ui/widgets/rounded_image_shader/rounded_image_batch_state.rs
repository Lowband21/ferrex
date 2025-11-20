//! Batch state implementation for RoundedImagePrimitive
//!
//! This module implements the PrimitiveBatchState trait for batched rendering
//! of rounded images, reducing draw calls from O(n) to O(1).

use crate::infrastructure::performance_config::texture_upload::{
    DEFERRED_QUEUE_MAX_SIZE, MAX_UPLOADS_PER_FRAME,
};
use bytemuck::{Pod, Zeroable};
use iced::widget::image::Handle;
use iced_wgpu::primitive::{buffer_manager::InstanceBufferManager, PrimitiveBatchState};
use iced_wgpu::{core::Rectangle, graphics, image, wgpu};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use wgpu::util::StagingBelt;

use super::create_batch_instance;

/// Instance data for each rounded image - must match shader layout exactly
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RoundedImageInstance {
    // vec4: position.xy, size.xy
    pub position_and_size: [f32; 4],
    // vec4: radius, opacity, rotation_y, animation_progress
    pub radius_opacity_rotation_anim: [f32; 4],
    // vec4: theme_color.rgb, z_depth
    pub theme_color_zdepth: [f32; 4],
    // vec4: scale, shadow_intensity, border_glow, animation_type
    pub scale_shadow_glow_type: [f32; 4],
    // vec4: is_hovered, show_overlay, show_border, progress
    pub hover_overlay_border_progress: [f32; 4],
    // vec4: mouse_position.xy, unused, unused
    pub mouse_pos_and_padding: [f32; 4],
    // vec4: progress_color.rgb, unused
    pub progress_color_and_padding: [f32; 4],
    // vec4: atlas_uv_min.xy, atlas_uv_max.xy
    pub atlas_uvs: [f32; 4],
    // vec4: atlas_layer, unused, unused, unused
    pub atlas_layer_and_padding: [f32; 4],
}

/// Deferred upload entry for images that exceeded frame budget
#[derive(Clone)]
struct DeferredUpload {
    instance: RoundedImageInstance,
    handle: Handle,
}

/// Batch state for rounded image primitives
pub struct RoundedImageBatchState {
    /// Buffer manager for instance data
    instance_manager: InstanceBufferManager<RoundedImageInstance>,
    /// Pending instances (without handles)
    pending_instances: Vec<RoundedImageInstance>,
    /// Render pipeline (shared)
    render_pipeline: Arc<wgpu::RenderPipeline>,
    /// Globals buffer for viewport transform
    globals_buffer: Option<wgpu::Buffer>,
    /// Globals bind group
    globals_bind_group: Option<wgpu::BindGroup>,
    /// Bind group layouts
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    atlas_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    /// Sampler for texture filtering
    sampler: Arc<wgpu::Sampler>,
    /// Number of texture uploads in current frame
    uploads_this_frame: u32,
    /// Queue of uploads deferred due to frame budget
    deferred_queue: VecDeque<DeferredUpload>,

    pub loaded_times: HashMap<u64, Instant>,
}

/// Global uniform data (viewport transform)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16], // 4x4 matrix = 64 bytes
    scale_factor: f32,    // 4 bytes
    _padding: [f32; 7],   // Padding to make total 96 bytes
}

impl RoundedImageBatchState {
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn add_instance(
        &mut self,
        id: u64,
        mut instance: RoundedImageInstance,
        handle: &Handle,
        image_cache: &mut image::Cache,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        cached: bool,
    ) {
        //let cached = image_cache.contains(&handle);

        if !cached {
            self.uploads_this_frame += 1;
        }

        if self.uploads_this_frame > MAX_UPLOADS_PER_FRAME && !cached {
            self.pending_instances.push(instance);
            return;
        }

        if cached {
            // Check AnimationType not None
            if instance.scale_shadow_glow_type[3] as u32 != 0 {
                if !self.loaded_times.contains_key(&id) {
                    self.loaded_times.insert(id, Instant::now());
                }
            }
        }

        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("UI::RoundedImageBatchState::UploadOrLoad");

        let atlas_entry = image_cache.upload_raster(device, encoder, handle);

        let (uv_min, uv_max, layer) = if let Some(entry) = atlas_entry {
            match entry {
                image::atlas::Entry::Contiguous(allocation) => {
                    let (x, y) = allocation.position();
                    let size = allocation.size();
                    let layer = allocation.layer() as u32;
                    const ATLAS_SIZE: f32 = image::atlas::SIZE as f32;
                    let uv_min = [x as f32 / ATLAS_SIZE, y as f32 / ATLAS_SIZE];
                    let uv_max = [
                        (x + size.width) as f32 / ATLAS_SIZE,
                        (y + size.height) as f32 / ATLAS_SIZE,
                    ];
                    (uv_min, uv_max, layer)
                }
                image::atlas::Entry::Fragmented { size, fragments } => {
                    if let Some(first) = fragments.first() {
                        let (x, y) = first.position;
                        let layer = first.allocation.layer() as u32;
                        const ATLAS_SIZE: f32 = image::atlas::SIZE as f32;
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

        instance.atlas_uvs = [uv_min[0], uv_min[1], uv_max[0], uv_max[1]];
        instance.atlas_layer_and_padding = [layer as f32, 0.0, 0.0, 0.0];

        self.pending_instances.push(instance);
    }
}

impl std::fmt::Debug for RoundedImageBatchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoundedImageBatchState")
            .field("instance_count", &self.instance_manager.instance_count())
            .field("pending_count", &self.pending_instances.len())
            .finish()
    }
}

impl PrimitiveBatchState for RoundedImageBatchState {
    type InstanceData = RoundedImageInstance;

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self
    where
        Self: Sized,
    {
        log::info!("Creating RoundedImageBatchState for batched rendering");

        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rounded Image Shader (Batched)"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/rounded_image.wgsl").into(),
            ),
        });

        // Create globals bind group layout
        let globals_bind_group_layout = Arc::new(device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Globals (Batched)"),
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
            },
        ));

        // Create atlas bind group layout to match iced's atlas
        let atlas_bind_group_layout = Arc::new(device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                label: Some("iced_wgpu::image texture atlas layout (Batched)"),
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
            },
        ));

        // Create sampler
        let sampler = Arc::new(device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rounded Image Sampler (Batched)"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }));

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rounded Image Pipeline Layout (Batched)"),
            bind_group_layouts: &[&globals_bind_group_layout, &atlas_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create vertex buffer layout for instance data
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RoundedImageInstance>() as u64,
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
        let render_pipeline = Arc::new(device.create_render_pipeline(
            &wgpu::RenderPipelineDescriptor {
                label: Some("Rounded Image Pipeline (Batched)"),
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
            },
        ));

        Self {
            instance_manager: InstanceBufferManager::new(),
            pending_instances: Vec::new(),
            render_pipeline,
            globals_buffer: None,
            globals_bind_group: None,
            globals_bind_group_layout,
            atlas_bind_group_layout,
            sampler,
            uploads_this_frame: 0,
            deferred_queue: VecDeque::new(),
            loaded_times: HashMap::new(),
        }
    }

    fn add_instance(&mut self, instance: Self::InstanceData) {
        // This shouldn't be called directly anymore
        self.instance_manager.add_instance(instance);
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
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        belt: &mut StagingBelt,
        image_cache: &mut image::Cache,
        viewport: &graphics::Viewport,
        scale_factor: f32,
    ) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("RoundedImageBatchState::prepare");

        // Move pending instances to the instance manager
        for instance in self.pending_instances.drain(..) {
            self.instance_manager.add_instance(instance);
        }

        // Upload instance data to GPU
        if self
            .instance_manager
            .upload(device, encoder, belt)
            .is_none()
        {
            // No instances to prepare
            return;
        }

        // Update globals buffer with viewport transform
        let globals = Globals {
            transform: viewport.projection().into(),
            scale_factor,
            _padding: [0.0; 7],
        };

        // Create or update globals buffer
        if self.globals_buffer.is_none() {
            self.globals_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Rounded Image Globals Buffer (Batched)"),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        if let Some(buffer) = &self.globals_buffer {
            belt.write_buffer(
                encoder,
                buffer,
                0,
                wgpu::BufferSize::new(std::mem::size_of::<Globals>() as u64).unwrap(),
                device,
            )
            .copy_from_slice(bytemuck::bytes_of(&globals));

            // Create globals bind group if needed
            if self.globals_bind_group.is_none() {
                self.globals_bind_group =
                    Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Rounded Image Globals Bind Group (Batched)"),
                        layout: &self.globals_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                        ],
                    }));
            }
        }

        log::trace!(
            "RoundedImageBatchState prepared {} instances",
            self.instance_manager.instance_count()
        );
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn render<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        image_cache: &'a image::Cache,
        scissor_rect: Rectangle<u32>,
    ) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("RoundedImageBatchState::render");

        let instance_count = self.instance_manager.instance_count();
        if instance_count == 0 {
            return; // Nothing to render
        }

        // Get the instance buffer
        let Some(instance_buffer) = self.instance_manager.buffer() else {
            return;
        };

        // Get the globals bind group
        let Some(globals_bind_group) = &self.globals_bind_group else {
            return;
        };

        // Get the atlas bind group from iced's image cache
        let atlas_bind_group = image_cache.bind_group();
        // Set pipeline and bind groups
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);
        render_pass.set_bind_group(1, atlas_bind_group, &[]);

        // Set scissor rect
        render_pass.set_scissor_rect(
            scissor_rect.x,
            scissor_rect.y,
            scissor_rect.width,
            scissor_rect.height,
        );

        // Bind instance buffer
        render_pass.set_vertex_buffer(0, instance_buffer.slice(..));

        // Draw all instances with a single draw call
        render_pass.draw(0..4, 0..instance_count as u32);

        log::trace!(
            "RoundedImageBatchState rendered {} instances",
            instance_count
        );
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn trim(&mut self) {
        self.instance_manager.clear();
        self.pending_instances.clear();

        // Log frame statistics before reset
        #[cfg(debug_assertions)]
        if self.uploads_this_frame > 0 || !self.deferred_queue.is_empty() {
            log::debug!(
                "Frame complete: {} uploads performed, {} items deferred",
                self.uploads_this_frame,
                self.deferred_queue.len()
            );
        }

        // Reset frame counter for next frame
        self.uploads_this_frame = 0;
    }

    fn instance_count(&self) -> usize {
        self.instance_manager.instance_count()
            + self.instance_manager.pending_count()
            + self.pending_instances.len()
    }
}
