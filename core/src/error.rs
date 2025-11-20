use thiserror::Error;

#[derive(Error, Debug)]
pub enum MediaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg_next::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid media file: {0}")]
    InvalidMedia(String),

    #[error("Media not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;
