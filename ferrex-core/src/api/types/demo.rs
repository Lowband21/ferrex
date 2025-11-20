use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::types::{ids::LibraryID, library::LibraryType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoStatus {
    pub root: PathBuf,
    pub libraries: Vec<DemoLibraryStatus>,
    pub username: String,
}

impl DemoStatus {
    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoLibraryStatus {
    pub library_id: LibraryID,
    pub name: String,
    pub library_type: LibraryType,
    pub root: PathBuf,
    /// Number of primary items (movies or series) planned for this library
    pub primary_item_count: usize,
    pub file_count: usize,
    pub directory_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemoResetRequest {
    pub movie_count: Option<usize>,
    pub series_count: Option<usize>,
}

impl DemoResetRequest {
    pub fn is_empty(&self) -> bool {
        self.movie_count.is_none() && self.series_count.is_none()
    }
}
