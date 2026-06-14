use std::fmt::{self, Display};

/// Errors produced by model constructors and validation routines.
#[derive(Debug)]
pub enum ModelError {
    Io(std::io::Error),
    InvalidMedia(String),
}

impl Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Io(err) => write!(f, "io error: {err}"),
            ModelError::InvalidMedia(msg) => write!(f, "invalid media: {msg}"),
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ModelError::Io(err) => Some(err),
            ModelError::InvalidMedia(_) => None,
        }
    }
}

impl From<std::io::Error> for ModelError {
    fn from(err: std::io::Error) -> Self {
        ModelError::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, ModelError>;
