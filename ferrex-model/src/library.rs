use std::path::PathBuf;

use crate::chrono::{DateTime, Utc};
use crate::media::Media;

use super::ids::LibraryId;

/// Read-only operations for library-like types
pub trait LibraryLike {
    fn needs_scan(&self) -> bool;
    fn get_id(&self) -> LibraryId;
    fn get_name(&self) -> &str;
    fn get_type(&self) -> LibraryType;
    fn get_paths(&self) -> Vec<PathBuf>; // Returns owned for compatibility with archived types
    fn get_scan_interval(&self) -> u32;
    fn get_last_scan(&self) -> Option<DateTime<Utc>>;
    fn is_enabled(&self) -> bool;
    fn is_auto_scan(&self) -> bool;
    fn is_watch_for_changes(&self) -> bool;
    fn is_analyze_on_scan(&self) -> bool;
    fn get_max_retry_attempts(&self) -> u32;
    fn get_created_at(&self) -> DateTime<Utc>;
    fn get_updated_at(&self) -> DateTime<Utc>;
    fn get_media_references_clone(&self) -> Option<Vec<Media>>;
}

/// Mutable operations for library types (only implemented by owned types like Library)
pub trait LibraryLikeMut: LibraryLike {
    fn new(
        name: String,
        library_type: LibraryType,
        paths: Vec<PathBuf>,
    ) -> Self;
    fn update_last_scan(&mut self);
    fn set_paths(&mut self, paths: Vec<PathBuf>);
    fn set_scan_interval(&mut self, interval: u32);
    fn set_last_scan(&mut self, last_scan: Option<DateTime<Utc>>);
    fn set_auto_scan(&mut self, auto_scan: bool);
    fn set_max_retry_attempts(&mut self, max_retry_attempts: u32);
    fn set_updated_at(&mut self, updated_at: Option<DateTime<Utc>>);
    fn set_media_references(&mut self, media_references: Vec<Media>);
}

/// Represents a media library with a specific type
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq)))]
pub struct Library {
    pub id: LibraryId,
    pub name: String,
    pub library_type: LibraryType,
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::VecPathBuf))]
    pub paths: Vec<PathBuf>,
    pub scan_interval_minutes: u32,
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::OptionDateTime))]
    pub last_scan: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub auto_scan: bool,
    pub watch_for_changes: bool,
    pub analyze_on_scan: bool,
    pub max_retry_attempts: u32,
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub created_at: DateTime<Utc>,
    #[cfg_attr(feature = "rkyv", rkyv(with = crate::rkyv_wrappers::DateTimeWrapper))]
    pub updated_at: DateTime<Utc>,
    pub media: Option<Vec<Media>>,
}

/// The type of content a library contains
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "serde", serde(rename_all = "PascalCase"))]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, Clone, PartialEq, Eq, Hash)))]
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
                let elapsed = Utc::now().signed_duration_since(last_scan);
                elapsed.num_minutes() >= self.scan_interval_minutes as i64
            }
        }
    }

    fn get_id(&self) -> LibraryId {
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

    fn get_last_scan(&self) -> Option<DateTime<Utc>> {
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

    fn get_created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    fn get_updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn get_media_references_clone(&self) -> Option<Vec<Media>> {
        self.media.clone()
    }
}

impl LibraryLikeMut for Library {
    fn new(
        name: String,
        library_type: LibraryType,
        paths: Vec<PathBuf>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: LibraryId::new(),
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
        self.last_scan = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    fn set_paths(&mut self, paths: Vec<PathBuf>) {
        self.paths = paths;
        self.updated_at = Utc::now();
    }

    fn set_scan_interval(&mut self, interval: u32) {
        self.scan_interval_minutes = interval;
        self.updated_at = Utc::now();
    }

    fn set_last_scan(&mut self, last_scan: Option<DateTime<Utc>>) {
        self.last_scan = last_scan;
        self.updated_at = Utc::now();
    }

    fn set_auto_scan(&mut self, auto_scan: bool) {
        self.auto_scan = auto_scan;
        self.updated_at = Utc::now();
    }

    fn set_max_retry_attempts(&mut self, max_retry_attempts: u32) {
        self.max_retry_attempts = max_retry_attempts;
        self.updated_at = Utc::now();
    }

    fn set_updated_at(&mut self, updated_at: Option<DateTime<Utc>>) {
        self.updated_at = updated_at.unwrap_or_else(Utc::now);
    }

    fn set_media_references(&mut self, media_references: Vec<Media>) {
        self.media = Some(media_references);
        self.updated_at = Utc::now();
    }
}

// Request types now live under api::types to avoid conflicts

/// Library scan result
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LibraryScanResult {
    pub library_id: LibraryId,
    pub library_name: String,
    pub total_files: usize,
    pub new_files: usize,
    pub updated_files: usize,
    pub deleted_files: usize,
    pub errors: Vec<String>,
    pub duration_seconds: f64,
}

#[cfg(feature = "rkyv")]
pub use archived::ArchivedLibraryExt;

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use crate::ids::ArchivedLibraryId;
    use crate::media::{ArchivedMedia, ArchivedMovieReference};
    use rkyv::{option::ArchivedOption, vec::ArchivedVec};

    pub trait ArchivedLibraryExt {
        fn media(&self) -> Option<&ArchivedVec<ArchivedMedia>>;
        fn media_as_slice(&self) -> &[ArchivedMedia];
        fn get_movie_refs(
            &self,
        ) -> impl Iterator<Item = &ArchivedMovieReference>;
    }

    impl ArchivedLibraryExt for ArchivedLibrary {
        fn media(&self) -> Option<&ArchivedVec<ArchivedMedia>> {
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

        fn get_movie_refs(
            &self,
        ) -> impl Iterator<Item = &ArchivedMovieReference> {
            self.media_as_slice().iter().filter_map(|m| match m {
                ArchivedMedia::Movie(movie) => Some(movie),
                _ => None,
            })
        }
    }

    impl ArchivedLibrary {
        pub fn get_id(&self) -> ArchivedLibraryId {
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
}
