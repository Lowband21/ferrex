use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::types::ids::{EpisodeID, LibraryId, MovieID, SeasonID, SeriesID};
use crate::types::library::LibraryType;
use crate::{
    domain::media::tv_parser::TvParser,
    error::{MediaError, Result},
};

/// Node classification for a path being scanned.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ScanNodeKind {
    SeriesRoot,
    SeasonFolder {
        season_number: Option<u16>,
    },
    #[default]
    MovieFolder,
    EpisodeFile,
    ExtrasFolder {
        tag: Option<String>,
    },
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeriesHint {
    pub title: String,
    pub slug: Option<String>,
    pub year: Option<u16>,
    pub region: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeriesRef {
    pub id: SeriesID,
    pub slug: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SeriesLink {
    Resolved(SeriesRef),
    Hint(SeriesHint),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeasonRef {
    pub id: SeasonID,
    pub number: Option<u16>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SeasonLink {
    Resolved(SeasonRef),
    Number(u16),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeRef {
    pub id: EpisodeID,
    pub number: Option<u16>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeHint {
    pub number: u16,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EpisodeLink {
    Resolved(EpisodeRef),
    Hint(EpisodeHint),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtraTag(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct MovieRootPath(String);

impl TryFrom<String> for MovieRootPath {
    type Error = MediaError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<MovieRootPath> for String {
    fn from(value: MovieRootPath) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct SeriesRootPath(String);

/// Fully-qualified season folder path (must be directly under a `SeriesRootPath`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct SeasonFolderPath(String);

fn ensure_direct_child_path(
    library_root_path_norm: &str,
    candidate_path_norm: &str,
    label: &str,
) -> Result<()> {
    let library_root_path = Path::new(library_root_path_norm);
    let candidate_path = Path::new(candidate_path_norm);

    let Some(parent) = candidate_path.parent() else {
        return Err(MediaError::InvalidMedia(format!(
            "{label} must have a parent folder: {candidate_path_norm}"
        )));
    };

    if parent != library_root_path {
        return Err(MediaError::InvalidMedia(format!(
            "{label} must be a folder directly under the library root (root={library_root_path_norm}, candidate={candidate_path_norm})"
        )));
    }

    if candidate_path
        .file_name()
        .and_then(|n| n.to_str())
        .is_none()
    {
        return Err(MediaError::InvalidMedia(format!(
            "{label} must have a final folder name: {candidate_path_norm}"
        )));
    }

    Ok(())
}

impl MovieRootPath {
    pub fn try_new(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        Ok(Self(path))
    }

    pub fn try_new_under_library_root(
        library_root_path_norm: &str,
        path_norm: impl Into<String>,
    ) -> Result<Self> {
        let path_norm = path_norm.into();
        ensure_direct_child_path(
            library_root_path_norm,
            &path_norm,
            "movie root path",
        )?;
        Ok(Self(path_norm))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl SeriesRootPath {
    pub fn try_new(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        if looks_like_season_folder_path(&path) {
            return Err(MediaError::InvalidMedia(format!(
                "series root path must not be a season folder: {}",
                path
            )));
        }
        Ok(Self(path))
    }

    pub fn try_new_under_library_root(
        library_root_path_norm: &str,
        path_norm: impl Into<String>,
    ) -> Result<Self> {
        let path_norm = path_norm.into();
        ensure_direct_child_path(
            library_root_path_norm,
            &path_norm,
            "series root path",
        )?;
        if looks_like_season_folder_path(&path_norm) {
            return Err(MediaError::InvalidMedia(format!(
                "series root path must not be a season folder: {}",
                path_norm
            )));
        }
        Ok(Self(path_norm))
    }

    pub fn try_from_episode_file_path(path_norm: &str) -> Result<Self> {
        let path = Path::new(path_norm);
        let Some(parent) = path.parent() else {
            return Err(MediaError::InvalidMedia(format!(
                "episode path missing parent folder: {}",
                path_norm
            )));
        };

        let Some(parent_name) =
            parent.file_name().and_then(|name| name.to_str())
        else {
            return Err(MediaError::InvalidMedia(format!(
                "episode path parent folder missing name: {}",
                path_norm
            )));
        };

        if TvParser::parse_season_folder(parent_name).is_none() {
            return Err(MediaError::InvalidMedia(format!(
                "episode path parent folder is not a season folder: {}",
                path_norm
            )));
        }

        let Some(series_root) = parent.parent() else {
            return Err(MediaError::InvalidMedia(format!(
                "episode season folder missing series root parent: {}",
                path_norm
            )));
        };

        let series_root_norm = series_root.to_string_lossy().to_string();
        Self::try_new(series_root_norm)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for SeriesRootPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for MovieRootPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<String> for SeriesRootPath {
    type Error = MediaError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<SeriesRootPath> for String {
    fn from(value: SeriesRootPath) -> Self {
        value.0
    }
}

impl TryFrom<String> for SeasonFolderPath {
    type Error = MediaError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Ok(Self(value))
    }
}

impl From<SeasonFolderPath> for String {
    fn from(value: SeasonFolderPath) -> Self {
        value.0
    }
}

fn looks_like_season_folder_path(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|s| s.to_str())
    else {
        return false;
    };
    TvParser::parse_season_folder(name).is_some()
}

impl SeasonFolderPath {
    pub fn try_new_under_series_root(
        series_root_path: &SeriesRootPath,
        season_folder_path_norm: impl Into<String>,
    ) -> Result<(Self, u16)> {
        let season_folder_path_norm = season_folder_path_norm.into();
        let season_path = Path::new(&season_folder_path_norm);
        let series_root_path = Path::new(series_root_path.as_str());

        let Some(parent) = season_path.parent() else {
            return Err(MediaError::InvalidMedia(format!(
                "season folder path must have a parent: {}",
                season_folder_path_norm
            )));
        };

        if parent != series_root_path {
            return Err(MediaError::InvalidMedia(format!(
                "season folder must be directly under series root (series_root={}, season_folder={})",
                series_root_path.display(),
                season_folder_path_norm
            )));
        }

        let Some(folder_name) =
            season_path.file_name().and_then(|n| n.to_str())
        else {
            return Err(MediaError::InvalidMedia(format!(
                "season folder missing name: {}",
                season_folder_path_norm
            )));
        };

        let Some(season_number) = TvParser::parse_season_folder(folder_name)
        else {
            return Err(MediaError::InvalidMedia(format!(
                "season folder name did not parse as season: {}",
                season_folder_path_norm
            )));
        };

        Ok((Self(season_folder_path_norm), season_number))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Context supplied to folder scans so they can infer parent relationships.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FolderScanContext {
    Movie(MovieFolderScanContext),
    Series(SeriesFolderScanContext),
    Season(SeasonFolderScanContext),
}

impl FolderScanContext {
    pub fn library_id(&self) -> LibraryId {
        match self {
            FolderScanContext::Movie(ctx) => ctx.library_id,
            FolderScanContext::Series(ctx) => ctx.library_id,
            FolderScanContext::Season(ctx) => ctx.library_id,
        }
    }

    pub fn folder_path_norm(&self) -> &str {
        match self {
            FolderScanContext::Movie(ctx) => ctx.movie_root_path.as_str(),
            FolderScanContext::Series(ctx) => ctx.series_root_path.as_str(),
            FolderScanContext::Season(ctx) => ctx.season_folder_path.as_str(),
        }
    }

    pub fn series_root_path(&self) -> Option<&SeriesRootPath> {
        match self {
            FolderScanContext::Movie(_) => None,
            FolderScanContext::Series(ctx) => Some(&ctx.series_root_path),
            FolderScanContext::Season(ctx) => Some(&ctx.series_root_path),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeriesFolderScanContext {
    pub library_id: LibraryId,
    pub series_root_path: SeriesRootPath,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeasonFolderScanContext {
    pub library_id: LibraryId,
    pub series_root_path: SeriesRootPath,
    pub season_folder_path: SeasonFolderPath,
    pub season_number: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MovieFolderScanContext {
    pub library_id: LibraryId,
    pub movie_root_path: MovieRootPath,
}

/// Hierarchy information attached to scan contexts and media jobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovieScanHierarchy {
    pub movie_root_path: MovieRootPath,
    pub movie_id: Option<MovieID>,
    pub extra_tag: Option<ExtraTag>,
}

/// Hierarchy information attached to scan contexts and media jobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeriesScanHierarchy {
    pub series_root_path: SeriesRootPath,
    pub series: SeriesLink,
}

/// Hierarchy information attached to scan contexts and media jobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeasonScanHierarchy {
    pub series_root_path: SeriesRootPath,
    pub series: SeriesLink,
    pub season: SeasonLink,
}

impl SeasonScanHierarchy {
    pub fn from_series_hierarch(
        series_hierarchy: SeriesScanHierarchy,
        season_link: SeasonLink,
    ) -> SeasonScanHierarchy {
        SeasonScanHierarchy {
            series_root_path: series_hierarchy.series_root_path,
            series: series_hierarchy.series,
            season: season_link,
        }
    }
}

/// Hierarchy information attached to scan contexts and media jobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EpisodeScanHierarchy {
    pub series_root_path: SeriesRootPath,
    pub series: SeriesLink,
    pub season: SeasonLink,
    pub episode: EpisodeLink,
}

impl EpisodeScanHierarchy {
    pub fn from_season_hierarch(
        season_hierarchy: SeasonScanHierarchy,
        episode_link: EpisodeLink,
    ) -> EpisodeScanHierarchy {
        EpisodeScanHierarchy {
            series_root_path: season_hierarchy.series_root_path,
            series: season_hierarchy.series,
            season: season_hierarchy.season,
            episode: episode_link,
        }
    }
}

/// Hierarchy information attached to scan contexts and media jobs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanHierarchy {
    pub library_type: LibraryType,
    pub series: Option<SeriesLink>,
    pub season: Option<SeasonLink>,
    pub episode: Option<EpisodeLink>,
    pub movie_id: Option<MovieID>,
    pub extra_tag: Option<ExtraTag>,
    pub series_root_path: Option<SeriesRootPath>,
}

pub trait WithSeriesHierarchy: Send + Sync {
    fn series_id(&self) -> Option<SeriesID>;
    fn series_hint(&self) -> Option<&SeriesHint>;
    fn series_slug(&self) -> Option<&str>;
    fn series_title_hint(&self) -> Option<&str>;
    fn series_root(&self) -> &SeriesRootPath;
}

impl WithSeriesHierarchy for SeriesScanHierarchy {
    fn series_id(&self) -> Option<SeriesID> {
        match &self.series {
            SeriesLink::Resolved(reference) => Some(reference.id),
            _ => None,
        }
    }

    fn series_hint(&self) -> Option<&SeriesHint> {
        match &self.series {
            SeriesLink::Hint(hint) => Some(hint),
            _ => None,
        }
    }

    fn series_slug(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.slug.as_deref(),
            SeriesLink::Hint(hint) => hint.slug.as_deref(),
        }
    }

    fn series_title_hint(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.title.as_deref(),
            SeriesLink::Hint(hint) => Some(hint.title.as_str()),
        }
    }

    fn series_root(&self) -> &SeriesRootPath {
        &self.series_root_path
    }
}

impl WithSeriesHierarchy for SeasonScanHierarchy {
    fn series_id(&self) -> Option<SeriesID> {
        match &self.series {
            SeriesLink::Resolved(reference) => Some(reference.id),
            _ => None,
        }
    }

    fn series_hint(&self) -> Option<&SeriesHint> {
        match &self.series {
            SeriesLink::Hint(hint) => Some(hint),
            _ => None,
        }
    }

    fn series_slug(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.slug.as_deref(),
            SeriesLink::Hint(hint) => hint.slug.as_deref(),
        }
    }

    fn series_title_hint(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.title.as_deref(),
            SeriesLink::Hint(hint) => Some(hint.title.as_str()),
        }
    }

    fn series_root(&self) -> &SeriesRootPath {
        &self.series_root_path
    }
}

impl WithSeriesHierarchy for EpisodeScanHierarchy {
    fn series_id(&self) -> Option<SeriesID> {
        match &self.series {
            SeriesLink::Resolved(reference) => Some(reference.id),
            _ => None,
        }
    }

    fn series_hint(&self) -> Option<&SeriesHint> {
        match &self.series {
            SeriesLink::Hint(hint) => Some(hint),
            _ => None,
        }
    }

    fn series_slug(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.slug.as_deref(),
            SeriesLink::Hint(hint) => hint.slug.as_deref(),
        }
    }

    fn series_title_hint(&self) -> Option<&str> {
        match &self.series {
            SeriesLink::Resolved(reference) => reference.title.as_deref(),
            SeriesLink::Hint(hint) => Some(hint.title.as_str()),
        }
    }

    fn series_root(&self) -> &SeriesRootPath {
        &self.series_root_path
    }
}

impl SeriesScanHierarchy {
    pub fn new(
        series_link: SeriesLink,
        series_root_path: SeriesRootPath,
    ) -> SeriesScanHierarchy {
        SeriesScanHierarchy {
            series: series_link,
            series_root_path,
        }
    }
}
