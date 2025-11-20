use ferrex_core::{ImageSize, ImageType};
use std::hash::{Hash, Hasher};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Visible,    // Currently visible on screen
    Preload,    // About to be visible (overscan)
    Background, // Prefetch for future
}

impl Priority {
    pub fn weight(&self) -> u8 {
        match self {
            Priority::Visible => 3,
            Priority::Preload => 2,
            Priority::Background => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub media_id: Uuid,
    pub size: ImageSize,
    pub image_type: ImageType,
    pub priority: Priority,
}

impl Hash for ImageRequest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.media_id.hash(state);
        self.size.hash(state);
    }
}

impl PartialEq for ImageRequest {
    fn eq(&self, other: &Self) -> bool {
        self.media_id == other.media_id && self.size == other.size
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
        }
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }
}
