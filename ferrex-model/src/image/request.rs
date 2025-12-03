use std::hash::{Hash, Hasher};

use uuid::Uuid;

use crate::{
    MediaType,
    image::{
        EpisodeSize, ImageSize, PosterSize, ProfileSize, sizes::BackdropSize,
    },
};

/// Domain-specific categories for poster imagery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PosterKind {
    Movie,
    Series,
    Season,
}

impl PosterKind {
    pub const fn image_type(self) -> MediaType {
        match self {
            PosterKind::Movie => MediaType::Movie,
            PosterKind::Series => MediaType::Series,
            PosterKind::Season => MediaType::Season,
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
    pub const fn image_type(self) -> MediaType {
        match self {
            BackdropKind::Movie => MediaType::Movie,
            BackdropKind::Series => MediaType::Series,
            BackdropKind::Season => MediaType::Season,
        }
    }
}

/// Logical size for episode still images (maps to EpisodeSize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EpisodeStillSize {
    #[default]
    Standard,
}

impl EpisodeStillSize {
    pub const fn as_episode_size(self) -> EpisodeSize {
        match self {
            EpisodeStillSize::Standard => EpisodeSize::W512,
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
    pub image_type: MediaType,
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
    /// Create a new image request with the given size and type.
    pub fn new(media_id: Uuid, size: ImageSize, image_type: MediaType) -> Self {
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
        Self::new(media_id, ImageSize::Poster(size), kind.image_type())
    }

    /// Construct a backdrop request using typed variants.
    pub fn backdrop(
        media_id: Uuid,
        kind: BackdropKind,
        size: BackdropSize,
    ) -> Self {
        Self::new(media_id, ImageSize::Backdrop(size), kind.image_type())
    }

    /// Construct an episode still request (16:9 still/thumbnail imagery).
    pub fn episode_still(media_id: Uuid, size: EpisodeStillSize) -> Self {
        Self::new(
            media_id,
            ImageSize::Thumbnail(size.as_episode_size()),
            MediaType::Episode,
        )
    }

    /// Construct a person profile request.
    pub fn person_profile(media_id: Uuid, size: ProfileSize) -> Self {
        Self::new(media_id, ImageSize::Profile(size), MediaType::Person)
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

#[cfg(test)]
mod tests {
    use super::{
        BackdropKind, BackdropSize, EpisodeStillSize, ImageRequest, ImageSize,
        MediaType, PosterKind, PosterSize, Priority, ProfileSize,
    };
    use crate::image::EpisodeSize;
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
        let base = ImageRequest::new(
            media_id,
            ImageSize::Poster(PosterSize::W300),
            MediaType::Movie,
        );
        let visible = base.clone().with_priority(Priority::Visible);
        let preload = base.clone().with_priority(Priority::Preload);

        assert_eq!(visible, preload);
        assert_eq!(hash_of(&visible), hash_of(&preload));
    }

    #[test]
    fn image_index_contributes_to_identity() {
        let media_id = Uuid::now_v7();
        let base = ImageRequest::new(
            media_id,
            ImageSize::Poster(PosterSize::W300),
            MediaType::Movie,
        );
        let first = base.clone().with_index(0);
        let second = base.clone().with_index(1);

        assert_ne!(first, second);
        assert_ne!(hash_of(&first), hash_of(&second));
    }

    #[test]
    fn typed_constructors_produce_expected_mappings() {
        let media_id = Uuid::now_v7();

        // Poster with Original size
        let poster = ImageRequest::poster(
            media_id,
            PosterKind::Movie,
            PosterSize::Original(None),
        );
        assert_eq!(poster.image_type, MediaType::Movie);
        assert!(matches!(
            poster.size,
            ImageSize::Poster(PosterSize::Original(None))
        ));

        // Poster with W300 size
        let standard_poster =
            ImageRequest::poster(media_id, PosterKind::Movie, PosterSize::W300);
        assert_eq!(standard_poster.image_type, MediaType::Movie);
        assert!(matches!(
            standard_poster.size,
            ImageSize::Poster(PosterSize::W300)
        ));

        // Poster with W600 size
        let quality_poster =
            ImageRequest::poster(media_id, PosterKind::Movie, PosterSize::W600);
        assert_eq!(quality_poster.image_type, MediaType::Movie);
        assert!(matches!(
            quality_poster.size,
            ImageSize::Poster(PosterSize::W600)
        ));

        let backdrop = ImageRequest::backdrop(
            media_id,
            BackdropKind::Series,
            BackdropSize::W3840,
        );
        assert_eq!(backdrop.image_type, MediaType::Series);
        assert!(matches!(
            backdrop.size,
            ImageSize::Backdrop(BackdropSize::W3840)
        ));

        let still =
            ImageRequest::episode_still(media_id, EpisodeStillSize::Standard);
        assert_eq!(still.image_type, MediaType::Episode);
        assert!(matches!(
            still.size,
            ImageSize::Thumbnail(EpisodeSize::W512)
        ));

        let profile = ImageRequest::person_profile(media_id, ProfileSize::W180);
        assert_eq!(profile.image_type, MediaType::Person);
        assert!(matches!(
            profile.size,
            ImageSize::Profile(ProfileSize::W180)
        ));
    }
}
