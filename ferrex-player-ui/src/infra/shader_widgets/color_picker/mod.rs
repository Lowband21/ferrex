//! GPU-accelerated color picker shader widget
//!
//! A radial color picker with:
//! - 2D wheel: hue around circumference, saturation as radius
//! - Draggable color point handles
//! - Color harmony support (complementary, triadic, split-complementary)
//! - Smooth animations for hover and drag states

pub mod render_pipeline;
pub mod state;

// Responsive sizing constants

/// Proportion of available width the picker should fill (0.0-1.0)
pub const COLOR_PICKER_FILL_RATIO: f32 = 0.85;

/// Minimum wheel radius (ensures usability at low scales/narrow windows)
pub const COLOR_PICKER_MIN_RADIUS: f32 = 100.0;

/// Base handle radius proportion (relative to wheel radius)
pub const COLOR_PICKER_HANDLE_RATIO: f32 = 0.05;

use iced::{
    Element, Event, Length, Point, Rectangle,
    advanced::graphics::Viewport,
    mouse, wgpu,
    widget::shader::{Pipeline as ShaderPipeline, Primitive, Program},
};
use std::sync::Arc;

use crate::domains::ui::messages::UiMessage;
use crate::infra::color::ColorPoint;

pub use render_pipeline::ColorPickerGlobals;
pub use state::{AccentColorConfig, ColorPickerInteraction};

/// Messages emitted by the color picker widget
#[derive(Debug, Clone)]
pub enum ColorPickerMessage {
    /// Primary hue changed (degrees 0-360)
    HueChanged(f32),
    /// Primary saturation changed (0-100)
    SaturationChanged(f32),
    /// Both hue and saturation changed (from wheel drag)
    HueSatChanged { hue: f32, saturation: f32 },
    /// Drag started on a point
    DragStarted(ColorPoint),
    /// Drag ended
    DragEnded,
    /// Hover state changed
    HoverChanged(Option<ColorPoint>),
}

/// The shader program for rendering the color picker
#[derive(Debug, Clone)]
pub struct ColorPickerProgram {
    /// Current color configuration
    pub config: AccentColorConfig,
    /// Widget bounds for scaling
    pub wheel_radius: f32,
    /// Handle radius
    pub handle_radius: f32,
    /// Callback for color changes
    pub on_change: Option<fn(ColorPickerMessage) -> UiMessage>,
    /// Stable ID for this program instance
    id: usize,
}

/// Interaction state stored per-widget instance
#[derive(Debug, Clone, Default)]
pub struct ColorPickerProgramState {
    /// Current mouse position relative to widget
    pub mouse_position: Option<Point>,
    /// Which color point is currently hovered
    pub hovered_point: Option<ColorPoint>,
    /// Which color point is being dragged
    pub dragging_point: Option<ColorPoint>,
    /// Whether primary mouse button was pressed inside this widget
    pub pressed_inside: bool,
    /// Animation values for hover state (0.0-1.0) for [primary, comp1, comp2]
    pub hover_animations: [f32; 3],
    /// Animation value for drag state (0.0-1.0)
    pub drag_animation: f32,
}

// Global counter for generating unique IDs
static COLOR_PICKER_ID_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl Program<UiMessage> for ColorPickerProgram {
    type State = ColorPickerProgramState;
    type Primitive = ColorPickerPrimitive;

    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        // Build interaction state from program state
        let interaction = ColorPickerInteraction {
            mouse_position: state.mouse_position,
            hovered_point: state.hovered_point,
            dragging_point: state.dragging_point,
            pressed_inside: state.pressed_inside,
            hover_animations: state.hover_animations,
            drag_animation: state.drag_animation,
        };

        ColorPickerPrimitive {
            bounds,
            config: self.config.clone(),
            interaction,
            wheel_radius: self.wheel_radius,
            handle_radius: self.handle_radius,
            program_id: self.id,
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<UiMessage>> {
        // Use widget-local coordinates (must match shader)
        let center = Self::center_from_bounds(bounds);
        let wheel_radius = Self::wheel_radius_from_bounds(bounds);

        if let Event::Mouse(mouse_event) = event {
            match mouse_event {
                mouse::Event::CursorMoved { .. } => {
                    if let Some(position) = cursor.position() {
                        let local_pos = Self::to_local(position, bounds);

                        // Always process dragging, even outside bounds
                        if state.dragging_point.is_some() {
                            // Calculate new hue and saturation from local position
                            let (new_hue, new_sat) =
                                AccentColorConfig::position_to_hue_sat(
                                    local_pos,
                                    center,
                                    wheel_radius,
                                );

                            // Emit color change message
                            if let Some(on_change) = self.on_change {
                                return Some(iced::widget::Action::publish(
                                    on_change(
                                        ColorPickerMessage::HueSatChanged {
                                            hue: new_hue,
                                            saturation: new_sat,
                                        },
                                    ),
                                ));
                            }
                        }

                        if bounds.contains(position) {
                            state.mouse_position = Some(local_pos);

                            // Hit test handles for hover state
                            let old_hovered = state.hovered_point;
                            state.hovered_point =
                                self.hit_test_handles(position, bounds);

                            if old_hovered != state.hovered_point {
                                return Some(
                                    iced::widget::Action::request_redraw(),
                                );
                            }
                            return Some(iced::widget::Action::request_redraw());
                        } else {
                            // Mouse outside widget bounds
                            let was_hovered = state.hovered_point.is_some();
                            state.mouse_position = None;
                            state.hovered_point = None;

                            if was_hovered {
                                return Some(
                                    iced::widget::Action::request_redraw(),
                                );
                            }
                        }
                    }
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    // Verify cursor is within widget bounds
                    if let Some(cursor_pos) = cursor.position() {
                        if !bounds.contains(cursor_pos) {
                            return None;
                        }

                        let local_pos = Self::to_local(cursor_pos, bounds);

                        // Check if click is within the wheel area
                        let dist_from_center = ((local_pos.x - center.x)
                            .powi(2)
                            + (local_pos.y - center.y).powi(2))
                        .sqrt();

                        if dist_from_center <= wheel_radius {
                            // Start dragging from anywhere on the wheel
                            state.pressed_inside = true;
                            state.dragging_point = Some(ColorPoint::Primary);
                            state.drag_animation = 1.0;

                            // Set initial color position
                            let (new_hue, new_sat) =
                                AccentColorConfig::position_to_hue_sat(
                                    local_pos,
                                    center,
                                    wheel_radius,
                                );

                            if let Some(on_change) = self.on_change {
                                return Some(iced::widget::Action::publish(
                                    on_change(
                                        ColorPickerMessage::HueSatChanged {
                                            hue: new_hue,
                                            saturation: new_sat,
                                        },
                                    ),
                                ));
                            }
                        }

                        return Some(iced::widget::Action::request_redraw());
                    }
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    if !state.pressed_inside {
                        return None;
                    }

                    state.pressed_inside = false;
                    let was_dragging = state.dragging_point.is_some();
                    state.dragging_point = None;
                    state.drag_animation = 0.0;

                    if was_dragging {
                        if let Some(on_change) = self.on_change {
                            return Some(iced::widget::Action::publish(
                                on_change(ColorPickerMessage::DragEnded),
                            ));
                        }
                        return Some(iced::widget::Action::request_redraw());
                    }
                }
                _ => {}
            }
        }
        None
    }
}

impl ColorPickerProgram {
    /// Compute widget size (smaller of width/height for 1:1 aspect)
    fn widget_size_from_bounds(bounds: Rectangle) -> f32 {
        bounds.width.min(bounds.height)
    }

    /// Compute wheel radius from bounds (must match shader calculation)
    fn wheel_radius_from_bounds(bounds: Rectangle) -> f32 {
        Self::widget_size_from_bounds(bounds) * 0.42
    }

    /// Compute handle radius from wheel radius
    fn handle_radius_from_wheel(wheel_radius: f32) -> f32 {
        (wheel_radius * 0.05).max(8.0)
    }

    /// Compute center in widget-local coordinates (must match shader)
    fn center_from_bounds(bounds: Rectangle) -> Point {
        let widget_size = Self::widget_size_from_bounds(bounds);
        Point::new(widget_size * 0.5, widget_size * 0.5)
    }

    /// Convert screen position to widget-local position
    fn to_local(pos: Point, bounds: Rectangle) -> Point {
        Point::new(pos.x - bounds.x, pos.y - bounds.y)
    }

    /// Hit test to find which handle (if any) is under the given position
    fn hit_test_handles(
        &self,
        mouse: Point,
        bounds: Rectangle,
    ) -> Option<ColorPoint> {
        let local_mouse = Self::to_local(mouse, bounds);
        let center = Self::center_from_bounds(bounds);
        let wheel_radius = Self::wheel_radius_from_bounds(bounds);
        let handle_radius = Self::handle_radius_from_wheel(wheel_radius);
        let positions = self.config.all_handle_positions(center, wheel_radius);
        let hit_radius_sq = (handle_radius * 1.5).powi(2);

        for (i, pos) in positions.iter().enumerate() {
            if let Some(p) = pos {
                let dist_sq = (local_mouse.x - p.x).powi(2)
                    + (local_mouse.y - p.y).powi(2);
                if dist_sq <= hit_radius_sq {
                    return ColorPoint::from_index(i);
                }
            }
        }
        None
    }
}

/// The primitive that renders the color picker
#[derive(Debug, Clone)]
pub struct ColorPickerPrimitive {
    pub bounds: Rectangle,
    pub config: AccentColorConfig,
    pub interaction: ColorPickerInteraction,
    pub wheel_radius: f32,
    pub handle_radius: f32,
    pub program_id: usize,
}

/// Pipeline state for the color picker renderer
#[derive(Debug)]
struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    globals_bind_group_layout: Arc<wgpu::BindGroupLayout>,
}

/// Shared renderer state
#[derive(Debug, Default)]
struct State {
    globals_buffer: Option<wgpu::Buffer>,
    globals_bind_group: Option<wgpu::BindGroup>,
}

/// The renderer for color picker primitives
#[derive(Debug)]
pub struct ColorPickerRenderer {
    pipeline: Pipeline,
    state: State,
}

impl ShaderPipeline for ColorPickerRenderer {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        ColorPickerRenderer {
            pipeline: Pipeline::new(device, format),
            state: State::default(),
        }
    }
}

impl Pipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Load shader
        let shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Color Picker Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../../shaders/color_picker.wgsl").into(),
                ),
            });

        // Create globals bind group layout
        let globals_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Color Picker Globals"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Create provider layout
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Color Picker Pipeline Layout"),
                bind_group_layouts: &[&globals_bind_group_layout],
                push_constant_ranges: &[],
            });

        // Create render provider
        let render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Color Picker Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
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

        Pipeline {
            render_pipeline,
            globals_bind_group_layout: Arc::new(globals_bind_group_layout),
        }
    }
}

impl Primitive for ColorPickerPrimitive {
    type Pipeline = ColorPickerRenderer;

    fn prepare(
        &self,
        renderer: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let pipeline = &renderer.pipeline;
        let state = &mut renderer.state;

        // Create globals buffer if needed
        if state.globals_buffer.is_none() {
            let globals_buffer =
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Color Picker Globals Buffer"),
                    size: std::mem::size_of::<ColorPickerGlobals>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

            let globals_bind_group =
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Color Picker Globals Bind Group"),
                    layout: pipeline.globals_bind_group_layout.as_ref(),
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: globals_buffer.as_entire_binding(),
                    }],
                });

            state.globals_buffer = Some(globals_buffer);
            state.globals_bind_group = Some(globals_bind_group);
        }

        // Build globals from current state
        // Use bounds from prepare() which has viewport-space position (accounts for scroll)
        let globals = self.build_globals(viewport, bounds);

        // Upload globals
        if let Some(buffer) = state.globals_buffer.as_ref() {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&[globals]));
        }
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
        render_pass.draw(0..4, 0..1);
        true
    }
}

impl ColorPickerPrimitive {
    /// Build GPU globals from current state
    ///
    /// # Arguments
    /// * `viewport` - The viewport for projection matrix and scale factor
    /// * `viewport_bounds` - Bounds from prepare() which are in viewport space (scroll-adjusted)
    fn build_globals(
        &self,
        viewport: &Viewport,
        viewport_bounds: &Rectangle,
    ) -> ColorPickerGlobals {
        let transform: [f32; 16] = viewport.projection().into();

        // Primary handle position (hue as 0-1, saturation as 0-1)
        let primary_hue = self.config.primary_hue / 360.0;
        let primary_sat = self.config.primary_saturation / 100.0;

        // Get complement hues
        let comp_hues = self
            .config
            .harmony_mode
            .complementary_hues(self.config.primary_hue);
        let comp1_hue = comp_hues.first().map(|h| h / 360.0).unwrap_or(0.0);
        let comp2_hue = comp_hues.get(1).map(|h| h / 360.0).unwrap_or(0.0);
        let comp1_active = if !comp_hues.is_empty() { 1.0 } else { 0.0 };
        let comp2_active = if comp_hues.len() >= 2 { 1.0 } else { 0.0 };

        // Animation values from interaction state
        let hover_anims = self.interaction.hover_animations;
        let drag_anim = self.interaction.drag_animation;

        // Hovered handle ID (-1 = none, 0 = primary, 1 = comp1, 2 = comp2)
        let hovered_id = self
            .interaction
            .hovered_point
            .map(|p| p.index() as f32)
            .unwrap_or(-1.0);

        // Get actual HSLuv-computed RGB colors for accurate handle display
        let primary_rgb = self.config.primary_color();
        let comp1_rgb = self.config.complement1_color();
        let comp2_rgb = self.config.complement2_color();

        // Use viewport_bounds position (scroll-adjusted) but original widget size
        // This ensures correct scrolling while maintaining fixed aspect ratio
        ColorPickerGlobals {
            transform,
            bounds: [
                viewport_bounds.x, // Viewport-space position (accounts for scroll)
                viewport_bounds.y, // Viewport-space position (accounts for scroll)
                self.bounds.width, // Original widget size (not clipped)
                self.bounds.height, // Original widget size (not clipped)
            ],
            geometry: [
                self.wheel_radius,
                self.config.lightness / 100.0,
                0.0, // unused
                0.0, // unused
            ],
            handle_primary: [
                primary_hue,
                primary_sat,
                hover_anims[0],
                drag_anim,
            ],
            handle_comp1: [
                comp1_hue,
                primary_sat,
                hover_anims[1],
                comp1_active,
            ],
            handle_comp2: [
                comp2_hue,
                primary_sat,
                hover_anims[2],
                comp2_active,
            ],
            visual_params: [
                self.handle_radius,
                2.0, // border_width
                0.0, // time (unused for now)
                viewport.scale_factor(),
            ],
            mouse_state: [
                self.interaction.mouse_position.map(|p| p.x).unwrap_or(0.0),
                self.interaction.mouse_position.map(|p| p.y).unwrap_or(0.0),
                hovered_id,
                0.0,
            ],
            primary_color_rgb: [
                primary_rgb.r,
                primary_rgb.g,
                primary_rgb.b,
                1.0,
            ],
            comp1_color_rgb: comp1_rgb
                .map_or([0.0; 4], |c| [c.r, c.g, c.b, 1.0]),
            comp2_color_rgb: comp2_rgb
                .map_or([0.0; 4], |c| [c.r, c.g, c.b, 1.0]),
        }
    }
}

/// Color picker widget builder
#[derive(Debug)]
pub struct ColorPicker {
    config: AccentColorConfig,
    wheel_radius: f32,
    handle_radius: f32,
    on_change: Option<fn(ColorPickerMessage) -> UiMessage>,
}

impl ColorPicker {
    /// Create a new color picker widget
    pub fn new(config: &AccentColorConfig) -> Self {
        Self {
            config: config.clone(),
            wheel_radius: 60.0,
            handle_radius: 8.0,
            on_change: None,
        }
    }

    /// Create a responsive color picker sized for available width and scale
    ///
    /// # Arguments
    /// * `config` - The accent color configuration
    /// * `available_width` - Available horizontal space in logical pixels
    /// * `scale` - UI scale factor from SizeProvider
    ///
    /// The widget will fill the available width scaled by the UI scale factor.
    /// The shader computes the actual wheel radius from the widget bounds.
    pub fn responsive(
        config: &AccentColorConfig,
        available_width: f32,
        scale: f32,
    ) -> Self {
        // Widget size in logical pixels, scaled by UI scale
        let widget_size = available_width * COLOR_PICKER_FILL_RATIO * scale;
        let wheel_radius = widget_size / 2.0;
        let handle_radius =
            (wheel_radius * COLOR_PICKER_HANDLE_RATIO).max(10.0 * scale);
        Self {
            config: config.clone(),
            wheel_radius,
            handle_radius,
            on_change: None,
        }
    }

    /// Set the wheel radius
    pub fn wheel_radius(mut self, radius: f32) -> Self {
        self.wheel_radius = radius;
        self
    }

    /// Set the handle radius
    pub fn handle_radius(mut self, radius: f32) -> Self {
        self.handle_radius = radius;
        self
    }

    /// Set the callback for color changes
    pub fn on_change(
        mut self,
        callback: fn(ColorPickerMessage) -> UiMessage,
    ) -> Self {
        self.on_change = Some(callback);
        self
    }
}

impl<'a> From<ColorPicker> for Element<'a, UiMessage> {
    fn from(picker: ColorPicker) -> Self {
        let id = COLOR_PICKER_ID_COUNTER
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let size = picker.wheel_radius * 2.0 + picker.handle_radius * 2.0 + 8.0;

        iced::widget::shader(ColorPickerProgram {
            config: picker.config,
            wheel_radius: picker.wheel_radius,
            handle_radius: picker.handle_radius,
            on_change: picker.on_change,
            id,
        })
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
    }
}

/// Helper function to create a color picker widget
pub fn color_picker(config: &AccentColorConfig) -> ColorPicker {
    ColorPicker::new(config)
}
