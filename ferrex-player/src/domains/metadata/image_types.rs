use ferrex_core::api_types::MediaId;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageSize {
    Thumbnail, // Small size for grids
    Poster,    // Standard poster size
    Backdrop,  // Wide backdrop/banner
    Full,      // Original size
    Profile,   // Person profile image (2:3 aspect ratio)
}

impl ImageSize {
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            ImageSize::Thumbnail => (150.0, 225.0),
            ImageSize::Poster => (200.0, 300.0),
            ImageSize::Backdrop => (1920.0, 1080.0), // Full HD backdrop
            ImageSize::Full => (0.0, 0.0),           // Dynamic
            ImageSize::Profile => (120.0, 180.0),    // 2:3 aspect ratio for cast
        }
    }

    pub fn suffix(&self) -> &str {
        match self {
            ImageSize::Thumbnail => "_thumb",
            ImageSize::Poster => "_poster",
            ImageSize::Backdrop => "_backdrop",
            ImageSize::Full => "",
            ImageSize::Profile => "_profile",
        }
    }
}

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
    pub media_id: MediaId,
    pub size: ImageSize,
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
    pub fn new(media_id: MediaId, size: ImageSize) -> Self {
        Self {
            media_id,
            size,
            priority: Priority::Visible,
        }
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn cache_key(&self) -> String {
        format!(
            "{}:{}",
            media_id_to_cache_key(&self.media_id),
            self.size.suffix()
        )
    }
}

// Helper functions for MediaId
pub fn media_id_to_cache_key(media_id: &MediaId) -> String {
    match media_id {
        MediaId::Movie(id) => format!("movie:{}", id.as_str()),
        MediaId::Series(id) => format!("series:{}", id.as_str()),
        MediaId::Season(id) => format!("season:{}", id.as_str()),
        MediaId::Episode(id) => format!("episode:{}", id.as_str()),
        MediaId::Person(id) => format!("person:{}", id.as_str()),
    }
}

pub fn media_id_type(media_id: &MediaId) -> &str {
    match media_id {
        MediaId::Movie(_) => "movie",
        MediaId::Series(_) => "series",
        MediaId::Season(_) => "season",
        MediaId::Episode(_) => "episode",
        MediaId::Person(_) => "person",
    }
}
