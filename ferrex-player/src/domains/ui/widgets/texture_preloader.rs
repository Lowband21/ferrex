use ferrex_model::MediaType;
use iced::widget::image::Handle;
use iced::widget::shader::{Primitive, Program};
use iced::{Element, Length};
use iced::{Rectangle, mouse};
use iced_wgpu::primitive::{
    BatchEncodeContext, BatchPrimitive, PrepareContext, PrimitiveBatchState,
    RenderContext,
};
use iced_wgpu::wgpu;

use crate::domains::ui::messages::UiMessage;
use ferrex_core::player_prelude::{ImageRequest, ImageSize, Priority};

/// A minimal shader program that preloads a set of image handles into the Iced atlas.
/// It does not render anything; it only triggers atlas uploads during the prepare pass.
#[derive(Debug, Clone)]
pub struct TexturePreloaderProgram {
    /// Image handles to upload. Caller should budget per-frame as needed.
    pub handles: Vec<Handle>,
    /// Safety cap to avoid excessive uploads when caller passes many handles.
    pub max_per_frame: usize,
}

#[derive(Debug, Default, Clone)]
pub struct TexturePreloaderState;

#[derive(Debug)]
pub struct TexturePreloaderPrimitive {
    /// Handles to upload this frame (already budgeted/sliced)
    pub handles: Vec<Handle>,
}

impl Program<UiMessage> for TexturePreloaderProgram {
    type State = TexturePreloaderState;
    type Primitive = TexturePreloaderPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        // Slice to budget for this frame; caller should also budget, this is just a hard cap.
        let n = self.max_per_frame.min(self.handles.len());
        let handles = if n > 0 {
            self.handles.iter().take(n).cloned().collect()
        } else {
            Vec::new()
        };

        TexturePreloaderPrimitive { handles }
    }

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<UiMessage>> {
        // No interaction
        None
    }
}

impl Primitive for TexturePreloaderPrimitive {
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
        _viewport: &iced::advanced::graphics::Viewport,
    ) {
        // Upload work handled through batch path
    }
}

#[derive(Default, Debug)]
pub struct TexturePreloaderBatchState {
    pending_handles: Vec<Handle>,
}

impl TexturePreloaderBatchState {
    fn enqueue(&mut self, handles: impl IntoIterator<Item = Handle>) {
        self.pending_handles.extend(handles);
    }
}

impl PrimitiveBatchState for TexturePreloaderBatchState {
    type InstanceData = ();

    fn new(_device: &wgpu::Device, _format: wgpu::TextureFormat) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    fn add_instance(&mut self, _instance: Self::InstanceData) {}

    fn prepare(&mut self, context: &mut PrepareContext<'_>) {
        #[cfg(any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ))]
        profiling::scope!("TexturePreloaderBatchState::prepare");

        let Some(image_cache) = context.resources.image_cache() else {
            self.pending_handles.clear();
            return;
        };

        for handle in self.pending_handles.drain(..) {
            let _ = image_cache.ensure_raster_region(
                context.device,
                context.encoder,
                context.belt,
                &handle,
            );
        }
    }

    fn render(
        &self,
        _render_pass: &mut wgpu::RenderPass<'_>,
        _context: &mut RenderContext<'_>,
        _range: std::ops::Range<u32>,
    ) {
        // Nothing to render
    }

    fn trim(&mut self) {
        self.pending_handles.clear();
    }

    fn instance_count(&self) -> usize {
        self.pending_handles.len()
    }
}

impl BatchPrimitive for TexturePreloaderPrimitive {
    type BatchState = TexturePreloaderBatchState;

    fn create_batch_state(
        _device: &wgpu::Device,
        _format: wgpu::TextureFormat,
    ) -> Self::BatchState {
        TexturePreloaderBatchState::default()
    }

    fn encode_batch(
        &self,
        state: &mut Self::BatchState,
        _context: &BatchEncodeContext<'_>,
    ) -> bool {
        if self.handles.is_empty() {
            return true;
        }

        state.enqueue(self.handles.iter().cloned());
        true
    }
}

/// Construct a texture preloader element. Place it inside a visible container so it runs every frame.
pub fn texture_preloader(
    handles: Vec<Handle>,
    max_per_frame: usize,
) -> Element<'static, UiMessage> {
    iced::widget::shader(TexturePreloaderProgram {
        handles,
        max_per_frame,
    })
    .width(Length::Fixed(1.0))
    .height(Length::Fixed(1.0))
    .into()
}

/// Collect cached handles for the given media IDs, if already decoded and present in the image cache.
/// This does not trigger network requests; it only returns handles that are already available.
pub fn collect_cached_handles_for_media(
    ids: impl IntoIterator<Item = uuid::Uuid>,
    image_type: MediaType,
    size: ImageSize,
) -> Vec<Handle> {
    use crate::infra::service_registry;

    let image_service = service_registry::get_image_service();
    if image_service.is_none() {
        return Vec::new();
    }
    let image_service = image_service.unwrap();

    let mut out = Vec::new();
    for id in ids {
        let request = ImageRequest::new(id, size, image_type)
            .with_priority(Priority::Visible)
            .with_index(0);
        if let Some((handle, _loaded_at)) =
            image_service.get().get_with_load_time(&request)
        {
            out.push(handle.clone());
        }
    }
    out
}
