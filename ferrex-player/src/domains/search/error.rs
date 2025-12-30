use thiserror::Error;

use crate::infra::repository::RepositoryError;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Player repo error resulting in failed search: {0}")]
    Repo(#[from] RepositoryError),
    #[error("Rkyv deserialization error resulting in failed search: {0}")]
    Rkyv(#[from] rkyv::rancor::Error),
    #[error("Player error resulting in failed search: {0}")]
    Internal(String),
    #[error("Server error resulting in failed search: {0}")]
    Server(String),
    #[error("No matches found: {0}")]
    NotFound(String),
}
