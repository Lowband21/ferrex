use chrono::{DateTime, Utc};
use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::{
    EpisodeReference, LibraryID, Media, MediaID, MovieReference,
    SeasonReference, SeriesReference,
};

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq, Eq))]
pub struct ScanStageLatencySummary {
    pub scan: u64,
    pub analyze: u64,
    pub index: u64,
}

#[derive(
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct ScanProgressEvent {
    pub version: String,
    pub scan_id: Uuid,
    pub library_id: LibraryID,
    pub status: String,
    pub completed_items: u64,
    pub total_items: u64,
    pub sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_key: Option<String>,
    pub p95_stage_latencies_ms: ScanStageLatencySummary,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    #[rkyv(with = crate::rkyv_wrappers::DateTimeWrapper)]
    pub emitted_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrying_items: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_lettered_items: Option<u64>,
}

impl fmt::Debug for ScanProgressEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanProgressEvent")
            .field("scan_id", &self.scan_id)
            .field("library_id", &self.library_id)
            .field("status", &self.status)
            .field("completed_items", &self.completed_items)
            .field("total_items", &self.total_items)
            .field("sequence", &self.sequence)
            .field("current_path", &self.current_path)
            .field("retrying_items", &self.retrying_items)
            .field("dead_lettered_items", &self.dead_lettered_items)
            .field("correlation_id", &self.correlation_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("p95_stage_latencies_ms", &self.p95_stage_latencies_ms)
            .field("emitted_at", &self.emitted_at)
            .finish()
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct ScanEventMetadata {
    pub version: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub library_id: LibraryID,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[serde(tag = "type", rename_all = "snake_case")]
#[rkyv(derive(Debug, PartialEq))]
pub enum MediaEvent {
    MovieAdded {
        movie: MovieReference,
    },
    SeriesAdded {
        series: SeriesReference,
    },
    SeasonAdded {
        season: SeasonReference,
    },
    EpisodeAdded {
        episode: EpisodeReference,
    },

    MovieUpdated {
        movie: MovieReference,
    },
    SeriesUpdated {
        series: SeriesReference,
    },
    SeasonUpdated {
        season: SeasonReference,
    },
    EpisodeUpdated {
        episode: EpisodeReference,
    },

    MediaDeleted {
        id: MediaID,
    },

    ScanStarted {
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanProgress {
        scan_id: Uuid,
        progress: ScanProgressEvent,
    },
    ScanCompleted {
        scan_id: Uuid,
        metadata: ScanEventMetadata,
    },
    ScanFailed {
        scan_id: Uuid,
        error: String,
        metadata: ScanEventMetadata,
    },
}

impl MediaEvent {
    pub fn into_media(self) -> Option<Media> {
        match self {
            MediaEvent::MovieAdded { movie }
            | MediaEvent::MovieUpdated { movie } => Some(Media::Movie(movie)),
            MediaEvent::SeriesAdded { series }
            | MediaEvent::SeriesUpdated { series } => {
                Some(Media::Series(series))
            }
            MediaEvent::SeasonAdded { season }
            | MediaEvent::SeasonUpdated { season } => {
                Some(Media::Season(season))
            }
            MediaEvent::EpisodeAdded { episode }
            | MediaEvent::EpisodeUpdated { episode } => {
                Some(Media::Episode(episode))
            }
            MediaEvent::MediaDeleted { .. }
            | MediaEvent::ScanStarted { .. }
            | MediaEvent::ScanProgress { .. }
            | MediaEvent::ScanCompleted { .. }
            | MediaEvent::ScanFailed { .. } => None,
        }
    }
}
