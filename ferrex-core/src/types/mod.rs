// Media module - organizes all media-related types and traits

pub mod details;
pub mod files;
pub mod ids;
pub mod image;
pub mod library;
pub mod media;
pub mod media_id;
pub mod numbers;
pub mod titles;
pub mod transcoding;
pub mod urls;
pub mod util_types;
pub mod scan;

// Re-export everything from submodules
pub use details::*;
pub use files::*;
pub use ids::*;
pub use image::*;
pub use library::*;
pub use media::*;
pub use media_id::*;
pub use numbers::*;
pub use titles::*;
pub use transcoding::*;
pub use urls::*;
pub use util_types::*;
pub use scan::*;
