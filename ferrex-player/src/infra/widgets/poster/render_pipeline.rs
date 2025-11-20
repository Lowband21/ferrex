use crate::infra::{
    widgets::poster::batch_state::{self, PosterInstance},
    widgets::poster::{
        Instant,
        poster_animation_types::{
            self, PosterAnimationType, calculate_animation_state,
        },
    },
};
use bytemuck::{Pod, Zeroable};
use iced::{Color, Point, Rectangle, wgpu};
use iced_wgpu::AtlasRegion;
use std::collections::HashMap;
use std::sync::Arc;

const ATLAS_SIZE: f32 = 2048.0;

/// Global uniform data (viewport transform)
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct Globals {
    pub(crate) transform: [f32; 16], // 4x4 matrix = 64 bytes
    pub(crate) scale_factor: f32,    // 4 bytes
    pub(crate) atlas_is_srgb: f32,   // 4 bytes
    pub(crate) target_is_srgb: f32,  // 4 bytes
    pub(crate) _padding: [f32; 5], // Padding to make total 96 bytes (20 bytes padding)
}

/// Instance data for each poster
/// Packed into vec4s to reduce vertex attribute count
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct Instance {
    // vec4: position.xy, size.xy
    pub(crate) position_and_size: [f32; 4],
    // vec4: radius, opacity, rotation_y, animation_progress
    pub(crate) radius_opacity_rotation_anim: [f32; 4],
    // vec4: theme_color.rgb, z_depth
    pub(crate) theme_color_zdepth: [f32; 4],
    // vec4: scale, shadow_intensity, border_glow, animation_type
    pub(crate) scale_shadow_glow_type: [f32; 4],
    // vec4: is_hovered, show_overlay, show_border, progress
    pub(crate) hover_overlay_border_progress: [f32; 4],
    // vec4: mouse_position.xy, unused, unused
    pub(crate) mouse_pos_and_padding: [f32; 4],
    // vec4: progress_color.rgb, unused
    pub(crate) progress_color_and_padding: [f32; 4],
    // vec4: atlas_uv_min.xy, atlas_uv_max.xy
    pub(crate) atlas_uvs: [f32; 4],
    // i32: atlas texture layer index (flat)
    pub(crate) atlas_layer: i32,
    // padding to keep 16-byte alignment and conservative stride
    pub(crate) _pad_atlas_layer: [i32; 3],
}

/// Pipeline state (immutable after creation)
#[allow(dead_code)]
pub(crate) struct Pipeline {
    pub(crate) render_pipeline: wgpu::RenderPipeline,
    pub(crate) atlas_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    pub(crate) globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
    pub(crate) sampler: Arc<wgpu::Sampler>,
}

/// Per-primitive render data
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) struct PrimitiveData {
    pub(crate) instance_buffer: wgpu::Buffer,
}

/// Batched render data for all primitives in a frame
#[allow(dead_code)]
pub(crate) struct BatchedData {
    pub(crate) instance_buffer: Option<wgpu::Buffer>,
    pub(crate) instances: Vec<Instance>, // Accumulate instances across prepare calls
}

/// Shared state for all posters
///
/// Batching Strategy:
/// - Multiple prepare_batched calls accumulate instances
/// - Single render call draws all instances at once
/// - Instances are cleared after render for next frame
#[derive(Default)]
#[allow(dead_code)]
pub(crate) struct State {
    // Globals buffer and bind group (shared by all)
    pub(crate) globals_buffer: Option<wgpu::Buffer>,
    pub(crate) globals_bind_group: Option<wgpu::BindGroup>,
    // Per-primitive data for current frame
    pub(crate) primitive_data: HashMap<usize, PrimitiveData>,
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
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Self {
        log::debug!("Creating rounded image shader pipeline");

        // Load shader
        let shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Rounded Image Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../../shaders/poster.wgsl").into(),
                ),
            });

        // Create globals bind group layout (includes sampler)
        log::debug!("Creating globals bind group layout");
        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Rounded Image Globals"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX
                            | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: Some(
                                std::num::NonZeroU64::new(96).unwrap(),
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
                        sample_type: wgpu::TextureSampleType::Float {
                            filterable: true,
                        },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                }],
            });

        // Create pipeline layout
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Rounded Image Pipeline Layout"),
                bind_group_layouts: &[
                    &globals_bind_group_layout,
                    &atlas_bind_group_layout,
                ],
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
                // atlas_layer: i32
                wgpu::VertexAttribute {
                    offset: 128,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Sint32,
                },
            ],
        };

        // Create render pipeline
        let render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
pub(crate) fn create_batch_instance(
    atlas_region: Option<AtlasRegion>,
    bounds: &Rectangle,
    radius: f32,
    animation: PosterAnimationType,
    load_time: Option<&Instant>,
    opacity: f32,
    theme_color: Color,
    animated_bounds: Option<&poster_animation_types::AnimatedPosterBounds>,
    is_hovered: bool,
    mouse_position: Option<Point>,
    progress: Option<f32>,
    progress_color: Color,
) -> batch_state::PosterInstance {
    // Extract UV coordinates and layer from the atlas entry
    let (mut uv_min, mut uv_max, layer) = if let Some(region) = atlas_region {
        (region.uv_min, region.uv_max, region.layer)
    } else {
        // Use out-of-range UVs to signal placeholder/invalid to the shader.
        ([-1.0, -1.0], [-1.0, -1.0], 0)
    };

    // Apply a half-texel inset to avoid sampling neighboring atlas allocations when
    // using linear filtering.
    if uv_min[0] >= 0.0
        && uv_min[1] >= 0.0
        && uv_max[0] <= 1.0
        && uv_max[1] <= 1.0
    {
        let half_texel = 0.5 / ATLAS_SIZE;
        uv_min[0] += half_texel;
        uv_min[1] += half_texel;
        uv_max[0] -= half_texel;
        uv_max[1] -= half_texel;
    }

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
            PosterAnimationType::Flip {
                total_duration,
                emerge_end,
                flip_end,
                rise_end,
            } => {
                if elapsed > total_duration {
                    PosterAnimationType::None
                } else {
                    PosterAnimationType::Flip {
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
    let (poster_position, poster_size) =
        if let Some(animated_bounds) = animated_bounds {
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
        PosterAnimationType::None => true,
        PosterAnimationType::PlaceholderSunken => true,
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

        if (-0.01..=1.01).contains(&norm_x) && (-0.01..=1.01).contains(&norm_y)
        {
            [norm_x.clamp(0.0, 1.0), norm_y.clamp(0.0, 1.0)]
        } else {
            [-1.0, -1.0]
        }
    } else {
        [-1.0, -1.0]
    };

    // Create instance data
    // Convert colors to linear once to avoid redundant conversions
    let [theme_r, theme_g, theme_b, _] = theme_color.into_linear();
    let [prog_r, prog_g, prog_b, _] = progress_color.into_linear();

    PosterInstance {
        position_and_size: [
            poster_position[0],
            poster_position[1],
            poster_size[0],
            poster_size[1],
        ],
        radius_opacity_rotation_anim: [
            radius,
            actual_opacity,
            rotation_y,
            animation_progress,
        ],
        theme_color_zdepth: [theme_r, theme_g, theme_b, z_depth],
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
        mouse_pos_and_padding: [
            mouse_pos_normalized[0],
            mouse_pos_normalized[1],
            0.0,
            0.0,
        ],
        progress_color_and_padding: [prog_r, prog_g, prog_b, 0.0],
        atlas_uvs: [uv_min[0], uv_min[1], uv_max[0], uv_max[1]],
        atlas_layer: layer as i32,
        _pad_atlas_layer: [0, 0, 0],
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
pub fn create_placeholder_instance(
    bounds: &Rectangle,
    radius: f32,
    theme_color: Color,
    animated_bounds: Option<&poster_animation_types::AnimatedPosterBounds>,
    progress: Option<f32>,
    progress_color: Color,
) -> batch_state::PosterInstance {
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
    // Convert colors to linear once to avoid redundant conversions
    let [theme_r, theme_g, theme_b, _] = theme_color.into_linear();
    let [prog_r, prog_g, prog_b, _] = progress_color.into_linear();

    PosterInstance {
        position_and_size: [
            poster_position[0],
            poster_position[1],
            poster_size[0],
            poster_size[1],
        ],
        radius_opacity_rotation_anim: [
            radius,
            actual_opacity,
            rotation_y,
            animation_progress,
        ],
        theme_color_zdepth: [theme_r, theme_g, theme_b, z_depth],
        scale_shadow_glow_type: [scale, shadow_intensity, border_glow, 0.0],
        hover_overlay_border_progress: [
            0.0,
            show_overlay,
            show_border,
            progress.unwrap_or(-1.0),
        ],
        mouse_pos_and_padding: [0.0, 0.0, 0.0, 0.0],
        progress_color_and_padding: [prog_r, prog_g, prog_b, 0.0],
        atlas_uvs: [-1.0, -1.0, -1.0, -1.0],
        atlas_layer: 0,
        _pad_atlas_layer: [0, 0, 0],
    }
}
