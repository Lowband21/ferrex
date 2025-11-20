use iced_wgpu::primitive::{
    BatchEncodeContext, BatchPrimitive, PrimitiveBatchState,
};

use iced::{
    Color, Point, Rectangle, Size,
    advanced::graphics::Viewport,
    wgpu,
    widget::{image::Handle, shader::Primitive},
};

use std::time::Instant;

use crate::infra::widgets::poster::{
    batch_state::{PendingPrimitive, PosterBatchState},
    poster_animation_types::{AnimatedPosterBounds, PosterAnimationType},
};

#[derive(Debug, Clone)]
pub struct PosterPrimitive {
    pub id: u64,
    pub handle: Handle,
    pub bounds: Rectangle,
    pub radius: f32,
    pub animation: PosterAnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub animated_bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub mouse_position: Option<Point>, // Mouse position relative to widget
    pub progress: Option<f32>,
    pub progress_color: Color,
}

impl PosterPrimitive {
    pub(crate) fn set_load_time(&mut self, load_time: Instant) {
        self.load_time = Some(load_time);
    }
}

impl Primitive for PosterPrimitive {
    type Renderer = ();

    fn initialize(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _format: wgpu::TextureFormat,
    ) -> Self::Renderer {
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

impl BatchPrimitive for PosterPrimitive {
    type BatchState = PosterBatchState;

    fn create_batch_state(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Self::BatchState {
        PosterBatchState::new(device, format)
    }

    fn encode_batch(
        &self,
        state: &mut Self::BatchState,
        context: &BatchEncodeContext<'_>,
    ) -> bool {
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
