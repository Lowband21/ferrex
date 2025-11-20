//! Batch state implementation for `RoundedImagePrimitive`.
//!
//! This module owns the GPU resources required to render all rounded poster
//! primitives in a single instanced draw call. Instances are accumulated during
//! the widget `encode_batch` phase and lazily uploaded during `prepare` once the
//! frame budget and texture cache state are known.

use super::{
    create_batch_instance, create_placeholder_instance, AnimatedPosterBounds, AnimationType,
};
use crate::infrastructure::constants::performance_config::texture_upload::MAX_UPLOADS_PER_FRAME;
use bytemuck::{Pod, Zeroable};
use iced::widget::image::Handle;
use iced::{Color, Point, Rectangle as LayoutRect};
use iced_wgpu::primitive::{
    buffer_manager::InstanceBufferManager, PrepareContext, PrimitiveBatchState, RenderContext,
};
use iced_wgpu::{core, wgpu, AtlasRegion};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// GPU instance payload for a rounded image primitive.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RoundedImageInstance {
    pub position_and_size: [f32; 4],
    pub radius_opacity_rotation_anim: [f32; 4],
    pub theme_color_zdepth: [f32; 4],
    pub scale_shadow_glow_type: [f32; 4],
    pub hover_overlay_border_progress: [f32; 4],
    pub mouse_pos_and_padding: [f32; 4],
    pub progress_color_and_padding: [f32; 4],
    pub atlas_uvs: [f32; 4],
    pub atlas_layer_and_padding: [f32; 4],
}

/// Batched primitive metadata captured during encoding.
#[derive(Debug, Clone)]
pub struct PendingPrimitive {
    pub id: u64,
    pub handle: Handle,
    pub bounds: LayoutRect,
    pub radius: f32,
    pub animation: AnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub animated_bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub mouse_position: Option<Point>,
    pub progress: Option<f32>,
    pub progress_color: Color,
}

/// globals uniform buffer layout shared with the WGSL shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16],
    scale_factor: f32,
    _padding: [f32; 7],
}

/// Handles instanced draws for rounded images.
pub struct RoundedImageBatchState {
    pending_primitives: Vec<PendingPrimitive>,
    pending_instances: Vec<RoundedImageInstance>,
    instance_manager: InstanceBufferManager<RoundedImageInstance>,
    render_pipeline: Option<Arc<wgpu::RenderPipeline>>,
    shader: Arc<wgpu::ShaderModule>,
    atlas_bind_group_layout: Option<Arc<wgpu::BindGroupLayout>>,
    surface_format: wgpu::TextureFormat,
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
    uploads_this_frame: u32,
    loaded_times: HashMap<u64, Instant>,
}

impl RoundedImageBatchState {
    /// Vertex layout describing the 9 vec4 instance attributes.
    fn vertex_buffer_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 9] = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 48,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 64,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 80,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 96,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 112,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 128,
                shader_location: 8,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RoundedImageInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRS,
        }
    }

    /// Lazily creates the render pipeline once the atlas layout is known.
    fn ensure_pipeline(&mut self, device: &wgpu::Device, atlas_layout: Arc<wgpu::BindGroupLayout>) {
        if let Some(existing) = &self.atlas_bind_group_layout {
            if Arc::ptr_eq(existing, &atlas_layout) && self.render_pipeline.is_some() {
                return;
            }

            if !Arc::ptr_eq(existing, &atlas_layout) {
                log::warn!(
                    "RoundedImageBatchState received a different atlas layout; rebuilding pipeline",
                );
            }
        }

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rounded Image Pipeline Layout (Batched)"),
            bind_group_layouts: &[&self.globals_bind_group_layout, atlas_layout.as_ref()],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rounded Image Pipeline (Batched)"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[Self::vertex_buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.surface_format,
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

        self.render_pipeline = Some(Arc::new(pipeline));
        self.atlas_bind_group_layout = Some(atlas_layout);
    }

    /// Adds a primitive captured during encoding.
    pub fn enqueue(&mut self, pending: PendingPrimitive) {
        if let Some(existing) = self
            .pending_primitives
            .iter_mut()
            .find(|candidate| candidate.id == pending.id)
        {
            *existing = pending;
        } else {
            self.pending_primitives.push(pending);
        }
    }

    fn push_placeholder(&mut self, pending: &PendingPrimitive) {
        let instance = create_placeholder_instance(
            &pending.bounds,
            pending.radius,
            pending.theme_color,
            pending.animated_bounds.as_ref(),
            pending.progress,
            pending.progress_color,
        );

        self.pending_instances.push(instance);
    }
}

impl std::fmt::Debug for RoundedImageBatchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoundedImageBatchState")
            .field(
                "rendered_instances",
                &self.instance_manager.instance_count(),
            )
            .field("pending_primitives", &self.pending_primitives.len())
            .finish()
    }
}

impl PrimitiveBatchState for RoundedImageBatchState {
    type InstanceData = RoundedImageInstance;

    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self
    where
        Self: Sized,
    {
        let shader = Arc::new(device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rounded Image Shader (Batched)"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/rounded_image.wgsl").into(),
            ),
        }));

        let globals_bind_group_layout = Arc::new(
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Globals Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(
                                std::num::NonZeroU64::new(std::mem::size_of::<Globals>() as u64)
                                    .expect("globals size > 0"),
                            ),
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
            }),
        );

        let sampler = Arc::new(device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Rounded Image Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        }));

        Self {
            pending_primitives: Vec::new(),
            pending_instances: Vec::new(),
            instance_manager: InstanceBufferManager::new(),
            render_pipeline: None,
            shader,
            atlas_bind_group_layout: None,
            surface_format: format,
            globals_buffer: None,
            globals_bind_group: None,
            globals_bind_group_layout,
            sampler,
            uploads_this_frame: 0,
            loaded_times: HashMap::new(),
        }
    }

    fn add_instance(&mut self, instance: Self::InstanceData) {
        self.pending_instances.push(instance);
    }

    fn prepare(&mut self, context: &mut PrepareContext<'_>) {
        if let Some(mut image_cache) = context.resources.image_cache() {
            // Mutable access is required so cached lookups register cache hits
            // and keep atlas allocations alive across the renderer's trim pass.
            let atlas_layout = image_cache.texture_layout();
            self.ensure_pipeline(context.device, atlas_layout);

            for mut pending in std::mem::take(&mut self.pending_primitives) {
                let mut atlas_region = image_cache.cached_raster_region(&pending.handle);
                let was_cached = atlas_region.is_some();

                if !was_cached {
                    if self.uploads_this_frame >= MAX_UPLOADS_PER_FRAME {
                        self.push_placeholder(&pending);
                        continue;
                    }

                    self.uploads_this_frame += 1;
                    atlas_region = image_cache.ensure_raster_region(
                        context.device,
                        context.encoder,
                        &pending.handle,
                    );
                }

                let Some(region) = atlas_region else {
                    self.push_placeholder(&pending);
                    continue;
                };

                let mut load_time_ref: Option<Instant> = pending.load_time;

                if pending.animation != AnimationType::None {
                    let entry = self.loaded_times.entry(pending.id);

                    load_time_ref = match (pending.load_time, entry) {
                        (Some(explicit), _) => Some(explicit),
                        (None, std::collections::hash_map::Entry::Occupied(occupied)) => {
                            Some(*occupied.get())
                        }
                        (None, std::collections::hash_map::Entry::Vacant(vacant)) => {
                            let instant = Instant::now();
                            vacant.insert(instant);
                            Some(instant)
                        }
                    };
                }

                let instance = create_batch_instance(
                    Some(region),
                    &pending.bounds,
                    pending.radius,
                    pending.animation,
                    load_time_ref.as_ref(),
                    pending.opacity,
                    pending.theme_color,
                    pending.animated_bounds.as_ref(),
                    pending.is_hovered,
                    pending.mouse_position,
                    pending.progress,
                    pending.progress_color,
                );

                self.pending_instances.push(instance);
            }
        } else {
            if !self.pending_primitives.is_empty() {
                log::warn!(
                    "RoundedImageBatchState::prepare missing image cache; rendering placeholders for {} primitives",
                    self.pending_primitives.len()
                );
            }

            for pending in std::mem::take(&mut self.pending_primitives) {
                self.push_placeholder(&pending);
            }
        }

        for instance in self.pending_instances.drain(..) {
            self.instance_manager.add_instance(instance);
        }

        let pending_before_upload = self.instance_manager.pending_count();
        let upload_result =
            self.instance_manager
                .upload(context.device, context.encoder, context.belt);

        if upload_result.is_none() {
            if pending_before_upload > 0 {
                log::error!(
                    "RoundedImageBatchState failed to upload {} pending instances",
                    pending_before_upload
                );
            }

            return;
        }

        let globals = Globals {
            transform: context.viewport.projection().into(),
            scale_factor: context.scale_factor,
            _padding: [0.0; 7],
        };

        if self.globals_buffer.is_none() {
            self.globals_buffer = Some(context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Rounded Image Globals Buffer (Batched)"),
                size: std::mem::size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        if let Some(buffer) = &self.globals_buffer {
            context
                .belt
                .write_buffer(
                    context.encoder,
                    buffer,
                    0,
                    wgpu::BufferSize::new(std::mem::size_of::<Globals>() as u64).unwrap(),
                    context.device,
                )
                .copy_from_slice(bytemuck::bytes_of(&globals));

            if self.globals_bind_group.is_none() {
                self.globals_bind_group = Some(context.device.create_bind_group(
                    &wgpu::BindGroupDescriptor {
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
                    },
                ));
            }
        }
    }

    fn render(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &mut RenderContext<'_>,
        range: std::ops::Range<u32>,
    ) {
        let instance_count = self.instance_manager.instance_count() as u32;
        if instance_count == 0 {
            return;
        }

        let start = range.start.min(instance_count);
        let end = range.end.min(instance_count);
        if start >= end {
            return;
        }

        let Some(image_cache) = context.resources.image_cache() else {
            log::error!("RoundedImageBatchState::render missing image cache");
            return;
        };

        let (Some(instance_buffer), Some(globals_bind_group), Some(pipeline)) = (
            self.instance_manager.buffer(),
            self.globals_bind_group.as_ref(),
            self.render_pipeline.as_ref(),
        ) else {
            log::error!(
                "RoundedImageBatchState::render missing buffer/pipeline for {} instances",
                end - start
            );
            return;
        };

        let atlas_bind_group = image_cache.bind_group();

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);
        render_pass.set_bind_group(1, atlas_bind_group, &[]);

        let scissor = context.scissor_rect;
        render_pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        render_pass.set_vertex_buffer(0, instance_buffer.slice(..));
        render_pass.draw(0..4, start..end);
    }

    fn trim(&mut self) {
        let pending = self.pending_primitives.len();

        if pending > 0 {
            log::warn!(
                "RoundedImageBatchState::trim discarded {} pending primitives",
                pending
            );
        }

        self.instance_manager.clear();
        self.pending_instances.clear();
        self.pending_primitives.clear();
        self.uploads_this_frame = 0;
    }

    fn instance_count(&self) -> usize {
        let uploaded = self.instance_manager.instance_count();
        let staged = self.instance_manager.pending_count()
            + self.pending_instances.len()
            + self.pending_primitives.len();

        uploaded + staged
    }
}
