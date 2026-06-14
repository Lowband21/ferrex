use uuid::Uuid;

use crate::ImageSize;

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ImageFetchSource {
    /// Remote TMDB asset fetched by a path fragment.
    Tmdb { tmdb_path: String, imz: ImageSize },
    /// Locally generated episode thumbnail sourced from a media file.
    EpisodeThumbnail {
        path_key: String,
        media_file_id: Uuid,
        imz: ImageSize,
    },
}
