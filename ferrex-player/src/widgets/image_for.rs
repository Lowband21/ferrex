//! Declarative image widget for the unified image system
//!
//! This widget provides a simple, declarative API for displaying media images
//! with automatic loading, caching, and animation support.

use crate::widgets::rounded_image_shader::AnimatedPosterBounds;
use crate::{
    api_types::{EpisodeReference, MovieReference, SeasonReference, SeriesReference},
    image_types::{ImageRequest, ImageSize, Priority},
    messages::ui::Message,
    service_registry,
    widgets::{rounded_image_shader, AnimationType},
};
use ferrex_core::api_types::MediaId;
use iced::{widget::image::Handle, Color, Element, Length};
use lucide_icons::Icon;
use std::time::Duration;

/// A declarative image widget that integrates with UnifiedImageService
pub struct ImageFor {
    media_id: MediaId,
    size: ImageSize,
    radius: f32,
    width: Length,
    height: Length,
    placeholder_icon: Icon,
    placeholder_text: Option<String>,
    priority: Priority,
    animation: AnimationType,
    theme_color: Option<Color>,
    is_hovered: bool,
    on_play: Option<Message>,
    on_click: Option<Message>,
    progress: Option<f32>,
    progress_color: Option<Color>,
}

impl ImageFor {
    /// Create a new image widget for the given media ID
    pub fn new(media_id: MediaId) -> Self {
        // Determine default size based on media type
        let (default_size, default_icon) = match &media_id {
            MediaId::Movie(_) => (ImageSize::Poster, Icon::Film),
            MediaId::Series(_) => (ImageSize::Poster, Icon::Tv),
            MediaId::Season(_) => (ImageSize::Poster, Icon::Tv),
            MediaId::Episode(_) => (ImageSize::Thumbnail, Icon::Play),
            MediaId::Person(_) => (ImageSize::Profile, Icon::User),
        };

        Self {
            media_id,
            size: default_size,
            radius: 8.0,
            width: Length::Fixed(200.0),
            height: Length::Fixed(300.0),
            placeholder_icon: default_icon,
            placeholder_text: None,
            priority: Priority::Preload,
            animation: AnimationType::enhanced_flip(),
            theme_color: None,
            is_hovered: false,
            on_play: None,
            on_click: None,
            progress: None,
            progress_color: None,
        }
    }

    /// Set the image size/type to load
    pub fn size(mut self, size: ImageSize) -> Self {
        // Update dimensions based on size
        let (w, h) = match size {
            ImageSize::Thumbnail => (150.0, 225.0),
            ImageSize::Poster => (200.0, 300.0),
            ImageSize::Backdrop => (400.0, 225.0),
            ImageSize::Full => (300.0, 450.0),
            ImageSize::Profile => (120.0, 180.0),
        };
        self.width = Length::Fixed(w);
        self.height = Length::Fixed(h);
        self.size = size;
        self
    }

    /// Set the corner radius
    pub fn rounded(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Set custom width
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Set custom height
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Set the placeholder icon
    pub fn placeholder(mut self, icon: Icon) -> Self {
        self.placeholder_icon = icon;
        self
    }

    /// Set placeholder text (shown below icon)
    pub fn placeholder_text(mut self, text: impl Into<String>) -> Self {
        self.placeholder_text = Some(text.into());
        self
    }

    /// Set the loading priority
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set priority based on visibility (convenience method)
    pub fn visible(mut self, is_visible: bool) -> Self {
        self.priority = if is_visible {
            Priority::Visible
        } else {
            Priority::Preload
        };
        self
    }

    /// Set the animation type
    pub fn animation(mut self, animation: AnimationType) -> Self {
        self.animation = animation;
        self
    }

    /// Disable animation
    pub fn no_animation(mut self) -> Self {
        self.animation = AnimationType::None;
        self
    }

    /// Set the theme color for flip animation backface
    pub fn theme_color(mut self, color: Color) -> Self {
        self.theme_color = Some(color);
        self
    }

    /// Set the hover state
    pub fn is_hovered(mut self, hovered: bool) -> Self {
        self.is_hovered = hovered;
        self
    }

    /// Set the play button callback
    pub fn on_play(mut self, message: Message) -> Self {
        self.on_play = Some(message);
        self
    }

    /// Set the click callback (for empty space)
    pub fn on_click(mut self, message: Message) -> Self {
        self.on_click = Some(message);
        self
    }

    /// Set the watch progress (0.0 = unwatched, 0.0-0.95 = in progress, 1.0 = watched)
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    /// Set the progress indicator color
    pub fn progress_color(mut self, color: Color) -> Self {
        self.progress_color = Some(color);
        self
    }
}

/// Helper function to create an image widget
pub fn image_for(media_id: impl Into<MediaId>) -> ImageFor {
    ImageFor::new(media_id.into())
}

// Note: From implementations for MediaId types are handled in api_types module

impl<'a> From<ImageFor> for Element<'a, Message> {
    fn from(image: ImageFor) -> Self {
        // Get fixed dimensions for layout
        let width = match image.width {
            Length::Fixed(w) => w,
            _ => 200.0, // Default fallback
        };
        let height = match image.height {
            Length::Fixed(h) => h,
            _ => 300.0, // Default fallback
        };

        // Create animated bounds for proper sizing
        let bounds = AnimatedPosterBounds::new(width, height);

        // Create the image request
        let request = ImageRequest {
            media_id: image.media_id.clone(),
            size: image.size,
            priority: image.priority,
        };

        // Check if we have access to the image service
        if let Some(image_service) = service_registry::get_image_service() {
            // Check the cache first
            if let Some((handle, loaded_at)) = image_service.get().get_with_load_time(&request) {
                //log::debug!("image_for: Cache HIT for {:?}", request.media_id);
                //log::debug!("  - loaded_at from get_with_load_time: {:?}", loaded_at);
                //log::debug!("  - animation type: {:?}", image.animation);
                //log::debug!("  - theme_color: {:?}", image.theme_color);

                // We have a cached image, use it with the rounded shader
                let mut shader: rounded_image_shader::RoundedImage = rounded_image_shader(handle)
                    .radius(image.radius)
                    .with_animated_bounds(bounds)
                    .is_hovered(image.is_hovered);

                // Set theme color if provided
                if let Some(color) = image.theme_color {
                    shader = shader.theme_color(color);
                }

                // Always set up button callbacks so they work when hovering
                // Use the play callback if provided by the caller
                if let Some(play_msg) = image.on_play.clone() {
                    shader = shader.on_play(play_msg);
                }
                // Use the click callback for empty space (details page)
                if let Some(click_msg) = image.on_click.clone() {
                    shader = shader.on_click(click_msg);
                }

                // Set the actual load time if available
                if let Some(load_time) = loaded_at {
                    //log::debug!("  - Setting load_time on shader: {:?}", load_time);
                    shader = shader.with_load_time(load_time);
                } else {
                    //log::debug!("  - No load_time, not setting on shader");
                }

                // Determine if we should animate based on how recently the image was loaded
                let should_animate = if let Some(load_time) = loaded_at {
                    // Get animation duration
                    let animation_duration = match image.animation {
                        AnimationType::None => Duration::from_secs(0),
                        AnimationType::Fade { duration } => duration,
                        AnimationType::Flip { duration } => duration,
                        AnimationType::EnhancedFlip { total_duration, .. } => total_duration,
                        AnimationType::PlaceholderSunken => Duration::from_secs(0), // No animation for placeholder
                    };

                    //log::debug!("  - animation type: {:?}", image.animation);
                    //log::debug!("  - animation duration: {:?}", animation_duration);

                    // Check if image was loaded recently (within 2x animation duration)
                    // This gives us a window where animations will play even if there's
                    // a slight delay between loading and display
                    let elapsed = load_time.elapsed();
                    let should = elapsed <= animation_duration * 2;

                    //log::debug!("  - elapsed since load: {:?}", elapsed);
                    //log::debug!("  - should_animate: {} (elapsed <= {:?})", should, animation_duration * 2);

                    should
                } else {
                    // No load time available
                    //log::debug!("  - should_animate: false (no load time)");
                    false
                };

                if should_animate {
                    // Apply animation (load time already set above)
                    //log::debug!("  - APPLYING animation: {:?}", image.animation);
                    shader = shader.with_animation(image.animation);
                } else {
                    // No animation for images loaded too long ago
                    //log::debug!("  - NO animation applied");
                    // Still need to set animation type to None so overlay can show
                    shader = shader.with_animation(AnimationType::None);
                }

                // Set progress indicator if provided
                if let Some(progress) = image.progress {
                    shader = shader.progress(progress);

                    // Use theme color as default progress color if not specified
                    let progress_color = image
                        .progress_color
                        .or(image.theme_color)
                        .unwrap_or(Color::from_rgb(0.0, 0.47, 1.0)); // Default blue

                    shader = shader.progress_color(progress_color);
                }

                shader.into()
            } else {
                // Not in cache, request it and show loading state
                //log::debug!(
                //    "image_for: Cache MISS for {:?}, requesting...",
                //    request.media_id
                //);
                //log::debug!(
                //    "  - Creating placeholder with theme_color: {:?}",
                //    image.theme_color
                //);
                image_service.get().request_image(request);

                create_loading_placeholder(bounds, image.radius, image.theme_color)
            }
        } else {
            // Service not initialized, show loading state
            create_loading_placeholder(bounds, image.radius, image.theme_color)
        }
    }
}

/// Create a loading placeholder using the shader widget
fn create_loading_placeholder<'a>(
    bounds: AnimatedPosterBounds,
    radius: f32,
    theme_color: Option<Color>,
) -> Element<'a, Message> {
    // Create a placeholder handle - we'll use a 1x1 transparent pixel
    // The shader will render the theme color on the backface
    let placeholder_handle = Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);

    // Use theme color or default
    let color = theme_color.unwrap_or_else(|| {
        log::debug!("No theme color provided, using default dark gray");
        Color::from_rgb(0.15, 0.15, 0.15)
    });

    //log::debug!("Creating placeholder shader with color: {:?}", color);

    // Create shader widget in initial sunken state
    // The PlaceholderSunken animation type will show backface with theme color
    // and apply sunken depth effect
    rounded_image_shader(placeholder_handle)
        .radius(radius)
        .with_animated_bounds(bounds)
        .theme_color(color)
        .with_animation(AnimationType::PlaceholderSunken)
        .is_hovered(false) // Placeholders are never hovered
        .into()
}

/// Linear interpolation between two colors
fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Darken a color by a factor (0.0 = black, 1.0 = original color)
fn darken_color(color: Color, factor: f32) -> Color {
    let factor = factor.clamp(0.0, 1.0);
    Color::from_rgba(
        color.r * factor,
        color.g * factor,
        color.b * factor,
        color.a,
    )
}

/// Extension trait for creating image widgets from media references
pub trait ImageForExt {
    fn image_for(&self) -> ImageFor;
}

impl ImageForExt for MovieReference {
    fn image_for(&self) -> ImageFor {
        image_for(MediaId::Movie(self.id.clone()))
    }
}

impl ImageForExt for SeriesReference {
    fn image_for(&self) -> ImageFor {
        image_for(MediaId::Series(self.id.clone()))
    }
}

impl ImageForExt for SeasonReference {
    fn image_for(&self) -> ImageFor {
        image_for(MediaId::Season(self.id.clone()))
    }
}

impl ImageForExt for EpisodeReference {
    fn image_for(&self) -> ImageFor {
        image_for(MediaId::Episode(self.id.clone())).size(ImageSize::Thumbnail)
    }
}
