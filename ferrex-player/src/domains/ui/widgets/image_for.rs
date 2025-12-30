//! Declarative image widget for the unified image system
//!
//! This widget provides a simple, declarative API for displaying media images
//! with automatic loading, caching, and animation support.

use crate::{
    domains::{
        metadata::image_service::{FirstDisplayHint, UnifiedImageService},
        ui::{
            messages::UiMessage, views::virtual_carousel::types::CarouselKey,
        },
    },
    infra::{
        image_log::register_media_title,
        service_registry,
        shader_widgets::poster::{
            Poster, PosterFace, PosterInstanceKey,
            animation::{
                AnimatedPosterBounds, AnimationBehavior, AnimationConfig,
                PosterAnimationType,
            },
            poster,
        },
        theme::{accent, fallback_theme_color_for},
    },
};

use ferrex_core::player_prelude::{ImageRequest, Priority};

use ferrex_model::{
    EpisodeReference, ImageSize, MovieReference, SeasonReference, Series,
};

use iced::{Color, Element, Length, widget::image::Handle};

use lucide_icons::Icon;
use rand::random;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use uuid::Uuid;

/// Cached image data to avoid repeated lookups
#[derive(Debug, Clone)]
struct CachedImageData {
    handle: Handle,
    loaded_at: Option<std::time::Instant>,
    instance_hash: u64,
}

/// A declarative image widget that integrates with UnifiedImageService
#[derive(Debug, Clone)]
pub struct ImageFor {
    media_id: Uuid,
    /// TMDB image variant id (`tmdb_image_variants.id`) to request.
    ///
    /// `None` means "no known image" and will render only a placeholder.
    iid: Option<Uuid>,
    size: ImageSize,
    radius: f32,
    width: Length,
    height: Length,
    placeholder_icon: Icon,
    placeholder_text: Option<String>,
    priority: Priority,
    animation: AnimationBehavior,
    theme_color: Option<Color>,
    is_hovered: bool,
    on_play: Option<UiMessage>,
    on_click: Option<UiMessage>,
    progress: Option<f32>,
    progress_color: Option<Color>,
    rotation_y: Option<f32>,
    face: Option<PosterFace>,
    // Optimization: Cache to avoid repeated lookups
    cached_data: Option<CachedImageData>,
    // If true, do not enqueue a network request on cache miss.
    skip_request: bool,
    // Text rendered below the poster by the shader
    title: Option<String>,
    meta: Option<String>,
    // Carousel context for unique poster instance identification
    carousel_key: Option<CarouselKey>,
    // Animation configuration snapshot used for config-aware bounds + animations.
    // Keeping this as a value avoids lifetime plumbing and allows per-widget overrides.
    animation_config: Option<AnimationConfig>,
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
        use crate::infra::constants::layout::poster;
        Self {
            media_id,
            iid: None,
            size: ImageSize::poster(),
            radius: poster::CORNER_RADIUS,
            width: Length::Fixed(poster::BASE_WIDTH),
            height: Length::Fixed(poster::BASE_HEIGHT),
            // Might want to use a different default icon
            placeholder_icon: Icon::FileArchive,
            placeholder_text: None,
            priority: Priority::Preload,
            // Default: flip on first display, fade on subsequent displays
            animation: AnimationBehavior::fade_slow_then_quick(),
            theme_color: Some(fallback_theme_color_for(media_id)),
            is_hovered: false,
            on_play: None,
            on_click: None,
            progress: None,
            progress_color: None,
            rotation_y: None,
            face: None,
            cached_data: None,
            skip_request: false,
            title: None,
            meta: None,
            carousel_key: None,
            animation_config: None,
        }
    }

    /// Set the TMDB image variant id to load.
    pub fn iid(mut self, iid: Option<Uuid>) -> Self {
        self.iid = iid;
        self
    }

    /// Set the image size/type to load
    pub fn size(mut self, size: ImageSize) -> Self {
        // Update dimensions based on size
        if let Some((w, h)) = size.dimensions() {
            self.width = Length::Fixed(w as f32);
            self.height = Length::Fixed(h as f32);
        }
        self.size = size;
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

    /// Set priority based on visibility (convenience method)
    pub fn visible(mut self, is_visible: bool) -> Self {
        self.priority = if is_visible {
            Priority::Visible
        } else {
            Priority::Preload
        };
        self
    }

    /// Set a custom animation behavior.
    pub fn animation_behavior(mut self, behavior: AnimationBehavior) -> Self {
        self.animation = behavior;
        self
    }

    /// Set the default fade animation using explicit config from RuntimeConfig.
    /// This is the preferred method when animation timing is user-configurable.
    /// Also stores the config for config-aware bounds calculation.
    pub fn with_animation_config(mut self, config: &AnimationConfig) -> Self {
        self.animation = AnimationBehavior::fade_slow_then_quick_with(config);
        self.animation_config = Some(*config);
        self
    }

    /// Disable animation
    pub fn no_animation(mut self) -> Self {
        self.animation = AnimationBehavior::constant(PosterAnimationType::None);
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
    pub fn on_play(mut self, message: UiMessage) -> Self {
        self.on_play = Some(message);
        self
    }

    /// Set the click callback (for empty space)
    pub fn on_click(mut self, message: UiMessage) -> Self {
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

    /// Override rotation_y for custom flip control
    pub fn rotation_y(mut self, rotation: f32) -> Self {
        self.rotation_y = Some(rotation);
        self
    }

    /// Set which face/provider to render
    pub fn face(mut self, face: PosterFace) -> Self {
        self.face = Some(face);
        self
    }

    /// If set, the image widget will not enqueue a fetch on cache miss and
    /// will render only a placeholder. Useful when metadata lacks a poster.
    pub fn skip_request(mut self, skip: bool) -> Self {
        self.skip_request = skip;
        self
    }

    /// Set the title text to render below the poster (max 24 chars)
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the meta text (year, rating, etc.) to render below the title (max 16 chars)
    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    /// Set the carousel context for unique poster instance identification.
    /// This is used to differentiate the same media appearing in multiple carousels.
    pub fn carousel_key(mut self, key: CarouselKey) -> Self {
        self.carousel_key = Some(key);
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
    static CACHED_IMAGE_SERVICE: std::cell::RefCell<Option<Arc<UnifiedImageService>>> = const { std::cell::RefCell::new(None) };
}

// Track which requests have already emitted a planner coverage warning to avoid spamming logs
static PLANNER_WARNED: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<u64>>,
> = once_cell::sync::Lazy::new(|| {
    std::sync::Mutex::new(std::collections::HashSet::new())
});

impl<'a> From<ImageFor> for Element<'a, UiMessage> {
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
        // Use config-aware bounds if an explicit AnimationConfig was set via with_animation_config
        let bounds = if let Some(config) = image.animation_config {
            AnimatedPosterBounds::new_with_config(width, height, &config)
        } else {
            AnimatedPosterBounds::new(width, height)
        };

        let request = image.iid.map(|iid| {
            ImageRequest::new(iid, image.size).with_priority(image.priority)
        });

        // Calculate instance hash for cache invalidation and widget identity.
        // Includes carousel_key so the same media in different carousels gets
        // unique primitive IDs for correct batch deduplication.
        let instance_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            image.media_id.hash(&mut hasher);
            image.iid.hash(&mut hasher);
            image.size.hash(&mut hasher);
            // Include carousel context for unique instance identity
            image.carousel_key.hash(&mut hasher);
            hasher.finish()
        };

        // Fast path: Check if we have cached data and it's still valid
        if let Some(cached) = &image.cached_data
            && cached.instance_hash == instance_hash
        {
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
                instance_hash,
                cached.loaded_at,
                image.animation,
                &image,
                bounds,
            );
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
            let Some(request) = request.clone() else {
                // No iid available; render placeholder only.
                let instance_key = PosterInstanceKey::new(
                    image.media_id,
                    image.carousel_key.clone(),
                );
                return create_loading_placeholder(
                    bounds,
                    image.radius,
                    image.theme_color,
                    instance_hash,
                    instance_key,
                    image.face.unwrap_or(PosterFace::Front),
                    image.rotation_y,
                );
            };

            // Check the cache first
            match image_service.take_loaded_entry(&request) {
                Some((handle, loaded_at, hint)) => {
                    #[cfg(any(
                        feature = "profile-with-puffin",
                        feature = "profile-with-tracy",
                        feature = "profile-with-tracing"
                    ))]
                    profiling::scope!("image_for::CacheHit");

                    image.cached_data = Some(CachedImageData {
                        handle: handle.clone(),
                        loaded_at,
                        instance_hash,
                    });

                    let animation_behavior = match hint {
                        Some(FirstDisplayHint::FlipOnce) => {
                            AnimationBehavior::flip_then_fade()
                        }
                        Some(FirstDisplayHint::FastThenSlow) => {
                            AnimationBehavior::fade_slow_then_quick()
                        }
                        None => image.animation,
                    };

                    let instance_key = PosterInstanceKey::new(
                        image.media_id,
                        image.carousel_key.clone(),
                    );

                    let mut shader: Poster =
                        poster(handle, Some(instance_hash))
                            .radius(image.radius)
                            .with_animated_bounds(bounds)
                            .is_hovered(image.is_hovered)
                            .menu_target(instance_key)
                            .face(image.face.unwrap_or(PosterFace::Front));

                    if let Some(color) = image.theme_color {
                        shader = shader.theme_color(color);
                    }

                    if let Some(play_msg) = image.on_play.clone() {
                        shader = shader.on_play(play_msg);
                    }

                    if let Some(click_msg) = image.on_click.clone() {
                        shader = shader.on_click(click_msg);
                    }

                    if let Some(rot) = image.rotation_y {
                        shader = shader.rotation_y(rot);
                    }

                    // Add a tiny random jitter to animation selection so rows don't animate in lockstep.
                    // We intentionally do NOT set an explicit load_time on the shader here; the
                    // batched renderer will start the animation when the texture is actually in the atlas.
                    let selected_animation = match loaded_at {
                        Some(load_time) => {
                            let jitter_ms: u64 = (random::<u8>() as u64) % 21; // 0-20ms
                            let jittered = load_time
                                + std::time::Duration::from_millis(jitter_ms);
                            animation_behavior.select(Some(jittered))
                        }
                        // If we lack a timestamp, use the repeat animation (configured fade)
                        None => animation_behavior.select(None),
                    };

                    shader = shader.with_animation(selected_animation);

                    // Set progress indicator if provided
                    if let Some(progress) = image.progress {
                        shader = shader.progress(progress);
                        shader = shader.progress_color(accent());
                    }

                    // Set title/meta text for shader rendering
                    if let Some(title) = &image.title {
                        shader = shader.title(title.clone());
                    }
                    if let Some(meta) = &image.meta {
                        shader = shader.meta(meta.clone());
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

                    if !image.skip_request {
                        image_service.request_image(request.clone());
                    } else {
                        // Hardening: warn if no planner snapshot appears to cover this request shortly
                        // We check again after a short delay to reduce false positives during transitions
                        let maybe_handle =
                            service_registry::get_image_service();
                        if let Some(svc) = maybe_handle {
                            // Only schedule if not already loaded/loading/queued
                            if !svc.is_loaded(&request)
                                && !svc.is_loading(&request)
                                && !svc.is_queued(&request)
                            {
                                let req = request.clone();
                                let req_hash = instance_hash;
                                // schedule delayed check
                                std::thread::spawn(move || {
                                    std::thread::sleep(
                                        std::time::Duration::from_millis(300),
                                    );
                                    if let Some(svc) =
                                        service_registry::get_image_service()
                                        && !svc.is_loaded(&req)
                                        && !svc.is_loading(&req)
                                        && !svc.is_queued(&req)
                                    {
                                        // Deduplicate warnings per request
                                        if let Ok(mut set) =
                                            PLANNER_WARNED.lock()
                                            && set.insert(req_hash)
                                        {
                                            log::warn!(
                                                "ImageFor(skip_request=true): no planner snapshot detected for iid={:?} size={:?}. Did the view emit DemandSnapshot?",
                                                req.iid,
                                                req.size
                                            );
                                        }
                                    }
                                });
                            }
                        }
                    }

                    let instance_key = PosterInstanceKey::new(
                        image.media_id,
                        image.carousel_key.clone(),
                    );
                    create_loading_placeholder(
                        bounds,
                        image.radius,
                        image.theme_color,
                        instance_hash,
                        instance_key,
                        image.face.unwrap_or(PosterFace::Front),
                        image.rotation_y,
                    )
                }
            }
        } else {
            // Service not initialized, show loading state
            let instance_key = PosterInstanceKey::new(
                image.media_id,
                image.carousel_key.clone(),
            );
            create_loading_placeholder(
                bounds,
                image.radius,
                image.theme_color,
                instance_hash,
                instance_key,
                image.face.unwrap_or(PosterFace::Front),
                image.rotation_y,
            )
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
    instance_hash: u64,
    loaded_at: Option<std::time::Instant>,
    animation: AnimationBehavior,
    image: &ImageFor,
    bounds: AnimatedPosterBounds,
) -> Element<'a, UiMessage> {
    // Create instance key from media_id and carousel_key
    let instance_key =
        PosterInstanceKey::new(image.media_id, image.carousel_key.clone());
    // Check if we should skip atlas upload for VeryFast scrolling
    let mut shader = poster(handle, Some(instance_hash))
        .radius(image.radius)
        .with_animated_bounds(bounds)
        .is_hovered(image.is_hovered)
        .menu_target(instance_key)
        .face(image.face.unwrap_or(PosterFace::Front));

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
    if let Some(rot) = image.rotation_y {
        shader = shader.rotation_y(rot);
    }

    // Apply load-time aware animation selection, but defer actual animation start
    // to the batched renderer (GPU upload time).
    let selected_animation = match loaded_at {
        Some(load_time) => {
            let jitter_ms: u64 = (random::<u8>() as u64) % 21; // 0-20ms
            let jittered =
                load_time + std::time::Duration::from_millis(jitter_ms);
            animation.select(Some(jittered))
        }
        // If we lack a timestamp, use the repeat animation (configured fade)
        None => animation.select(None),
    };

    shader = shader.with_animation(selected_animation);

    // Set progress indicator if provided
    if let Some(progress) = image.progress {
        shader = shader.progress(progress);
        shader = shader.progress_color(accent());
    }

    // Set title/meta text for shader rendering
    if let Some(title) = &image.title {
        shader = shader.title(title.clone());
    }
    if let Some(meta) = &image.meta {
        shader = shader.meta(meta.clone());
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
    instance_hash: u64,
    instance_key: PosterInstanceKey,
    face: PosterFace,
    rotation_override: Option<f32>,
) -> Element<'a, UiMessage> {
    // Create a placeholder handle - we'll use a 1x1 transparent pixel
    // The shader will render the theme color on the backface
    let placeholder_handle = Handle::from_rgba(1, 1, vec![0, 0, 0, 0]);

    // Use theme color or default
    let color =
        theme_color.unwrap_or_else(|| Color::from_rgb(0.15, 0.15, 0.15));

    //log::debug!("Creating placeholder shader with color: {:?}", color);

    // Create shader widget in initial sunken state
    // The PlaceholderSunken animation type will show backface with theme color
    // and apply sunken depth effect
    // Use the request hash so the placeholder shares identity with the texture once it loads.
    let mut poster = poster(placeholder_handle, Some(instance_hash))
        .radius(radius)
        .with_animated_bounds(bounds)
        .theme_color(color)
        .with_animation(PosterAnimationType::PlaceholderSunken)
        .is_hovered(false) // Placeholders are never hovered
        .menu_target(instance_key)
        .face(face);

    if let Some(rot) = rotation_override {
        poster = poster.rotation_y(rot);
    }

    poster.into()
}

/// Extension trait for creating image widgets from media references
pub trait ImageForExt {
    fn image_for(&self) -> ImageFor;
}

impl ImageForExt for MovieReference {
    fn image_for(&self) -> ImageFor {
        // Temporary diagnostics: register a human-readable title for media_id
        crate::infra::image_log::register_media_title(
            self.id.to_uuid(),
            self.title.as_ref(),
        );
        image_for(self.id.to_uuid())
            .iid(self.details.primary_poster_iid)
            .skip_request(self.details.primary_poster_iid.is_none())
            .placeholder(Icon::Film)
    }
}

impl ImageForExt for Series {
    fn image_for(&self) -> ImageFor {
        // Temporary diagnostics: register a human-readable title for media_id
        register_media_title(self.id.to_uuid(), self.title.as_ref());
        image_for(self.id.to_uuid())
            .iid(self.details.primary_poster_iid)
            .skip_request(self.details.primary_poster_iid.is_none())
            .placeholder(Icon::Tv)
    }
}

impl ImageForExt for SeasonReference {
    fn image_for(&self) -> ImageFor {
        // Temporary diagnostics: register a simple season label
        crate::infra::image_log::register_media_title(
            self.id.to_uuid(),
            &format!("Season {}", self.season_number.value()),
        );
        image_for(self.id.to_uuid())
            .iid(self.details.primary_poster_iid)
            .skip_request(self.details.primary_poster_iid.is_none())
            .placeholder(Icon::Tv)
    }
}

impl ImageForExt for EpisodeReference {
    fn image_for(&self) -> ImageFor {
        // Optional: register episode label (not typically used for posters)
        crate::infra::image_log::register_media_title(
            self.id.to_uuid(),
            &format!(
                "S{}E{}",
                self.season_number.value(),
                self.episode_number.value()
            ),
        );
        image_for(self.id.to_uuid())
            .iid(self.details.primary_still_iid)
            .skip_request(self.details.primary_still_iid.is_none())
            .size(ImageSize::thumbnail())
            .placeholder(Icon::FileImage)
    }
}
