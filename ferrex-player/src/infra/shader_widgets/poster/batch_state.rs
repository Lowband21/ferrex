//! Batch state implementation for `PosterPrimitive`.
//!
//! This module owns the GPU resources required to render all poster primitives
//! primitives in a single instanced draw call. Instances are accumulated during
//! the widget `encode_batch` phase and lazily uploaded during `prepare` once the
//! frame budget and texture cache state are known.

use crate::infra::{
    render::upload_budget::{TimingBasedBudget, UploadBudgetConfig},
    shader_widgets::poster::{
        PosterFace,
        animation::{AnimatedPosterBounds, PosterAnimationType},
        font_atlas::FontAtlas,
        render_pipeline::{create_batch_instance, create_placeholder_instance},
    },
};

use iced::{Color, Point, Rectangle as LayoutRect, widget::image::Handle};
use iced_wgpu::{
    primitive::{
        PrepareContext, PrimitiveBatchState, RenderContext,
        buffer_manager::InstanceBufferManager,
    },
    wgpu,
};

use wgpu::util::DeviceExt;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Instant,
};

use bytemuck::{Pod, Zeroable};

// Thread-safe storage for text scale, bridging UI state to GPU renderer.
// Default is 1.0f32 (0x3F800000 in IEEE 754).
static TEXT_SCALE_BITS: AtomicU32 = AtomicU32::new(0x3F800000);

/// Set the text scale factor for poster title/meta rendering.
/// Called from settings when the user changes their scale preference.
pub fn set_text_scale(scale: f32) {
    TEXT_SCALE_BITS.store(scale.to_bits(), Ordering::Relaxed);
}

/// Get the current text scale factor.
fn get_text_scale() -> f32 {
    f32::from_bits(TEXT_SCALE_BITS.load(Ordering::Relaxed))
}

/// Default duration of hover scale transition in milliseconds (fallback)
const DEFAULT_HOVER_TRANSITION_MS: u64 =
    crate::infra::constants::layout::animation::HOVER_TRANSITION_MS;

/// Default delay before scaling down after hover ends in milliseconds (fallback)
const DEFAULT_HOVER_SCALE_DOWN_DELAY_MS: u64 =
    crate::infra::constants::layout::animation::HOVER_SCALE_DOWN_DELAY_MS;

/// Tracks hover scale transition state for smooth animations
#[derive(Debug, Clone, Copy)]
struct HoverTransition {
    /// When the transition started
    start_time: Instant,
    /// Progress at transition start (0.0 = unhovered, 1.0 = hovered)
    start_progress: f32,
    /// Target progress at transition end (0.0 = unhovered, 1.0 = hovered)
    end_progress: f32,
    /// Transition duration in milliseconds (from AnimatedPosterBounds)
    transition_ms: u64,
    /// Delay before scaling down after hover ends (milliseconds)
    scale_down_delay_ms: u64,
}

impl HoverTransition {
    /// Calculate current hover progress (0.0 = not hovered, 1.0 = fully hovered)
    fn progress(&self) -> f32 {
        let elapsed_ms_u128 = self.start_time.elapsed().as_millis();

        if (self.start_progress - self.end_progress).abs() <= f32::EPSILON {
            return self.end_progress;
        }

        let scaling_down = self.end_progress < self.start_progress;

        // Hover-out: optionally hold before starting the scale-down.
        if scaling_down && elapsed_ms_u128 < self.scale_down_delay_ms as u128 {
            return self.start_progress;
        }

        if self.transition_ms == 0 {
            return self.end_progress;
        }

        let elapsed_for_transition_ms = if scaling_down {
            elapsed_ms_u128.saturating_sub(self.scale_down_delay_ms as u128)
                as f32
        } else {
            elapsed_ms_u128 as f32
        };

        let duration_ms = self.transition_ms as f32;
        let raw_t = (elapsed_for_transition_ms / duration_ms).clamp(0.0, 1.0);

        // Apply easing: ease-out for hover-in, ease-in for hover-out.
        let eased_t = if scaling_down {
            // ease-in-cubic: t^3
            raw_t * raw_t * raw_t
        } else {
            // ease-out-cubic: 1 - (1-t)^3
            let t = 1.0 - raw_t;
            1.0 - t * t * t
        };

        self.start_progress
            + (self.end_progress - self.start_progress) * eased_t
    }

    /// Check if transition is complete
    fn is_complete(&self) -> bool {
        let scaling_down = self.end_progress < self.start_progress;
        let total_ms = if scaling_down {
            self.transition_ms.saturating_add(self.scale_down_delay_ms)
        } else {
            self.transition_ms
        };

        self.start_time.elapsed().as_millis() >= total_ms as u128
    }

    // Note: we intentionally only track `progress` here. The actual scale
    // calculation (including hover_scale) happens when building GPU instances.
}

/// GPU instance payload for a poster primitive.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct PosterInstance {
    pub position_and_size: [f32; 4],
    pub radius_opacity_rotation_anim: [f32; 4],
    pub theme_color_zdepth: [f32; 4],
    pub scale_shadow_glow_type: [f32; 4],
    pub hover_overlay_border_progress: [f32; 4],
    pub mouse_pos_and_padding: [f32; 4],
    pub progress_color_and_padding: [f32; 4],
    pub atlas_uvs: [f32; 4],
    pub atlas_layer: i32,
    pub _pad_atlas_layer: [i32; 3],
    // Text data: packed character indices for title and meta
    // Each u32 contains 4 character indices (8 bits each)
    pub title_chars: [u32; 6], // 24 chars max
    pub meta_chars: [u32; 4],  // 16 chars max
    pub text_params: [f32; 4], // [title_len, meta_len, reserved, reserved]
}

/// Batched primitive metadata captured during encoding.
#[derive(Debug, Clone)]
pub struct PendingPrimitive {
    pub id: u64,
    pub handle: Handle,
    pub bounds: LayoutRect,
    pub radius: f32,
    pub animation: PosterAnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub animated_bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub mouse_position: Option<Point>,
    pub progress: Option<f32>,
    pub progress_color: Color,
    pub rotation_override: Option<f32>,
    pub face: PosterFace,
    /// Title text to render below the poster
    pub title: Option<String>,
    /// Meta text (e.g., year) to render below the title
    pub meta: Option<String>,
}

/// globals uniform buffer layout shared with the WGSL shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    transform: [f32; 16],
    scale_factor: f32,
    // Whether atlas samples are already linear (sRGB texture implies GPU decode)
    atlas_is_srgb: f32,
    // Whether the render target is sRGB (GPU will encode on write)
    target_is_srgb: f32,
    // UI text scale for poster title/meta (from unified scaling infra)
    text_scale: f32,
    _padding: [f32; 4],
}

/// Handles instanced draws for batched posters.
pub struct PosterBatchState {
    pending_primitives: Vec<PendingPrimitive>,
    pending_instances_front: Vec<PosterInstance>,
    pending_instances_back: Vec<PosterInstance>,
    instance_manager_front: InstanceBufferManager<PosterInstance>,
    instance_manager_back: InstanceBufferManager<PosterInstance>,
    render_pipeline_front: Option<Arc<wgpu::RenderPipeline>>,
    render_pipeline_back: Option<Arc<wgpu::RenderPipeline>>,
    shader: Arc<wgpu::ShaderModule>,
    shader_back: Arc<wgpu::ShaderModule>,
    atlas_bind_group_layout: Option<Arc<wgpu::BindGroupLayout>>,
    surface_format: wgpu::TextureFormat,
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    sampler: Arc<wgpu::Sampler>,
    upload_budget: TimingBasedBudget,
    loaded_times: HashMap<(u64, PosterFace), Instant>,
    // Avoid log flooding: remember last layer we logged per instance id
    logged_layers: HashMap<u64, i32>,
    // Track hover state transitions for smooth scale animations
    hover_transitions: HashMap<u64, HoverTransition>,
    // Track previous hover state to detect changes
    previous_hover_states: HashMap<u64, bool>,
    groups_front: Vec<PosterGroup>,
    groups_back: Vec<PosterGroup>,
    // Font atlas for SDF text rendering
    font_atlas: Option<FontAtlas>,
    font_atlas_texture: Option<wgpu::Texture>,
    font_atlas_view: Option<wgpu::TextureView>,
    font_atlas_uploaded: bool,
    // Text rendering provider
    shader_text: Arc<wgpu::ShaderModule>,
    render_pipeline_text: Option<Arc<wgpu::RenderPipeline>>,
}

impl PosterBatchState {
    /// Vertex layout describing instance attributes including text data.
    fn vertex_buffer_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        const ATTRS: [wgpu::VertexAttribute; 13] = [
            // Original 9 attributes
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
                format: wgpu::VertexFormat::Sint32,
            },
            // Text data attributes (locations 9-12)
            wgpu::VertexAttribute {
                offset: 144,
                shader_location: 9,
                format: wgpu::VertexFormat::Uint32x4, // title_chars[0..4]
            },
            wgpu::VertexAttribute {
                offset: 160,
                shader_location: 10,
                format: wgpu::VertexFormat::Uint32x2, // title_chars[4..6]
            },
            wgpu::VertexAttribute {
                offset: 168,
                shader_location: 11,
                format: wgpu::VertexFormat::Uint32x4, // meta_chars
            },
            wgpu::VertexAttribute {
                offset: 184,
                shader_location: 12,
                format: wgpu::VertexFormat::Float32x4, // text_params
            },
        ];

        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<PosterInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRS,
        }
    }

    /// Lazily creates the front render provider once the atlas layout is known.
    fn ensure_pipeline_front(
        &mut self,
        device: &wgpu::Device,
        atlas_layout: Arc<wgpu::BindGroupLayout>,
    ) {
        if let Some(existing) = &self.atlas_bind_group_layout {
            if Arc::ptr_eq(existing, &atlas_layout)
                && self.render_pipeline_front.is_some()
            {
                return;
            }

            if !Arc::ptr_eq(existing, &atlas_layout) {
                log::warn!(
                    "PosterBatchState received a different atlas layout; rebuilding provider",
                );
            }
        }

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Poster Pipeline Layout (Batched)"),
                bind_group_layouts: &[
                    &self.globals_bind_group_layout,
                    atlas_layout.as_ref(),
                ],
                push_constant_ranges: &[],
            });

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Poster Front Pipeline (Batched)"),
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

        self.render_pipeline_front = Some(Arc::new(pipeline));
        self.atlas_bind_group_layout = Some(atlas_layout);
    }

    /// Lazily creates the back render provider once the atlas layout is known.
    fn ensure_pipeline_back(
        &mut self,
        device: &wgpu::Device,
        atlas_layout: Arc<wgpu::BindGroupLayout>,
    ) {
        if let Some(existing) = &self.atlas_bind_group_layout
            && Arc::ptr_eq(existing, &atlas_layout)
            && self.render_pipeline_back.is_some()
        {
            return;
        }

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Poster Back Pipeline Layout (Batched)"),
                bind_group_layouts: &[
                    &self.globals_bind_group_layout,
                    atlas_layout.as_ref(),
                ],
                push_constant_ranges: &[],
            });

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Poster Back Pipeline (Batched)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader_back,
                    entry_point: Some("vs_main_back"),
                    buffers: &[Self::vertex_buffer_layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader_back,
                    entry_point: Some("fs_main_back"),
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

        self.render_pipeline_back = Some(Arc::new(pipeline));
        self.atlas_bind_group_layout = Some(atlas_layout);
    }

    /// Lazily creates the text render provider for title/meta rendering.
    /// Text uses only the globals bind group (which includes the font atlas).
    fn ensure_pipeline_text(&mut self, device: &wgpu::Device) {
        if self.render_pipeline_text.is_some() {
            return;
        }

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Poster Text Pipeline Layout"),
                bind_group_layouts: &[&self.globals_bind_group_layout],
                push_constant_ranges: &[],
            });

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Poster Text Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader_text,
                    entry_point: Some("vs_text"),
                    buffers: &[Self::vertex_buffer_layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader_text,
                    entry_point: Some("fs_text"),
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

        self.render_pipeline_text = Some(Arc::new(pipeline));
    }

    /// Adds a primitive captured during encoding.
    pub fn enqueue(&mut self, pending: PendingPrimitive) {
        const POSITION_EPSILON: f32 = 0.5;

        // Deduplicate by (id, position, face) to allow same content at different screen locations
        // while preventing accidental double-renders of the exact same widget.
        // Face is included to prevent front/back primitives from replacing each other during animation.
        if let Some(existing) =
            self.pending_primitives.iter_mut().find(|candidate| {
                candidate.id == pending.id
                    && (candidate.bounds.x - pending.bounds.x).abs()
                        < POSITION_EPSILON
                    && (candidate.bounds.y - pending.bounds.y).abs()
                        < POSITION_EPSILON
                    && candidate.face == pending.face
            })
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
            pending.face,
        );

        match pending.face {
            PosterFace::Front => self.pending_instances_front.push(instance),
            PosterFace::Back => self.pending_instances_back.push(instance),
        }
    }

    fn push_group_instance(
        &mut self,
        face: PosterFace,
        group: Arc<wgpu::BindGroup>,
    ) {
        let groups = match face {
            PosterFace::Front => &mut self.groups_front,
            PosterFace::Back => &mut self.groups_back,
        };
        match groups.last_mut() {
            Some(last) if Arc::ptr_eq(&last.atlas, &group) => {
                last.instance_count += 1;
            }
            _ => groups.push(PosterGroup {
                atlas: group,
                instance_count: 1,
            }),
        }
    }

    /// Compute smooth hover progress for a poster instance.
    /// Tracks hover state changes and interpolates scale over time.
    /// Returns progress from 0.0 (not hovered) to 1.0 (fully hovered).
    fn compute_hover_progress(
        &mut self,
        id: u64,
        is_hovered: bool,
        hover_transition_ms: u64,
        hover_scale_down_delay_ms: u64,
    ) -> f32 {
        let prev_hovered = match self.previous_hover_states.get(&id).copied() {
            Some(prev) => prev,
            None => {
                self.previous_hover_states.insert(id, is_hovered);
                self.hover_transitions.remove(&id);
                return if is_hovered { 1.0 } else { 0.0 };
            }
        };

        // Detect hover state change
        if prev_hovered != is_hovered {
            let current_progress = self
                .hover_transitions
                .get(&id)
                .map(HoverTransition::progress)
                .unwrap_or_else(|| if prev_hovered { 1.0 } else { 0.0 });

            let end_progress = if is_hovered { 1.0 } else { 0.0 };

            if (current_progress - end_progress).abs() <= f32::EPSILON {
                self.hover_transitions.remove(&id);
            } else {
                let transition = HoverTransition {
                    start_time: Instant::now(),
                    start_progress: current_progress,
                    end_progress,
                    transition_ms: hover_transition_ms,
                    scale_down_delay_ms: hover_scale_down_delay_ms,
                };
                self.hover_transitions.insert(id, transition);
            }

            self.previous_hover_states.insert(id, is_hovered);
        }

        // Get current progress from transition (or default based on hover state)
        if let Some(transition) = self.hover_transitions.get(&id).copied() {
            let progress = transition.progress();
            if transition.is_complete() {
                self.hover_transitions.remove(&id);
            }
            progress
        } else if is_hovered {
            1.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
struct PosterGroup {
    atlas: Arc<wgpu::BindGroup>,
    instance_count: u32,
}

impl std::fmt::Debug for PosterBatchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PosterBatchState")
            .field(
                "rendered_instances_front",
                &self.instance_manager_front.instance_count(),
            )
            .field(
                "rendered_instances_back",
                &self.instance_manager_back.instance_count(),
            )
            .field("pending_primitives", &self.pending_primitives.len())
            .finish()
    }
}

impl PrimitiveBatchState for PosterBatchState {
    type InstanceData = PosterInstance;

    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self
    where
        Self: Sized,
    {
        // Combine common helpers with the poster shader to allow shared utilities.
        let shader_src = format!(
            "{}\n{}",
            include_str!("../../shaders/common.wgsl"),
            include_str!("../../shaders/poster.wgsl")
        );

        let shader = Arc::new(device.create_shader_module(
            wgpu::ShaderModuleDescriptor {
                label: Some("Poster Shader (Batched)"),
                source: wgpu::ShaderSource::Wgsl(shader_src.into()),
            },
        ));

        let shader_src_back = format!(
            "{}\n{}",
            include_str!("../../shaders/common.wgsl"),
            include_str!("../../shaders/poster_back.wgsl")
        );

        let shader_back = Arc::new(device.create_shader_module(
            wgpu::ShaderModuleDescriptor {
                label: Some("Poster Back Shader (Batched)"),
                source: wgpu::ShaderSource::Wgsl(shader_src_back.into()),
            },
        ));

        // Text shader for title/meta rendering (no 3D rotation)
        let shader_text = Arc::new(device.create_shader_module(
            wgpu::ShaderModuleDescriptor {
                label: Some("Poster Text Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../../shaders/poster_text.wgsl").into(),
                ),
            },
        ));

        let globals_bind_group_layout = Arc::new(
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Poster Globals Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX
                            | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(
                                std::num::NonZeroU64::new(std::mem::size_of::<
                                    Globals,
                                >(
                                )
                                    as u64)
                                .expect("globals size > 0"),
                            ),
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
                    // Font atlas SDF texture for text rendering
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
                ],
            }),
        );

        let sampler =
            Arc::new(device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Rounded Image Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..wgpu::SamplerDescriptor::default()
            }));

        // Generate font atlas for SDF text rendering
        let (font_atlas, font_atlas_texture, font_atlas_view) =
            match FontAtlas::generate() {
                Ok(atlas) => {
                    let texture =
                        device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("Font Atlas SDF Texture"),
                            size: wgpu::Extent3d {
                                width: atlas.width,
                                height: atlas.height,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });

                    // Texture data will be uploaded in prepare() where we have queue access
                    let view =
                        texture.create_view(&wgpu::TextureViewDescriptor {
                            label: Some("Font Atlas SDF View"),
                            ..Default::default()
                        });

                    log::info!(
                        "Font atlas generated: {}x{} with {} glyphs",
                        atlas.width,
                        atlas.height,
                        atlas.glyphs.len()
                    );

                    (Some(atlas), Some(texture), Some(view))
                }
                Err(e) => {
                    log::error!("Failed to generate font atlas: {}", e);
                    (None, None, None)
                }
            };

        Self {
            pending_primitives: Vec::new(),
            pending_instances_front: Vec::new(),
            pending_instances_back: Vec::new(),
            instance_manager_front: InstanceBufferManager::new(),
            instance_manager_back: InstanceBufferManager::new(),
            render_pipeline_front: None,
            render_pipeline_back: None,
            shader: shader.clone(),
            shader_back,
            atlas_bind_group_layout: None,
            surface_format: format,
            globals_buffer: None,
            globals_bind_group: None,
            globals_bind_group_layout,
            sampler,
            upload_budget: TimingBasedBudget::new(UploadBudgetConfig::for_hz(
                120,
            )),
            loaded_times: HashMap::new(),
            logged_layers: HashMap::new(),
            hover_transitions: HashMap::new(),
            previous_hover_states: HashMap::new(),
            groups_front: Vec::new(),
            groups_back: Vec::new(),
            font_atlas,
            font_atlas_texture,
            font_atlas_view,
            font_atlas_uploaded: false,
            shader_text,
            render_pipeline_text: None,
        }
    }

    fn add_instance(&mut self, instance: Self::InstanceData) {
        self.pending_instances_front.push(instance);
    }

    fn prepare(&mut self, context: &mut PrepareContext<'_>) {
        self.groups_front.clear();
        self.groups_back.clear();
        self.upload_budget.begin_frame();

        if let Some(image_cache) = context.resources.image_cache() {
            // Mutable access is required so cached lookups register cache hits
            // and keep atlas allocations alive across the renderer's trim pass.
            let atlas_layout = image_cache.texture_layout();
            self.ensure_pipeline_front(context.device, atlas_layout.clone());
            self.ensure_pipeline_back(context.device, atlas_layout.clone());
            self.ensure_pipeline_text(context.device);

            // Keep track of the last atlas bind group we saw this frame so we can
            // reuse it when we draw placeholders for images that missed the budget.
            let mut last_group: Option<Arc<wgpu::BindGroup>> = None;

            for pending in std::mem::take(&mut self.pending_primitives) {
                // Bind group for the texture atlas containing this image
                let mut bind_group: Option<Arc<wgpu::BindGroup>> = None;

                let mut atlas_region =
                    image_cache.cached_raster_region(&pending.handle);
                let was_cached = atlas_region.is_some();

                if !was_cached {
                    // Diagnostic: log computed row/padded stride for non-256-aligned widths
                    if log::log_enabled!(log::Level::Debug)
                        && let Some(dims) =
                            image_cache.measure_image(&pending.handle)
                    {
                        let width = dims.width;
                        let height = dims.height;
                        if width > 0 && height > 0 {
                            let row_bytes = width as usize * 4;
                            let align =
                                wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
                            let padded = if row_bytes == 0 {
                                0
                            } else {
                                row_bytes.div_ceil(align) * align
                            };
                            if !row_bytes.is_multiple_of(align) {
                                log::trace!(
                                    "Poster atlas upload: {}x{} RGBA, row_bytes={} padded_bytes_per_row={} (align {}), extent=({}, {}, 1)",
                                    width,
                                    height,
                                    row_bytes,
                                    padded,
                                    align,
                                    width,
                                    height
                                );
                            }
                        }
                    }
                    if !self.upload_budget.can_upload() {
                        // Over budget: draw a placeholder instance and, if we
                        // have seen any atlas bind group already, use it.
                        let instance = create_placeholder_instance(
                            &pending.bounds,
                            pending.radius,
                            pending.theme_color,
                            pending.animated_bounds.as_ref(),
                            pending.progress,
                            pending.progress_color,
                            pending.face,
                        );
                        match pending.face {
                            PosterFace::Front => {
                                self.pending_instances_front.push(instance)
                            }
                            PosterFace::Back => {
                                self.pending_instances_back.push(instance)
                            }
                        }

                        let group_arc = match &last_group {
                            Some(group) => Arc::clone(group),
                            None => {
                                let fallback =
                                    Arc::clone(image_cache.bind_group());
                                last_group = Some(fallback.clone());
                                fallback
                            }
                        };

                        self.push_group_instance(pending.face, group_arc);

                        continue;
                    }

                    // Timed upload with budget tracking
                    let upload_start = Instant::now();
                    let upload_result = image_cache.upload_raster(
                        context.device,
                        context.encoder,
                        context.belt,
                        &pending.handle,
                    );
                    self.upload_budget.record_upload(upload_start.elapsed());

                    if let Some((_entry, group)) = upload_result {
                        bind_group = Some(group.clone());
                        last_group = Some(group.clone());
                    }
                    // Re-check the cached region after upload
                    atlas_region =
                        image_cache.cached_raster_region(&pending.handle);
                }

                let Some(region) = atlas_region else {
                    // Still no region: draw a placeholder; use last_group if present
                    let instance = create_placeholder_instance(
                        &pending.bounds,
                        pending.radius,
                        pending.theme_color,
                        pending.animated_bounds.as_ref(),
                        pending.progress,
                        pending.progress_color,
                        pending.face,
                    );
                    match pending.face {
                        PosterFace::Front => {
                            self.pending_instances_front.push(instance)
                        }
                        PosterFace::Back => {
                            self.pending_instances_back.push(instance)
                        }
                    }

                    let group_arc = match &last_group {
                        Some(group) => Arc::clone(group),
                        None => {
                            let fallback = Arc::clone(image_cache.bind_group());
                            last_group = Some(fallback.clone());
                            fallback
                        }
                    };

                    self.push_group_instance(pending.face, group_arc);
                    continue;
                };

                let mut load_time_ref: Option<Instant> = pending.load_time;

                // Only record a GPU-based start time for real textures/animations.
                // Skip placeholders so the flip starts when the actual atlas upload completes.
                if !matches!(
                    pending.animation,
                    PosterAnimationType::None
                        | PosterAnimationType::PlaceholderSunken
                ) {
                    let entry =
                        self.loaded_times.entry((pending.id, pending.face));

                    load_time_ref = match (pending.load_time, entry) {
                        (Some(explicit), _) => Some(explicit),
                        (
                            None,
                            std::collections::hash_map::Entry::Occupied(
                                occupied,
                            ),
                        ) => Some(*occupied.get()),
                        (
                            None,
                            std::collections::hash_map::Entry::Vacant(vacant),
                        ) => {
                            let instant = Instant::now();
                            vacant.insert(instant);
                            Some(instant)
                        }
                    };
                }

                // Log atlas layer once per id or on change to avoid flooding
                #[cfg(debug_assertions)]
                {
                    let layer_i32 = region.layer as i32;
                    let should_log = match self.logged_layers.get(&pending.id) {
                        None => true,
                        Some(prev) => *prev != layer_i32,
                    };
                    if should_log {
                        log::trace!(
                            "PosterBatch: id={} atlas_layer={} (cached={}, uploads_this_frame={}), uv_min=({:.6},{:.6}) uv_max=({:.6},{:.6})",
                            pending.id,
                            layer_i32,
                            was_cached,
                            self.upload_budget.uploads_this_frame(),
                            region.uv_min[0],
                            region.uv_min[1],
                            region.uv_max[0],
                            region.uv_max[1]
                        );
                        self.logged_layers.insert(pending.id, layer_i32);
                    }
                }

                // Compute smooth hover progress for scale animation
                let hover_transition_ms = pending
                    .animated_bounds
                    .as_ref()
                    .map(|b| b.hover_transition_ms)
                    .unwrap_or(DEFAULT_HOVER_TRANSITION_MS);
                let hover_scale_down_delay_ms = pending
                    .animated_bounds
                    .as_ref()
                    .map(|b| b.hover_scale_down_delay_ms)
                    .unwrap_or(DEFAULT_HOVER_SCALE_DOWN_DELAY_MS);

                let hover_progress = self.compute_hover_progress(
                    pending.id,
                    pending.is_hovered,
                    hover_transition_ms,
                    hover_scale_down_delay_ms,
                );

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
                    hover_progress,
                    pending.mouse_position,
                    pending.progress,
                    pending.progress_color,
                    pending.rotation_override,
                    pending.face,
                    pending.title.as_deref(),
                    pending.meta.as_deref(),
                );

                // Track groups by atlas bind group. If none obtained yet, try to
                // fallback to the main atlas bind group by triggering an upload_raster
                // call (which will provide it if available).
                if bind_group.is_none()
                    && let Some((_entry, group)) = image_cache.upload_raster(
                        context.device,
                        context.encoder,
                        context.belt,
                        &pending.handle,
                    )
                {
                    bind_group = Some(group.clone());
                    last_group = Some(group.clone());
                }

                // If we still do not have a bind group, associate this instance
                // with the last known group (placeholders use invalid UVs).
                let Some(group_arc) = bind_group.or_else(|| last_group.clone())
                else {
                    // No group available this frame (can happen on first frame before any textures load)
                    continue;
                };

                // Append instance and update grouping for render segmentation
                match pending.face {
                    PosterFace::Front => {
                        self.pending_instances_front.push(instance)
                    }
                    PosterFace::Back => {
                        self.pending_instances_back.push(instance)
                    }
                }

                last_group = Some(group_arc.clone());
                self.push_group_instance(pending.face, group_arc);
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

        // Validate group counts match instance counts (debug assertion)
        #[cfg(debug_assertions)]
        {
            let front_instance_count = self.pending_instances_front.len();
            let back_instance_count = self.pending_instances_back.len();
            let front_group_total: usize = self
                .groups_front
                .iter()
                .map(|g| g.instance_count as usize)
                .sum();
            let back_group_total: usize = self
                .groups_back
                .iter()
                .map(|g| g.instance_count as usize)
                .sum();

            debug_assert_eq!(
                front_instance_count, front_group_total,
                "Front instance count mismatch: {} instances vs {} in groups",
                front_instance_count, front_group_total
            );
            debug_assert_eq!(
                back_instance_count, back_group_total,
                "Back instance count mismatch: {} instances vs {} in groups",
                back_instance_count, back_group_total
            );
        }

        for instance in self.pending_instances_front.drain(..) {
            self.instance_manager_front.add_instance(instance);
        }
        for instance in self.pending_instances_back.drain(..) {
            self.instance_manager_back.add_instance(instance);
        }

        let pending_before_upload_front =
            self.instance_manager_front.pending_count();
        let upload_result_front = self.instance_manager_front.upload(
            context.device,
            context.encoder,
            context.belt,
        );

        let pending_before_upload_back =
            self.instance_manager_back.pending_count();
        let upload_result_back = self.instance_manager_back.upload(
            context.device,
            context.encoder,
            context.belt,
        );

        if upload_result_front.is_none() && pending_before_upload_front > 0 {
            log::error!(
                "PosterBatchState failed to upload {} pending front instances",
                pending_before_upload_front
            );
        }
        if upload_result_back.is_none() && pending_before_upload_back > 0 {
            log::error!(
                "PosterBatchState failed to upload {} pending back instances",
                pending_before_upload_back
            );
        }

        // Consider renderer present-time encode for non-sRGB surfaces
        let target_is_srgb =
            if wgpu::TextureFormat::is_srgb(&self.surface_format) {
                1.0
            } else if iced_wgpu::graphics::color::GAMMA_CORRECTION {
                // Renderer will encode to sRGB on present when gamma-correct composition is enabled
                1.0
            } else {
                0.0
            };

        // Atlas is sRGB under gamma-correct composition; keep in sync with presentation
        let atlas_is_srgb = target_is_srgb;

        let globals = Globals {
            transform: context.viewport.projection().into(),
            scale_factor: context.scale_factor,
            atlas_is_srgb,
            target_is_srgb,
            text_scale: get_text_scale(),
            _padding: [0.0; 4],
        };

        if self.globals_buffer.is_none() {
            self.globals_buffer =
                Some(context.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Rounded Image Globals Buffer (Batched)"),
                    size: std::mem::size_of::<Globals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }

        // Upload font atlas texture data if not yet uploaded
        if !self.font_atlas_uploaded {
            if let (Some(atlas), Some(texture)) =
                (&self.font_atlas, &self.font_atlas_texture)
            {
                // Calculate padded row size for wgpu alignment
                let bytes_per_row = atlas.width * 4;
                let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let padded_bytes_per_row =
                    bytes_per_row.div_ceil(align) * align;
                let total_size = (padded_bytes_per_row * atlas.height) as usize;

                // Create padded data
                let mut padded_data = vec![0u8; total_size];
                for y in 0..atlas.height as usize {
                    let src_start = y * (atlas.width * 4) as usize;
                    let src_end = src_start + (atlas.width * 4) as usize;
                    let dst_start = y * padded_bytes_per_row as usize;
                    let dst_end = dst_start + (atlas.width * 4) as usize;
                    padded_data[dst_start..dst_end].copy_from_slice(
                        &atlas.texture_data[src_start..src_end],
                    );
                }

                // Create staging buffer with mapped data
                let staging_buffer = context.device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("Font Atlas Staging Buffer"),
                        contents: &padded_data,
                        usage: wgpu::BufferUsages::COPY_SRC,
                    },
                );

                // Copy buffer to texture
                context.encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer: &staging_buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded_bytes_per_row),
                            rows_per_image: Some(atlas.height),
                        },
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: atlas.width,
                        height: atlas.height,
                        depth_or_array_layers: 1,
                    },
                );

                self.font_atlas_uploaded = true;
                log::info!(
                    "Font atlas texture uploaded: {}x{} ({} bytes)",
                    atlas.width,
                    atlas.height,
                    total_size
                );
            } else if self.font_atlas_texture.is_none() {
                // Create fallback 1x1 white texture if font atlas failed
                let fallback_data = [255u8, 255, 255, 255];
                let fallback_texture =
                    context.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Font Atlas Fallback Texture"),
                        size: wgpu::Extent3d {
                            width: 1,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });

                // Create staging buffer for fallback
                let staging_buffer = context.device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("Font Atlas Fallback Staging"),
                        contents: &fallback_data,
                        usage: wgpu::BufferUsages::COPY_SRC,
                    },
                );

                // Copy to fallback texture
                context.encoder.copy_buffer_to_texture(
                    wgpu::TexelCopyBufferInfo {
                        buffer: &staging_buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(4),
                            rows_per_image: Some(1),
                        },
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &fallback_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );

                let fallback_view = fallback_texture.create_view(
                    &wgpu::TextureViewDescriptor {
                        label: Some("Font Atlas Fallback View"),
                        ..Default::default()
                    },
                );

                self.font_atlas_texture = Some(fallback_texture);
                self.font_atlas_view = Some(fallback_view);
                self.font_atlas_uploaded = true;
                log::warn!(
                    "Using fallback font atlas texture (font atlas generation failed)"
                );
            }
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

            // Only create bind group when we have the font atlas texture view
            if self.globals_bind_group.is_none()
                && let Some(font_atlas_view) = &self.font_atlas_view
            {
                self.globals_bind_group =
                    Some(context.device.create_bind_group(
                        &wgpu::BindGroupDescriptor {
                            label: Some(
                                "Rounded Image Globals Bind Group (Batched)",
                            ),
                            layout: &self.globals_bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: buffer.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(
                                        &self.sampler,
                                    ),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource:
                                        wgpu::BindingResource::TextureView(
                                            font_atlas_view,
                                        ),
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
        _range: std::ops::Range<u32>,
    ) {
        // IMPORTANT: We ignore the range parameter and always render ALL instances.
        //
        // When a poster flips from front to back face, the front buffer indices shift
        // (what was at index 8 might now be at index 7). Iced's batching system splits
        // rendering into passes with different scissor rects based on instance indices.
        // If we used the range, flipped posters would cause adjacent carousels' first
        // posters to render in the wrong scissor region and get clipped.
        //
        // By rendering all instances every pass, each instance renders at its correct
        // position and the scissor rect naturally clips what's off-screen.
        let front_count = self.instance_manager_front.instance_count() as u32;
        let back_count = self.instance_manager_back.instance_count() as u32;

        // Use full range for both faces
        let full_front_range = 0..front_count;
        let full_back_range = 0..back_count;

        Self::render_face(
            &self.instance_manager_front,
            &self.groups_front,
            self.render_pipeline_front.as_ref(),
            self.globals_bind_group.as_ref(),
            render_pass,
            context,
            full_front_range,
        );

        Self::render_face(
            &self.instance_manager_back,
            &self.groups_back,
            self.render_pipeline_back.as_ref(),
            self.globals_bind_group.as_ref(),
            render_pass,
            context,
            full_back_range,
        );

        // Render text (title/meta) after poster faces - also use full range
        let total_count = front_count + back_count;
        self.render_text(render_pass, context, 0..total_count);
    }

    fn trim(&mut self) {
        let pending = self.pending_primitives.len();

        if pending > 0 {
            log::warn!(
                "RoundedImageBatchState::trim discarded {} pending primitives",
                pending
            );
        }

        self.instance_manager_front.clear();
        self.instance_manager_back.clear();
        self.pending_instances_front.clear();
        self.pending_instances_back.clear();
        self.pending_primitives.clear();
        self.upload_budget.end_frame();
        self.groups_front.clear();
        self.groups_back.clear();
    }

    fn instance_count(&self) -> usize {
        let uploaded = self.instance_manager_front.instance_count()
            + self.instance_manager_back.instance_count();
        let staged = self.instance_manager_front.pending_count()
            + self.instance_manager_back.pending_count()
            + self.pending_instances_front.len()
            + self.pending_instances_back.len()
            + self.pending_primitives.len();

        uploaded + staged
    }
}

impl PosterBatchState {
    fn render_face(
        manager: &InstanceBufferManager<PosterInstance>,
        groups: &[PosterGroup],
        pipeline: Option<&Arc<wgpu::RenderPipeline>>,
        globals: Option<&wgpu::BindGroup>,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &mut RenderContext<'_>,
        range: std::ops::Range<u32>,
    ) {
        let instance_count = manager.instance_count() as u32;
        if instance_count == 0 {
            return;
        }

        let start = range.start.min(instance_count);
        let end = range.end.min(instance_count);
        if start >= end {
            return;
        }

        let (Some(instance_buffer), Some(globals_bind_group), Some(pipeline)) =
            (manager.buffer(), globals, pipeline)
        else {
            log::error!(
                "PosterBatchState::render missing buffer/provider for {} instances",
                end - start
            );
            return;
        };

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);

        let scissor = context.scissor_rect;
        render_pass.set_scissor_rect(
            scissor.x,
            scissor.y,
            scissor.width,
            scissor.height,
        );
        render_pass.set_vertex_buffer(0, instance_buffer.slice(..));

        let mut offset: u32 = 0;
        for group in groups {
            let group_start = offset;
            let group_end = offset + group.instance_count;

            let draw_start = start.max(group_start);
            let draw_end = end.min(group_end);

            if draw_start < draw_end {
                render_pass.set_bind_group(1, group.atlas.as_ref(), &[]);
                render_pass.draw(0..4, draw_start..draw_end);
            }

            offset = group_end;
            if offset >= end {
                break;
            }
        }
    }

    /// Renders text (title/meta) for all poster instances (both front and back faces).
    /// Text stays fixed below the poster regardless of flip state.
    fn render_text(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &mut RenderContext<'_>,
        range: std::ops::Range<u32>,
    ) {
        let (Some(globals_bind_group), Some(pipeline)) = (
            self.globals_bind_group.as_ref(),
            self.render_pipeline_text.as_ref(),
        ) else {
            return;
        };

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, globals_bind_group, &[]);

        let scissor = context.scissor_rect;
        render_pass.set_scissor_rect(
            scissor.x,
            scissor.y,
            scissor.width,
            scissor.height,
        );

        // Render text for FRONT instances
        if let Some(buffer) = self.instance_manager_front.buffer() {
            let count = self.instance_manager_front.instance_count() as u32;
            if count > 0 {
                let start = range.start.min(count);
                let end = range.end.min(count);
                if start < end {
                    render_pass.set_vertex_buffer(0, buffer.slice(..));
                    render_pass.draw(0..4, start..end);
                }
            }
        }

        // Render text for BACK instances (text stays visible when poster is flipped)
        if let Some(buffer) = self.instance_manager_back.buffer() {
            let count = self.instance_manager_back.instance_count() as u32;
            if count > 0 {
                let start = range.start.min(count);
                let end = range.end.min(count);
                if start < end {
                    render_pass.set_vertex_buffer(0, buffer.slice(..));
                    render_pass.draw(0..4, start..end);
                }
            }
        }
    }
}
