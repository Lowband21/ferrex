use std::hash::{Hash, Hasher};

use uuid::Uuid;

use crate::{ImageSize, ImageType};

/// Domain-specific categories for poster imagery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosterKind {
    Movie,
    Series,
    Season,
}

impl PosterKind {
    const fn image_type(self) -> ImageType {
        match self {
            PosterKind::Movie => ImageType::Movie,
            PosterKind::Series => ImageType::Series,
            PosterKind::Season => ImageType::Season,
        }
    }
}

/// Available logical poster sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PosterSize {
    /// Small thumbnail poster (maps to `ImageSize::Thumbnail`).
    Thumb,
    /// Default poster resolution (maps to `ImageSize::Poster`).
    #[default]
    Standard,
    /// Higher-quality poster that prefers richer variants while staying cache friendly.
    Quality,
    /// Hero poster used in detail views (maps to `ImageSize::Full`, w500).
    Original,
}

impl PosterSize {
    const fn as_image_size(self) -> ImageSize {
        match self {
            PosterSize::Thumb => ImageSize::Thumbnail,
            PosterSize::Standard => ImageSize::Poster,
            PosterSize::Quality => ImageSize::Poster,
            PosterSize::Original => ImageSize::Full,
        }
    }
}

/// Domain-specific categories for backdrop imagery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackdropKind {
    Movie,
    Series,
    Season,
}

impl BackdropKind {
    const fn image_type(self) -> ImageType {
        match self {
            BackdropKind::Movie => ImageType::Movie,
            BackdropKind::Series => ImageType::Series,
            BackdropKind::Season => ImageType::Season,
        }
    }
}

/// Logical backdrop size (currently a single option to encourage type safety).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BackdropSize {
    #[default]
    Quality,
}

impl BackdropSize {
    const fn as_image_size(self) -> ImageSize {
        match self {
            BackdropSize::Quality => ImageSize::Backdrop,
        }
    }
}

/// Logical profile image size for cast/people portraits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProfileSize {
    #[default]
    Standard,
    Any,
}

impl ProfileSize {
    const fn as_image_size(self) -> ImageSize {
        match self {
            ProfileSize::Standard | ProfileSize::Any => ImageSize::Profile,
        }
    }
}

/// Logical size for episode still images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EpisodeStillSize {
    #[default]
    Standard,
}

impl EpisodeStillSize {
    const fn as_image_size(self) -> ImageSize {
        match self {
            EpisodeStillSize::Standard => ImageSize::Thumbnail,
        }
    }
}

/// Priority hint for unified image loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Image should be fetched immediately because it is on-screen.
    Visible,
    /// Image will become visible soon; preload with elevated priority.
    Preload,
    /// Low-priority background prefetch.
    Background,
}

impl Priority {
    /// Convert the priority to a queue weight (higher is more urgent).
    pub fn weight(&self) -> u8 {
        match self {
            Priority::Visible => 3,
            Priority::Preload => 2,
            Priority::Background => 1,
        }
    }
}

/// Unified image request shared between the player and server components.
///
/// Equality and hashing deliberately ignore `priority`. The cache uses
/// `ImageRequest` as its key and should treat requests for the same media
/// (including alternate indices) as identical regardless of urgency.
#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub media_id: Uuid,
    pub size: ImageSize,
    pub image_type: ImageType,
    pub priority: Priority,
    pub image_index: u32,
}

impl Hash for ImageRequest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.media_id.hash(state);
        self.size.hash(state);
        self.image_type.hash(state);
        self.image_index.hash(state);
    }
}

impl PartialEq for ImageRequest {
    fn eq(&self, other: &Self) -> bool {
        self.media_id == other.media_id
            && self.size == other.size
            && self.image_type == other.image_type
            && self.image_index == other.image_index
    }
}

impl Eq for ImageRequest {}

impl ImageRequest {
    #[track_caller]
    pub fn new(media_id: Uuid, size: ImageSize, image_type: ImageType) -> Self {
        assert!(
            is_valid_combination(image_type, size),
            "Invalid image size {:?} for image type {:?}",
            size,
            image_type
        );
        Self {
            media_id,
            size,
            image_type,
            priority: Priority::Visible,
            image_index: 0,
        }
    }

    /// Construct a poster request using typed variants.
    pub fn poster(media_id: Uuid, kind: PosterKind, size: PosterSize) -> Self {
        Self::new(media_id, size.as_image_size(), kind.image_type())
    }

    /// Construct a backdrop request using typed variants.
    pub fn backdrop(media_id: Uuid, kind: BackdropKind, size: BackdropSize) -> Self {
        Self::new(media_id, size.as_image_size(), kind.image_type())
    }

    /// Construct an episode still request (2:1 still/thumbnail imagery).
    pub fn episode_still(media_id: Uuid, size: EpisodeStillSize) -> Self {
        Self::new(media_id, size.as_image_size(), ImageType::Episode)
    }

    /// Construct a person profile request.
    pub fn person_profile(media_id: Uuid, size: ProfileSize) -> Self {
        Self::new(media_id, size.as_image_size(), ImageType::Person)
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_index(mut self, index: u32) -> Self {
        self.image_index = index;
        self
    }
}

const fn is_valid_combination(image_type: ImageType, size: ImageSize) -> bool {
    use ImageSize::*;
    use ImageType::*;

    match image_type {
        Movie | Series | Season => matches!(size, Thumbnail | Poster | Backdrop | Full),
        Episode => matches!(size, Thumbnail),
        Person => matches!(size, Profile),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackdropKind, BackdropSize, EpisodeStillSize, ImageRequest, ImageSize, ImageType,
        PosterKind, PosterSize, Priority, ProfileSize,
    };
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use uuid::Uuid;

    fn hash_of(request: &ImageRequest) -> u64 {
        let mut hasher = DefaultHasher::new();
        request.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn requests_ignore_priority_for_identity() {
        let media_id = Uuid::now_v7();
        let base = ImageRequest::new(media_id, ImageSize::Poster, ImageType::Movie);
        let visible = base.clone().with_priority(Priority::Visible);
        let preload = base.clone().with_priority(Priority::Preload);

        assert_eq!(visible, preload);
        assert_eq!(hash_of(&visible), hash_of(&preload));
    }

    #[test]
    fn image_index_contributes_to_identity() {
        let media_id = Uuid::now_v7();
        let base = ImageRequest::new(media_id, ImageSize::Poster, ImageType::Movie);
        let first = base.clone().with_index(0);
        let second = base.clone().with_index(1);

        assert_ne!(first, second);
        assert_ne!(hash_of(&first), hash_of(&second));
    }

    #[test]
    #[should_panic(expected = "Invalid image size")]
    fn invalid_combinations_panic() {
        let media_id = Uuid::now_v7();
        // Movie media cannot request a profile-sized image.
        let _ = ImageRequest::new(media_id, ImageSize::Profile, ImageType::Movie);
    }

    #[test]
    fn typed_constructors_produce_expected_mappings() {
        let media_id = Uuid::now_v7();

        let poster = ImageRequest::poster(media_id, PosterKind::Movie, PosterSize::Original);
        assert_eq!(poster.image_type, ImageType::Movie);
        assert_eq!(poster.size, ImageSize::Full);

        let quality_poster = ImageRequest::poster(media_id, PosterKind::Movie, PosterSize::Quality);
        assert_eq!(quality_poster.image_type, ImageType::Movie);
        assert_eq!(quality_poster.size, ImageSize::Poster);

        let backdrop =
            ImageRequest::backdrop(media_id, BackdropKind::Series, BackdropSize::Quality);
        assert_eq!(backdrop.image_type, ImageType::Series);
        assert_eq!(backdrop.size, ImageSize::Backdrop);

        let still = ImageRequest::episode_still(media_id, EpisodeStillSize::Standard);
        assert_eq!(still.image_type, ImageType::Episode);
        assert_eq!(still.size, ImageSize::Thumbnail);

        let profile = ImageRequest::person_profile(media_id, ProfileSize::Standard);
        assert_eq!(profile.image_type, ImageType::Person);
        assert_eq!(profile.size, ImageSize::Profile);

        let any_profile = ImageRequest::person_profile(media_id, ProfileSize::Any);
        assert_eq!(any_profile.image_type, ImageType::Person);
        assert_eq!(any_profile.size, ImageSize::Profile);
    }
}
