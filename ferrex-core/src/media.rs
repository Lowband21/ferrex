// Media module - organizes all media-related types and traits

mod ids;
mod titles;
mod urls;
mod numbers;
mod references;
mod traits;
mod details;
mod files;

// Re-export everything from submodules
pub use ids::*;
pub use titles::*;
pub use urls::*;
pub use numbers::*;
pub use references::*;
pub use traits::*;
pub use details::*;
pub use files::*;