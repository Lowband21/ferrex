use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::MediaEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanSseEventType {
    Started,
    Progress,
    Quiescing,
    Completed,
    Failed,
}

impl ScanSseEventType {
    pub const fn event_name(self) -> &'static str {
        match self {
            Self::Started => "scan.started",
            Self::Progress => "scan.progress",
            Self::Quiescing => "scan.quiescing",
            Self::Completed => "scan.completed",
            Self::Failed => "scan.failed",
        }
    }
}

impl fmt::Display for ScanSseEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.event_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseScanSseEventTypeError {
    invalid_value: String,
}

impl ParseScanSseEventTypeError {
    pub fn new(value: &str) -> Self {
        Self {
            invalid_value: value.to_string(),
        }
    }
}

impl fmt::Display for ParseScanSseEventTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid scan SSE event type: {}", self.invalid_value)
    }
}

impl std::error::Error for ParseScanSseEventTypeError {}

impl FromStr for ScanSseEventType {
    type Err = ParseScanSseEventTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "scan.started" => Ok(Self::Started),
            "scan.progress" => Ok(Self::Progress),
            "scan.quiescing" => Ok(Self::Quiescing),
            "scan.completed" => Ok(Self::Completed),
            "scan.failed" => Ok(Self::Failed),
            other => Err(ParseScanSseEventTypeError::new(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSseEventType {
    MovieAdded,
    SeriesAdded,
    SeasonAdded,
    EpisodeAdded,
    MovieUpdated,
    SeriesUpdated,
    SeasonUpdated,
    EpisodeUpdated,
    MediaDeleted,
    Scan(ScanSseEventType),
}

impl MediaSseEventType {
    pub const fn event_name(self) -> &'static str {
        match self {
            Self::MovieAdded => "media.movie_added",
            Self::SeriesAdded => "media.series_added",
            Self::SeasonAdded => "media.season_added",
            Self::EpisodeAdded => "media.episode_added",
            Self::MovieUpdated => "media.movie_updated",
            Self::SeriesUpdated => "media.series_updated",
            Self::SeasonUpdated => "media.season_updated",
            Self::EpisodeUpdated => "media.episode_updated",
            Self::MediaDeleted => "media.deleted",
            Self::Scan(kind) => kind.event_name(),
        }
    }
}

impl fmt::Display for MediaSseEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.event_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseMediaSseEventTypeError {
    invalid_value: String,
}

impl ParseMediaSseEventTypeError {
    pub fn new(value: &str) -> Self {
        Self {
            invalid_value: value.to_string(),
        }
    }
}

impl fmt::Display for ParseMediaSseEventTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid media SSE event type: {}", self.invalid_value)
    }
}

impl std::error::Error for ParseMediaSseEventTypeError {}

impl FromStr for MediaSseEventType {
    type Err = ParseMediaSseEventTypeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "media.movie_added" => Ok(Self::MovieAdded),
            "media.series_added" => Ok(Self::SeriesAdded),
            "media.season_added" => Ok(Self::SeasonAdded),
            "media.episode_added" => Ok(Self::EpisodeAdded),
            "media.movie_updated" => Ok(Self::MovieUpdated),
            "media.series_updated" => Ok(Self::SeriesUpdated),
            "media.season_updated" => Ok(Self::SeasonUpdated),
            "media.episode_updated" => Ok(Self::EpisodeUpdated),
            "media.deleted" => Ok(Self::MediaDeleted),
            other => {
                match ScanSseEventType::from_str(other) {
                    Ok(kind) => Ok(Self::Scan(kind)),
                    Err(_) => Err(ParseMediaSseEventTypeError::new(other)),
                }
            }
        }
    }
}

impl MediaEvent {
    pub fn sse_event_type(&self) -> MediaSseEventType {
        match self {
            MediaEvent::MovieAdded { .. } => MediaSseEventType::MovieAdded,
            MediaEvent::SeriesAdded { .. } => MediaSseEventType::SeriesAdded,
            MediaEvent::SeasonAdded { .. } => MediaSseEventType::SeasonAdded,
            MediaEvent::EpisodeAdded { .. } => MediaSseEventType::EpisodeAdded,
            MediaEvent::MovieUpdated { .. } => MediaSseEventType::MovieUpdated,
            MediaEvent::SeriesUpdated { .. } => MediaSseEventType::SeriesUpdated,
            MediaEvent::SeasonUpdated { .. } => MediaSseEventType::SeasonUpdated,
            MediaEvent::EpisodeUpdated { .. } => MediaSseEventType::EpisodeUpdated,
            MediaEvent::MediaDeleted { .. } => MediaSseEventType::MediaDeleted,
            MediaEvent::ScanStarted { .. } => MediaSseEventType::Scan(ScanSseEventType::Started),
            MediaEvent::ScanProgress { .. } => MediaSseEventType::Scan(ScanSseEventType::Progress),
            MediaEvent::ScanCompleted { .. } => MediaSseEventType::Scan(ScanSseEventType::Completed),
            MediaEvent::ScanFailed { .. } => MediaSseEventType::Scan(ScanSseEventType::Failed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaSseEventType, ScanSseEventType};
    use std::str::FromStr;

    #[test]
    fn scan_event_name_roundtrip() {
        for (name, value) in [
            ("scan.started", ScanSseEventType::Started),
            ("scan.progress", ScanSseEventType::Progress),
            ("scan.quiescing", ScanSseEventType::Quiescing),
            ("scan.completed", ScanSseEventType::Completed),
            ("scan.failed", ScanSseEventType::Failed),
        ] {
            assert_eq!(value.event_name(), name);
            assert_eq!(ScanSseEventType::from_str(name).unwrap(), value);
        }
    }

    #[test]
    fn media_event_name_roundtrip() {
        for (name, value) in [
            ("media.movie_added", MediaSseEventType::MovieAdded),
            ("media.series_added", MediaSseEventType::SeriesAdded),
            ("media.season_added", MediaSseEventType::SeasonAdded),
            ("media.episode_added", MediaSseEventType::EpisodeAdded),
            ("media.movie_updated", MediaSseEventType::MovieUpdated),
            ("media.series_updated", MediaSseEventType::SeriesUpdated),
            ("media.season_updated", MediaSseEventType::SeasonUpdated),
            ("media.episode_updated", MediaSseEventType::EpisodeUpdated),
            ("media.deleted", MediaSseEventType::MediaDeleted),
            ("scan.started", MediaSseEventType::Scan(ScanSseEventType::Started)),
            ("scan.progress", MediaSseEventType::Scan(ScanSseEventType::Progress)),
            ("scan.quiescing", MediaSseEventType::Scan(ScanSseEventType::Quiescing)),
            ("scan.completed", MediaSseEventType::Scan(ScanSseEventType::Completed)),
            ("scan.failed", MediaSseEventType::Scan(ScanSseEventType::Failed)),
        ] {
            assert_eq!(value.event_name(), name);
            assert_eq!(MediaSseEventType::from_str(name).unwrap(), value);
        }
    }
}
