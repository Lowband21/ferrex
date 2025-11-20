use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize, option::ArchivedOption,
    vec::ArchivedVec,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::types::Media;

use super::{ArchivedLibraryID, ArchivedMedia, ArchivedMovieReference, LibraryID};

/// Read-only operations for library-like types

pub trait LibraryLike {
    fn needs_scan(&self) -> bool;
    fn get_id(&self) -> LibraryID;
    fn get_name(&self) -> &str;
    fn get_type(&self) -> LibraryType;
    fn get_paths(&self) -> Vec<PathBuf>; // Returns owned for compatibility with archived types
    fn get_scan_interval(&self) -> u32;
    fn get_last_scan(&self) -> Option<chrono::DateTime<chrono::Utc>>;
    fn is_enabled(&self) -> bool;
    fn is_auto_scan(&self) -> bool;
    fn is_watch_for_changes(&self) -> bool;
    fn is_analyze_on_scan(&self) -> bool;
    fn get_max_retry_attempts(&self) -> u32;
    fn get_created_at(&self) -> chrono::DateTime<chrono::Utc>;
    fn get_updated_at(&self) -> chrono::DateTime<chrono::Utc>;
    fn get_media_references_clone(&self) -> Option<Vec<Media>>;
}

/// Mutable operations for library types (only implemented by owned types like Library)
pub trait LibraryLikeMut: LibraryLike {
    fn new(name: String, library_type: LibraryType, paths: Vec<PathBuf>) -> Self;
    fn update_last_scan(&mut self);
    fn set_paths(&mut self, paths: Vec<PathBuf>);
    fn set_scan_interval(&mut self, interval: u32);
    fn set_last_scan(&mut self, last_scan: Option<chrono::DateTime<chrono::Utc>>);
    fn set_auto_scan(&mut self, auto_scan: bool);
    fn set_max_retry_attempts(&mut self, max_retry_attempts: u32);
    fn set_updated_at(&mut self, updated_at: Option<chrono::DateTime<chrono::Utc>>);
    fn set_media_references(&mut self, media_references: Vec<Media>);
}

/// Represents a media library with a specific type
#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct Library {
    pub id: LibraryID,
    pub name: String,
    pub library_type: LibraryType,
    #[rkyv(with = crate::rkyv_wrappers::VecPathBuf)]
    pub paths: Vec<PathBuf>,
    pub scan_interval_minutes: u32,
    #[rkyv(with = crate::rkyv_wrappers::OptionDateTime)]
    pub last_scan: Option<chrono::DateTime<chrono::Utc>>,
    pub enabled: bool,
    pub auto_scan: bool,
    pub watch_for_changes: bool,
    pub analyze_on_scan: bool,
    pub max_retry_attempts: u32,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub media: Option<Vec<Media>>,
}

pub trait ArchivedLibraryExt {
    fn media<'a>(&'a self) -> Option<&'a ArchivedVec<ArchivedMedia>>;
    fn media_as_slice(&self) -> &[ArchivedMedia];
    fn get_movie_refs(&self) -> impl Iterator<Item = &ArchivedMovieReference>;
}

impl ArchivedLibraryExt for ArchivedLibrary {
    fn media<'a>(&'a self) -> Option<&'a ArchivedVec<ArchivedMedia>> {
        match &self.media {
            ArchivedOption::Some(media) => Some(media),
            ArchivedOption::None => None,
        }
    }
    fn media_as_slice(&self) -> &[ArchivedMedia] {
        match &self.media {
            ArchivedOption::Some(media) => media.as_slice(),
            ArchivedOption::None => &[],
        }
    }

    fn get_movie_refs(&self) -> impl Iterator<Item = &ArchivedMovieReference> {
        self.media_as_slice().iter().filter_map(|m| match m {
            ArchivedMedia::Movie(movie) => Some(movie),
            _ => None,
        })
    }
}

/// The type of content a library contains
#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[serde(rename_all = "PascalCase")]
#[rkyv(derive(Debug, Clone, PartialEq, Eq, Hash))]
pub enum LibraryType {
    Movies,
    Series,
}

impl std::fmt::Display for LibraryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryType::Movies => write!(f, "Movies"),
            LibraryType::Series => write!(f, "TV Shows"),
        }
    }
}

impl LibraryLike for Library {
    fn needs_scan(&self) -> bool {
        if !self.enabled {
            return false;
        }

        match self.last_scan {
            None => true,
            Some(last_scan) => {
                let elapsed = chrono::Utc::now().signed_duration_since(last_scan);
                elapsed.num_minutes() >= self.scan_interval_minutes as i64
            }
        }
    }

    fn get_id(&self) -> LibraryID {
        self.id
    }

    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_type(&self) -> LibraryType {
        self.library_type
    }

    fn get_paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    fn get_scan_interval(&self) -> u32 {
        self.scan_interval_minutes
    }

    fn get_last_scan(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.last_scan
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn is_auto_scan(&self) -> bool {
        self.auto_scan
    }

    fn is_watch_for_changes(&self) -> bool {
        self.watch_for_changes
    }

    fn is_analyze_on_scan(&self) -> bool {
        self.analyze_on_scan
    }

    fn get_max_retry_attempts(&self) -> u32 {
        self.max_retry_attempts
    }

    fn get_created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.created_at
    }

    fn get_updated_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.updated_at
    }

    fn get_media_references_clone(&self) -> Option<Vec<Media>> {
        self.media.clone()
    }
}

impl ArchivedLibrary {
    pub fn get_id(&self) -> ArchivedLibraryID {
        self.id
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_type(&self) -> &ArchivedLibraryType {
        &self.library_type
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_auto_scan(&self) -> bool {
        self.auto_scan
    }

    pub fn is_watch_for_changes(&self) -> bool {
        self.watch_for_changes
    }

    pub fn is_analyze_on_scan(&self) -> bool {
        self.analyze_on_scan
    }
}

impl LibraryLikeMut for Library {
    fn new(name: String, library_type: LibraryType, paths: Vec<PathBuf>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: LibraryID::new_uuid(),
            name,
            library_type,
            paths,
            scan_interval_minutes: 60,
            last_scan: None,
            enabled: true,
            auto_scan: true,
            watch_for_changes: true,
            analyze_on_scan: false,
            max_retry_attempts: 3,
            created_at: now,
            updated_at: now,
            media: None,
        }
    }

    fn update_last_scan(&mut self) {
        self.last_scan = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }

    fn set_paths(&mut self, paths: Vec<PathBuf>) {
        self.paths = paths;
        self.updated_at = chrono::Utc::now();
    }

    fn set_scan_interval(&mut self, interval: u32) {
        self.scan_interval_minutes = interval;
        self.updated_at = chrono::Utc::now();
    }

    fn set_last_scan(&mut self, last_scan: Option<chrono::DateTime<chrono::Utc>>) {
        self.last_scan = last_scan;
        self.updated_at = chrono::Utc::now();
    }

    fn set_auto_scan(&mut self, auto_scan: bool) {
        self.auto_scan = auto_scan;
        self.updated_at = chrono::Utc::now();
    }

    fn set_max_retry_attempts(&mut self, max_retry_attempts: u32) {
        self.max_retry_attempts = max_retry_attempts;
        self.updated_at = chrono::Utc::now();
    }

    fn set_updated_at(&mut self, updated_at: Option<chrono::DateTime<chrono::Utc>>) {
        self.updated_at = updated_at.unwrap_or_else(chrono::Utc::now);
    }

    fn set_media_references(&mut self, media_references: Vec<Media>) {
        self.media = Some(media_references);
        self.updated_at = chrono::Utc::now();
    }
}

// Request types moved to api_types.rs to avoid conflicts

/// Library scan result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryScanResult {
    pub library_id: LibraryID,
    pub library_name: String,
    pub total_files: usize,
    pub new_files: usize,
    pub updated_files: usize,
    pub deleted_files: usize,
    pub errors: Vec<String>,
    pub duration_seconds: f64,
}
