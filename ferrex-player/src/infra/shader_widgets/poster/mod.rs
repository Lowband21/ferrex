//! Shader-based poster widget for Iced
//!
//! This implementation uses GPU shaders for true rounded rectangle clipping
//! with anti-aliasing, providing better performance than Canvas-based approaches.

mod batch_state;
pub mod poster_animation_types;
mod poster_program;
mod primitive;
mod render_pipeline;
pub use render_pipeline::PosterFace;

use crate::{
    domains::ui::messages::UiMessage,
    infra::shader_widgets::poster::poster_animation_types::PosterAnimationType,
};

use iced::{Color, Element, Length, widget::image::Handle};

use iced_wgpu::primitive::register_batchable_type;

use std::{sync::OnceLock, time::Instant};

static BATCH_REGISTRATION: OnceLock<()> = OnceLock::new();

fn ensure_batch_registration() {
    BATCH_REGISTRATION.get_or_init(|| {
        register_batchable_type::<primitive::PosterPrimitive>();
    });
}

/// A widget that displays a poster with rounded corners using GPU shaders
pub struct Poster {
    id: u64,
    menu_target: Option<uuid::Uuid>,
    handle: Handle,
    radius: f32,
    width: Length,
    height: Length,
    animation: PosterAnimationType,
    load_time: Option<Instant>,
    opacity: f32,
    theme_color: Color,
    bounds: Option<poster_animation_types::AnimatedPosterBounds>,
    is_hovered: bool,
    on_play: Option<UiMessage>,
    on_edit: Option<UiMessage>,
    on_options: Option<UiMessage>,
    on_click: Option<UiMessage>, // For clicking empty space (details page)
    progress: Option<f32>,       // Progress percentage (0.0 to 1.0)
    progress_color: Color,       // Color for the progress bar
    rotation_y: Option<f32>,
    face: PosterFace,
}

impl Poster {
    /// Creates a new rounded image with a single handle
    pub fn new(handle: Handle, id: Option<u64>) -> Self {
        use crate::domains::ui::theme::MediaServerTheme;

        Self {
            id: id.unwrap_or(0),
            menu_target: None,
            handle,
            radius: crate::infra::constants::layout::poster::CORNER_RADIUS,
            width: Length::Fixed(200.0),
            height: Length::Fixed(300.0),
            animation: PosterAnimationType::None,
            load_time: None,
            opacity: 1.0,
            theme_color: Color::from_rgb(0.1, 0.1, 0.1), // Default dark gray
            bounds: None,
            is_hovered: false,
            on_play: None,
            on_edit: None,
            on_options: None,
            on_click: None,
            progress: None,
            progress_color: MediaServerTheme::ACCENT_BLUE, // Default to theme blue
            rotation_y: None,
            face: PosterFace::Front,
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
    pub fn with_animation(mut self, animation: PosterAnimationType) -> Self {
        self.animation = animation;
        /*
        if self.load_time.is_none() {
            self.load_time = Some(Instant::now());
        } */
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

    /// Sets the theme color for backface
    pub fn theme_color(mut self, color: Color) -> Self {
        self.theme_color = color;
        self
    }

    /// Sets animated bounds with padding
    pub fn with_animated_bounds(
        mut self,
        bounds: poster_animation_types::AnimatedPosterBounds,
    ) -> Self {
        self.bounds = Some(bounds);
        // Use layout bounds for stable grid positioning
        let (width, height) = bounds.layout_bounds();
        self.width = Length::Fixed(width);
        self.height = Length::Fixed(height);
        self
    }

    /// Sets the hover state
    pub fn is_hovered(mut self, hovered: bool) -> Self {
        self.is_hovered = hovered;
        self
    }

    /// Sets the play button callback
    pub fn on_play(mut self, message: UiMessage) -> Self {
        self.on_play = Some(message);
        self
    }

    /// Sets the edit button callback
    pub fn on_edit(mut self, message: UiMessage) -> Self {
        self.on_edit = Some(message);
        self
    }

    /// Sets the options button callback
    pub fn on_options(mut self, message: UiMessage) -> Self {
        self.on_options = Some(message);
        self
    }

    /// Sets the click callback (for clicking empty space)
    pub fn on_click(mut self, message: UiMessage) -> Self {
        self.on_click = Some(message);
        self
    }

    /// Overrides rotation_y (radians) for custom flip control.
    pub fn rotation_y(mut self, rotation: f32) -> Self {
        self.rotation_y = Some(rotation);
        self
    }

    /// Sets the menu target (media id) for right-click toggles
    pub fn menu_target(mut self, media_id: uuid::Uuid) -> Self {
        self.menu_target = Some(media_id);
        self
    }

    /// Sets the progress percentage (0.0 to 1.0)
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    /// Sets the progress bar color
    pub fn progress_color(mut self, color: Color) -> Self {
        self.progress_color = color;
        self
    }

    /// Sets which face/pipeline to render
    pub fn face(mut self, face: PosterFace) -> Self {
        self.face = face;
        self
    }
}

/// Helper function to create a rounded image widget
pub fn poster(handle: Handle, id: Option<u64>) -> Poster {
    Poster::new(handle, id)
}

impl<'a> From<Poster> for Element<'a, UiMessage> {
    fn from(image: Poster) -> Self {
        let shader = iced::widget::shader(poster_program::PosterProgram {
            id: image.id,
            menu_target: image.menu_target,
            handle: image.handle,
            radius: image.radius,
            animation: image.animation,
            load_time: image.load_time,
            opacity: image.opacity,
            theme_color: image.theme_color,
            bounds: image.bounds,
            is_hovered: image.is_hovered,
            progress: image.progress,
            progress_color: image.progress_color,
            on_play: image.on_play,
            on_edit: image.on_edit,
            on_options: image.on_options,
            on_click: image.on_click,
            rotation_y: image.rotation_y,
            face: image.face,
        })
        .width(image.width)
        .height(image.height);

        shader.into()
    }
}
