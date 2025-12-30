use std::hash::{Hash, Hasher};

use uuid::Uuid;

use crate::image::ImageSize;

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
    /// `tmdb_image_variants.id` (UUID) for the selected image.
    ///
    /// This is the canonical identifier for images across server and player.
    pub iid: Uuid,
    pub size: ImageSize,
    pub priority: Priority,
}

impl Hash for ImageRequest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.iid.hash(state);
        self.size.hash(state);
    }
}

impl PartialEq for ImageRequest {
    fn eq(&self, other: &Self) -> bool {
        self.iid == other.iid && self.size == other.size
    }
}

impl Eq for ImageRequest {}

impl ImageRequest {
    /// Create a new image request for a specific TMDB image variant id.
    pub fn new(iid: Uuid, size: ImageSize) -> Self {
        Self {
            iid,
            size,
            priority: Priority::Visible,
        }
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageRequest, ImageSize, Priority};
    use crate::image::{BackdropSize, PosterSize};
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
        let iid = Uuid::now_v7();
        let base = ImageRequest::new(iid, ImageSize::Poster(PosterSize::W185));
        let visible = base.clone().with_priority(Priority::Visible);
        let preload = base.clone().with_priority(Priority::Preload);

        assert_eq!(visible, preload);
        assert_eq!(hash_of(&visible), hash_of(&preload));
    }

    #[test]
    fn requests_hash_by_iid_and_size() {
        let iid = Uuid::now_v7();
        let poster =
            ImageRequest::new(iid, ImageSize::Poster(PosterSize::W185));
        let backdrop =
            ImageRequest::new(iid, ImageSize::Backdrop(BackdropSize::W780));

        assert_ne!(hash_of(&poster), hash_of(&backdrop));
        assert!(matches!(poster.size, ImageSize::Poster(PosterSize::W185)));
    }
}
