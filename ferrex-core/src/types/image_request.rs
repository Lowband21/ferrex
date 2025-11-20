use std::hash::{Hash, Hasher};

use uuid::Uuid;

use crate::{ImageSize, ImageType};

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
    pub fn new(media_id: Uuid, size: ImageSize, image_type: ImageType) -> Self {
        Self {
            media_id,
            size,
            image_type,
            priority: Priority::Visible,
            image_index: 0,
        }
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
    use super::{ImageRequest, ImageSize, ImageType, Priority};
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
        let media_id = Uuid::new_v4();
        let base = ImageRequest::new(media_id, ImageSize::Poster, ImageType::Movie);
        let visible = base.clone().with_priority(Priority::Visible);
        let preload = base.clone().with_priority(Priority::Preload);

        assert_eq!(visible, preload);
        assert_eq!(hash_of(&visible), hash_of(&preload));
    }

    #[test]
    fn image_index_contributes_to_identity() {
        let media_id = Uuid::new_v4();
        let base = ImageRequest::new(media_id, ImageSize::Poster, ImageType::Movie);
        let first = base.clone().with_index(0);
        let second = base.clone().with_index(1);

        assert_ne!(first, second);
        assert_ne!(hash_of(&first), hash_of(&second));
    }
}
