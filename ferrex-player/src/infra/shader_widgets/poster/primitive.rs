use crate::infra::shader_widgets::poster::{
    PosterFace,
    animation::{AnimatedPosterBounds, PosterAnimationType},
    batch_state::{PendingPrimitive, PosterBatchState},
};

use iced::{
    Color, Point, Rectangle, Size,
    advanced::graphics::Viewport,
    wgpu,
    widget::{
        image::Handle,
        shader::{Pipeline, Primitive},
    },
};
use iced_wgpu::primitive::{
    BatchEncodeContext, BatchPrimitive, PrimitiveBatchState,
};

use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PosterPrimitive {
    pub id: u64,
    pub handle: Handle,
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
    pub rotation_override: Option<f32>,
    pub face: PosterFace,
    /// Title text to render below the poster
    pub title: Option<String>,
    /// Meta text (e.g., year) to render below the title
    pub meta: Option<String>,
}

/// No-op provider for batched primitives.
/// This is never actually used because batching always succeeds,
/// but the trait requires a Pipeline type to compile.
pub struct NoopPipeline;

impl Pipeline for NoopPipeline {
    fn new(
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _format: wgpu::TextureFormat,
    ) -> Self {
        NoopPipeline
    }
}

impl Primitive for PosterPrimitive {
    type Pipeline = NoopPipeline;

    fn prepare(
        &self,
        _pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // Batched provider performs all rendering work.
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
            rotation_override: self.rotation_override,
            face: self.face,
            title: self.title.clone(),
            meta: self.meta.clone(),
        });

        true
    }
}
