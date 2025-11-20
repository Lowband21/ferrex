//! Declarative image widget for the unified image system
//!
//! This widget provides a simple, declarative API for displaying media images
//! with automatic loading, caching, and animation support.

use crate::domains::ui::widgets::rounded_image_shader::AnimatedPosterBounds;
use crate::{
    domains::ui::messages::Message,
    domains::ui::widgets::{AnimationType, rounded_image_shader},
    infrastructure::api_types::{
        EpisodeReference, MovieReference, SeasonReference, SeriesReference,
    },
    infrastructure::service_registry,
};
use ferrex_core::{ImageRequest, ImageSize, ImageType, MediaIDLike, Priority};
use iced::{Color, Element, Length, widget::image::Handle};
use lucide_icons::Icon;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Cached image data to avoid repeated lookups
#[derive(Debug, Clone)]
struct CachedImageData {
    handle: Handle,
    loaded_at: Option<std::time::Instant>,
    request_hash: u64,
}

/// A declarative image widget that integrates with UnifiedImageService
pub struct ImageFor {
    media_id: Uuid,
    size: ImageSize,
    image_type: ImageType,
    radius: f32,
    width: Length,
    height: Length,
    placeholder_icon: Icon,
    placeholder_text: Option<String>,
    priority: Priority,
    image_index: u32,
    animation: AnimationType,
    theme_color: Option<Color>,
    is_hovered: bool,
    on_play: Option<Message>,
    on_click: Option<Message>,
    progress: Option<f32>,
    progress_color: Option<Color>,
    // Optimization: Cache to avoid repeated lookups
    cached_data: Option<CachedImageData>,
}

impl ImageFor {
    /// Create a new image widget for the given media ID
    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    pub fn new(media_id: Uuid) -> Self {
        Self {
            media_id,
            size: ImageSize::Poster,
            image_type: ImageType::Movie,
            radius: crate::infrastructure::constants::layout::poster::CORNER_RADIUS,
            width: Length::Fixed(200.0),
            height: Length::Fixed(300.0),
            // Might want to use a different default icon
            placeholder_icon: Icon::FileArchive,
            placeholder_text: None,
            priority: Priority::Preload,
            image_index: 0,
            // Default; callers (views) should set from UI state
            animation: AnimationType::None,
            theme_color: None,
            is_hovered: false,
            on_play: None,
            on_click: None,
            progress: None,
            progress_color: None,
            cached_data: None,
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

    /// Set the image type to load
    pub fn image_type(mut self, image_type: ImageType) -> Self {
        self.image_type = image_type;
        self
    }

    /// Set the corner radius
    pub fn radius(mut self, radius: f32) -> Self {
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

    /// Set the image index for multi-image categories (e.g. cast order)
    pub fn image_index(mut self, index: u32) -> Self {
        self.image_index = index;
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
pub fn image_for(media_id: Uuid) -> ImageFor {
    ImageFor::new(media_id)
}

// Note: From implementations for MediaID types are handled in api_types module

// Thread-local cache for the image service to avoid repeated lookups
thread_local! {
    static CACHED_IMAGE_SERVICE: std::cell::RefCell<Option<crate::infrastructure::service_registry::ImageServiceHandle>> = std::cell::RefCell::new(None);
}

impl<'a> From<ImageFor> for Element<'a, Message> {
    fn from(mut image: ImageFor) -> Self {
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
        let request = ImageRequest::new(image.media_id, image.size, image.image_type)
            .with_priority(image.priority)
            .with_index(image.image_index);

        // Calculate request hash for cache invalidation
        let request_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            request.media_id.hash(&mut hasher);
            request.size.hash(&mut hasher);
            request.image_index.hash(&mut hasher);
            //(request.priority as u8).hash(&mut hasher);
            hasher.finish()
        };

        // Fast path: Check if we have cached data and it's still valid
        if let Some(cached) = &image.cached_data {
            if cached.request_hash == request_hash {
                // Profile cache reuse (fast path)
                #[cfg(any(
                    feature = "profile-with-puffin",
                    feature = "profile-with-tracy",
                    feature = "profile-with-tracing"
                ))]
                profiling::scope!("UI::Poster::CacheReuse::FastPath");

                // Reuse cached data without service lookup or DashMap access
                return create_shader_from_cached(
                    cached.handle.clone(),
                    request_hash,
                    cached.loaded_at,
                    &image,
                    bounds,
                );
            }
        }

        // Slow path: Get or cache the image service (thread-local optimization)
        let image_service = CACHED_IMAGE_SERVICE.with(|cache| {
            let mut cached = cache.borrow_mut();
            if cached.is_none() {
                *cached = service_registry::get_image_service();
            }
            cached.clone()
        });

        // Check if we have access to the image service
        if let Some(image_service) = image_service {
            // Check the cache first
            match image_service.get().get_with_load_time(&request) {
                Some((handle, loaded_at)) => {
                    #[cfg(any(
                        feature = "profile-with-puffin",
                        feature = "profile-with-tracy",
                        feature = "profile-with-tracing"
                    ))]
                    profiling::scope!("image_for::CacheHit");

                    image.cached_data = Some(CachedImageData {
                        handle: handle.clone(),
                        loaded_at,
                        request_hash,
                    });

                    let mut shader: rounded_image_shader::RoundedImage =
                        rounded_image_shader(handle, Some(request_hash))
                            .radius(image.radius)
                            .with_animated_bounds(bounds)
                            .is_hovered(image.is_hovered);

                    if let Some(color) = image.theme_color {
                        shader = shader.theme_color(color);
                    }

                    if let Some(play_msg) = image.on_play.clone() {
                        shader = shader.on_play(play_msg);
                    }

                    if let Some(click_msg) = image.on_click.clone() {
                        shader = shader.on_click(click_msg);
                    }

                    /*
                    if let Some(load_time) = loaded_at {
                        shader = shader.with_load_time(load_time);
                    } else {
                    } */

                    /*
                    let should_animate = if let Some(load_time) = loaded_at {
                        // Get animation duration
                        let animation_duration = match image.animation {
                            AnimationType::None => Duration::from_secs(0),
                            AnimationType::Fade { duration } => duration,
                            AnimationType::Flip { duration } => duration,
                            AnimationType::EnhancedFlip { total_duration, .. } => total_duration,
                            AnimationType::PlaceholderSunken => Duration::from_secs(0), // No animation for placeholder
                        };

                        // Check if image was loaded recently (within 2x animation duration)
                        // This gives us a window where animations will play even if there's
                        // a slight delay between loading and display
                        let elapsed = load_time.elapsed();
                        let should = elapsed <= animation_duration * 10;

                        should
                    } else {
                        false
                    }; */

                    //if should_animate {
                    shader = shader.with_animation(image.animation);
                    //} else {
                    //shader = shader.with_animation(AnimationType::None);
                    //}

                    // Set progress indicator if provided
                    if let Some(progress) = image.progress {
                        shader = shader.progress(progress);

                        let progress_color = Color::from_rgb(0.0, 0.47, 1.0); // Default blue

                        shader = shader.progress_color(progress_color);
                    }

                    shader.into()
                }
                _ => {
                    // Profile image request for loading
                    #[cfg(any(
                        feature = "profile-with-puffin",
                        feature = "profile-with-tracy",
                        feature = "profile-with-tracing"
                    ))]
                    profiling::scope!("image_for::CacheMiss");

                    image_service.get().request_image(request);

                    create_loading_placeholder(bounds, image.radius, image.theme_color)
                }
            }
        } else {
            // Service not initialized, show loading state
            create_loading_placeholder(bounds, image.radius, image.theme_color)
        }
    }
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
fn create_shader_from_cached<'a>(
    handle: Handle,
    request_hash: u64,
    loaded_at: Option<std::time::Instant>,
    image: &ImageFor,
    bounds: AnimatedPosterBounds,
) -> Element<'a, Message> {
    // Check if we should skip atlas upload for VeryFast scrolling
    let mut shader = rounded_image_shader(handle, Some(request_hash))
        .radius(image.radius)
        .with_animated_bounds(bounds)
        .is_hovered(image.is_hovered);

    // Set theme color if provided
    if let Some(color) = image.theme_color {
        shader = shader.theme_color(color);
    }

    // Set up button callbacks
    if let Some(play_msg) = image.on_play.clone() {
        shader = shader.on_play(play_msg);
    }
    if let Some(click_msg) = image.on_click.clone() {
        shader = shader.on_click(click_msg);
    }

    // Set load time if available
    /*
    if let Some(load_time) = loaded_at {
        shader = shader.with_load_time(load_time);
    }

    // Determine animation based on load time
    let should_animate = if let Some(load_time) = loaded_at {
        let animation_duration = match image.animation {
            AnimationType::None => Duration::from_secs(0),
            AnimationType::Fade { duration } => duration,
            AnimationType::Flip { duration } => duration,
            AnimationType::EnhancedFlip { total_duration, .. } => total_duration,
            AnimationType::PlaceholderSunken => Duration::from_secs(0),
        };
        load_time.elapsed() <= animation_duration * 2
    } else {
        false
    };*/

    //if should_animate {
    shader = shader.with_animation(image.animation);
    //} else {
    //shader = shader.with_animation(AnimationType::None);
    //}

    // Set progress indicator if provided
    if let Some(progress) = image.progress {
        shader = shader.progress(progress);
        let progress_color = Color::from_rgb(0.0, 0.47, 1.0);
        shader = shader.progress_color(progress_color);
    }

    shader.into()
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
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
    rounded_image_shader(placeholder_handle, None)
        .radius(radius)
        .with_animated_bounds(bounds)
        .theme_color(color)
        .with_animation(AnimationType::PlaceholderSunken)
        .is_hovered(false) // Placeholders are never hovered
        .into()
}

/// Extension trait for creating image widgets from media references
pub trait ImageForExt {
    fn image_for(&self) -> ImageFor;
}

impl ImageForExt for MovieReference {
    fn image_for(&self) -> ImageFor {
        image_for(self.id.to_uuid())
            .placeholder(Icon::Film)
            .image_type(ImageType::Movie)
    }
}

impl ImageForExt for SeriesReference {
    fn image_for(&self) -> ImageFor {
        image_for(self.id.to_uuid())
            .placeholder(Icon::Tv)
            .image_type(ImageType::Series)
    }
}

impl ImageForExt for SeasonReference {
    fn image_for(&self) -> ImageFor {
        image_for(self.id.to_uuid())
            .placeholder(Icon::Tv)
            .image_type(ImageType::Season)
    }
}

impl ImageForExt for EpisodeReference {
    fn image_for(&self) -> ImageFor {
        image_for(self.id.to_uuid())
            .placeholder(Icon::FileImage)
            .image_type(ImageType::Episode)
    }
}
