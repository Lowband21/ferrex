use iced::widget::image::Handle;
use iced::widget::shader::{Primitive, Program, Storage};
use iced::{Element, Length};
use iced::{Rectangle, mouse};
use iced_wgpu::image as wgpu_image;
use iced_wgpu::wgpu;

use crate::domains::ui::messages::Message;

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

impl Program<Message> for TexturePreloaderProgram {
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
    ) -> Option<iced::widget::Action<Message>> {
        // No interaction
        None
    }
}

impl Primitive for TexturePreloaderPrimitive {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn prepare_batched(
        &mut self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        _format: wgpu::TextureFormat,
        _storage: &mut Storage,
        _bounds: &Rectangle,
        _viewport: &iced::advanced::graphics::Viewport,
        image_cache: &mut wgpu_image::Cache,
    ) {
        // Upload each handle to the atlas via iced's cache
        for handle in &self.handles {
            let _ = image_cache.upload_raster(device, encoder, handle);
        }
    }

    fn render_with_cache(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _storage: &Storage,
        _target: &wgpu::TextureView,
        _clip_bounds: &Rectangle<u32>,
        _image_cache: &wgpu_image::Cache,
    ) {
        // No drawing
    }
}

/// Construct a texture preloader element. Place it inside a visible container so it runs every frame.
pub fn texture_preloader(handles: Vec<Handle>, max_per_frame: usize) -> Element<'static, Message> {
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
    image_type: ferrex_core::ImageType,
    size: ferrex_core::ImageSize,
) -> Vec<Handle> {
    use crate::domains::metadata::image_types::{ImageRequest, Priority};
    use crate::infrastructure::service_registry;

    let image_service = service_registry::get_image_service();
    if image_service.is_none() {
        return Vec::new();
    }
    let image_service = image_service.unwrap();

    let mut out = Vec::new();
    for id in ids {
        let request = ImageRequest {
            media_id: id,
            size,
            image_type,
            priority: Priority::Visible,
        };
        if let Some((handle, _loaded_at)) = image_service.get().get_with_load_time(&request) {
            out.push(handle.clone());
        }
    }
    out
}
