#[cfg(feature = "database")]
pub mod database;
pub mod error;
pub mod extras_parser;
pub mod library;
pub mod media;
#[cfg(feature = "ffmpeg")]
pub mod metadata;
pub mod providers;
#[cfg(feature = "database")]
pub mod scanner;
#[cfg(feature = "database")]
pub mod streaming_scanner;
pub mod tv_parser;
pub mod types;

#[cfg(feature = "database")]
pub use database::*;
pub use error::*;
pub use extras_parser::ExtrasParser;
pub use library::*;
pub use media::*;
#[cfg(feature = "ffmpeg")]
pub use metadata::*;
pub use providers::{MetadataProvider, ProviderError, TmdbProvider};
#[cfg(feature = "database")]
pub use scanner::*;
#[cfg(feature = "database")]
pub use streaming_scanner::*;
pub use tv_parser::{TvParser, EpisodeInfo};
pub use types::{TranscodingJobResponse, TranscodingStatus, TranscodingProgressDetails};
