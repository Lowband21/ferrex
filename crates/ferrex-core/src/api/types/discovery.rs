use ferrex_model::{MediaID, VideoMediaType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::watch::{
    ContinueWatchingActionHint, ContinueWatchingItem, ItemWatchStatus,
    SeriesContinueWatchingItem,
};

/// Stable section id for the global resume/continue-watching shelf.
pub const DISCOVERY_SECTION_RESUME: &str = "resume";
/// Stable section id for the global recently-added shelf.
pub const DISCOVERY_SECTION_RECENTLY_ADDED: &str = "recently-added";
/// Stable section id for the global recently-released shelf.
pub const DISCOVERY_SECTION_RECENTLY_RELEASED: &str = "recently-released";
/// Stable section id for audience-rating based deterministic picks.
pub const DISCOVERY_SECTION_AUDIENCE_RATING_PICKS: &str =
    "audience-rating-picks";
/// Stable section id for watch-aware series continuation shelves.
pub const DISCOVERY_SECTION_CONTINUE_SERIES: &str = "continue-series";
/// Stable section id for series the authenticated user has not started.
pub const DISCOVERY_SECTION_UNWATCHED_SERIES: &str = "unwatched-series";

/// Top-level response for deterministic discovery endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryResponse {
    pub sections: Vec<DiscoverySection>,
}

impl DiscoveryResponse {
    pub fn new(sections: Vec<DiscoverySection>) -> Self {
        Self { sections }
    }
}

/// A renderable discovery shelf/section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoverySection {
    /// Stable section id, for example `recently-added`.
    pub id: String,
    /// Human-readable title rendered by clients.
    pub title: String,
    /// Short explanation for why the section exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Layout hint clients can map to their platform-specific UI.
    pub layout_hint: DiscoveryLayoutHint,
    /// Ordered, already-limited section items.
    pub items: Vec<DiscoveryItem>,
}

impl DiscoverySection {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        reason: Option<String>,
        layout_hint: DiscoveryLayoutHint,
        mut items: Vec<DiscoveryItem>,
        limit: usize,
    ) -> Self {
        if items.len() > limit {
            items.truncate(limit);
        }

        Self {
            id: id.into(),
            title: title.into(),
            reason,
            layout_hint,
            items,
        }
    }

    pub fn poster_row(
        id: impl Into<String>,
        title: impl Into<String>,
        reason: Option<String>,
        items: Vec<DiscoveryItem>,
        limit: usize,
    ) -> Self {
        Self::new(
            id,
            title,
            reason,
            DiscoveryLayoutHint::PosterRow,
            items,
            limit,
        )
    }

    pub fn continue_row(
        id: impl Into<String>,
        title: impl Into<String>,
        reason: Option<String>,
        items: Vec<DiscoveryItem>,
        limit: usize,
    ) -> Self {
        Self::new(
            id,
            title,
            reason,
            DiscoveryLayoutHint::ContinueRow,
            items,
            limit,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Lightweight layout hints rather than client-specific UI instructions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryLayoutHint {
    PosterRow,
    ContinueRow,
}

/// Renderable discovery item with enough denormalized metadata for shelves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryItem {
    /// Stable string key combining media kind and UUID, for example `movie:<uuid>`.
    pub id: String,
    /// Logical Ferrex media id.
    pub media_id: MediaID,
    /// Normalized media kind for JSON clients.
    pub media_type: DiscoveryMediaType,
    /// Primary display title.
    pub title: String,
    /// Optional display subtitle, such as an episode label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// Primary poster image iid when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_iid: Option<Uuid>,
    /// Primary backdrop image iid when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_iid: Option<Uuid>,
    /// ISO-like release/air date string as stored by metadata providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// Parsed year from release/air date when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_year: Option<u16>,
    /// Runtime in minutes when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_minutes: Option<u32>,
    /// Audience/critic rating summary when available.
    #[serde(default, skip_serializing_if = "DiscoveryRatingSummary::is_empty")]
    pub ratings: DiscoveryRatingSummary,
    /// User-specific watch-state summary when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<DiscoveryWatchSummary>,
    /// Primary action target for clients that can start playback directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback: Option<DiscoveryPlaybackAction>,
    /// Per-item deterministic recommendation/display reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DiscoveryItem {
    pub fn new(media_id: MediaID, title: impl Into<String>) -> Self {
        let media_type = DiscoveryMediaType::from(&media_id);
        Self {
            id: discovery_media_stable_id(&media_id),
            media_id,
            media_type,
            title: title.into(),
            subtitle: None,
            poster_iid: None,
            backdrop_iid: None,
            release_date: None,
            release_year: None,
            runtime_minutes: None,
            ratings: DiscoveryRatingSummary::default(),
            watch: None,
            playback: None,
            reason: None,
        }
    }
}

/// Normalized media kind for discovery JSON payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMediaType {
    Movie,
    Series,
    Season,
    Episode,
}

impl DiscoveryMediaType {
    pub const fn as_slug(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Season => "season",
            Self::Episode => "episode",
        }
    }
}

impl From<&MediaID> for DiscoveryMediaType {
    fn from(value: &MediaID) -> Self {
        match value {
            MediaID::Movie(_) => Self::Movie,
            MediaID::Series(_) => Self::Series,
            MediaID::Season(_) => Self::Season,
            MediaID::Episode(_) => Self::Episode,
        }
    }
}

impl From<VideoMediaType> for DiscoveryMediaType {
    fn from(value: VideoMediaType) -> Self {
        match value {
            VideoMediaType::Movie => Self::Movie,
            VideoMediaType::Series => Self::Series,
            VideoMediaType::Season => Self::Season,
            VideoMediaType::Episode => Self::Episode,
        }
    }
}

/// User-facing rating summary. TMDB vote average currently maps to `audience`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryRatingSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critic: Option<f32>,
}

impl DiscoveryRatingSummary {
    pub fn is_empty(&self) -> bool {
        self.audience.is_none() && self.critic.is_none()
    }
}

/// Watch-state summary optimized for shelf display.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct DiscoveryWatchSummary {
    pub state: DiscoveryWatchState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_seconds: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_watched_epoch_seconds: Option<i64>,
}

impl DiscoveryWatchSummary {
    pub fn from_continue_watching(item: &ContinueWatchingItem) -> Self {
        let state = match item.action_hint {
            ContinueWatchingActionHint::NextEpisode => {
                DiscoveryWatchState::Unwatched
            }
            ContinueWatchingActionHint::Resume => {
                DiscoveryWatchState::InProgress
            }
        };

        Self {
            state,
            progress: progress_ratio(item.position, item.duration),
            position_seconds: finite_seconds(item.position),
            duration_seconds: finite_seconds(item.duration),
            last_watched_epoch_seconds: Some(item.last_watched),
        }
    }

    /// Build a shelf watch summary from a library-scoped series continue row.
    pub fn from_series_continue_watching(
        item: &SeriesContinueWatchingItem,
    ) -> Self {
        let state = match item.action_hint {
            ContinueWatchingActionHint::NextEpisode => {
                DiscoveryWatchState::Unwatched
            }
            ContinueWatchingActionHint::Resume => {
                DiscoveryWatchState::InProgress
            }
        };

        Self {
            state,
            progress: progress_ratio(item.position, item.duration),
            position_seconds: finite_seconds(item.position),
            duration_seconds: finite_seconds(item.duration),
            last_watched_epoch_seconds: Some(item.last_watched),
        }
    }

    pub fn from_item_status(status: &ItemWatchStatus) -> Self {
        match status {
            ItemWatchStatus::InProgress(item) => Self {
                state: DiscoveryWatchState::InProgress,
                progress: progress_ratio(item.position, item.duration),
                position_seconds: finite_seconds(item.position),
                duration_seconds: finite_seconds(item.duration),
                last_watched_epoch_seconds: Some(item.last_watched),
            },
            ItemWatchStatus::Completed(item) => Self {
                state: DiscoveryWatchState::Completed,
                progress: Some(1.0),
                position_seconds: None,
                duration_seconds: None,
                last_watched_epoch_seconds: Some(item.last_watched),
            },
        }
    }
}

/// Coarse watch state for a discovery item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryWatchState {
    Unwatched,
    InProgress,
    Completed,
}

/// Primary playback/action target for an item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DiscoveryPlaybackAction {
    pub target_media_id: Uuid,
    pub target_media_type: DiscoveryMediaType,
    pub hint: DiscoveryPlaybackHint,
}

/// Hint describing how a client should label the primary action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryPlaybackHint {
    Play,
    Resume,
    NextEpisode,
}

/// Convert the existing continue-watching card DTO into a Discovery item.
///
/// `ContinueWatchingItem` keeps card identity and playback identity separate:
/// `card_media_id`/`media_type` identify the shelf card, while `action_target`
/// identifies the exact movie or episode clients should play.
pub fn discovery_item_from_continue_watching(
    continue_item: &ContinueWatchingItem,
) -> DiscoveryItem {
    let card_media_id =
        MediaID::from((continue_item.card_media_id, continue_item.media_type));

    let mut item = DiscoveryItem::new(
        card_media_id,
        continue_watching_title(continue_item),
    );
    item.subtitle = continue_item.subtitle.clone();
    item.poster_iid = continue_item.poster_iid;
    item.watch =
        Some(DiscoveryWatchSummary::from_continue_watching(continue_item));
    item.playback = Some(DiscoveryPlaybackAction {
        target_media_id: continue_item.action_target.media_id,
        target_media_type: DiscoveryMediaType::from(
            continue_item.action_target.media_type,
        ),
        hint: discovery_playback_hint_from_continue_watching(
            continue_item.action_hint,
        ),
    });
    item
}

/// Build a stable media key for section item identity and client diffing.
pub fn discovery_media_stable_id(media_id: &MediaID) -> String {
    format!(
        "{}:{}",
        DiscoveryMediaType::from(media_id).as_slug(),
        media_id.as_uuid()
    )
}

/// Parse a release/air date string and return its four-digit year.
pub fn release_year_from_date(date: &str) -> Option<u16> {
    let year = date.split_once('-').map_or(date, |(year, _)| year);
    if year.len() != 4 || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    year.parse().ok()
}

fn progress_ratio(position: f32, duration: f32) -> Option<f32> {
    if duration <= 0.0 || !position.is_finite() || !duration.is_finite() {
        return None;
    }

    Some((position / duration).clamp(0.0, 1.0))
}

fn finite_seconds(seconds: f32) -> Option<f32> {
    if seconds.is_finite() && seconds >= 0.0 {
        Some(seconds)
    } else {
        None
    }
}

fn continue_watching_title(item: &ContinueWatchingItem) -> String {
    let title = item.title.trim();
    if title.is_empty() {
        format!(
            "Untitled {}",
            DiscoveryMediaType::from(item.media_type).as_slug()
        )
    } else {
        title.to_string()
    }
}

fn discovery_playback_hint_from_continue_watching(
    hint: ContinueWatchingActionHint,
) -> DiscoveryPlaybackHint {
    match hint {
        ContinueWatchingActionHint::NextEpisode => {
            DiscoveryPlaybackHint::NextEpisode
        }
        ContinueWatchingActionHint::Resume => DiscoveryPlaybackHint::Resume,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrex_model::{EpisodeID, MovieID, SeriesID, VideoMediaType};

    use crate::domain::watch::{
        CompletedItem, ContinueWatchingActionTarget, InProgressItem,
    };

    fn item_with_uuid(uuid: Uuid) -> DiscoveryItem {
        DiscoveryItem::new(MediaID::Movie(MovieID(uuid)), "Item")
    }

    #[test]
    fn stable_media_ids_include_kind_and_uuid() {
        let movie_uuid = Uuid::from_u128(1);
        let series_uuid = Uuid::from_u128(2);

        assert_eq!(
            discovery_media_stable_id(&MediaID::Movie(MovieID(movie_uuid))),
            format!("movie:{movie_uuid}")
        );
        assert_eq!(
            discovery_media_stable_id(&MediaID::Series(SeriesID(series_uuid))),
            format!("series:{series_uuid}")
        );
    }

    #[test]
    fn continue_watching_movie_maps_to_resume_discovery_item() {
        let movie_uuid = Uuid::from_u128(10);
        let poster_uuid = Uuid::from_u128(11);
        let source = ContinueWatchingItem {
            media_id: movie_uuid,
            media_type: VideoMediaType::Movie,
            card_media_id: movie_uuid,
            action_target: ContinueWatchingActionTarget {
                media_id: movie_uuid,
                media_type: VideoMediaType::Movie,
            },
            action_hint: ContinueWatchingActionHint::Resume,
            position: 30.0,
            duration: 120.0,
            last_watched: 1_700_000_000,
            title: "Movie title".to_string(),
            subtitle: Some("Resume • 2m left".to_string()),
            poster_iid: Some(poster_uuid),
        };

        let item = discovery_item_from_continue_watching(&source);

        assert_eq!(item.id, format!("movie:{movie_uuid}"));
        assert_eq!(item.media_id, MediaID::Movie(MovieID(movie_uuid)));
        assert_eq!(item.media_type, DiscoveryMediaType::Movie);
        assert_eq!(item.title, "Movie title");
        assert_eq!(item.subtitle.as_deref(), Some("Resume • 2m left"));
        assert_eq!(item.poster_iid, Some(poster_uuid));
        assert_eq!(
            item.watch,
            Some(DiscoveryWatchSummary {
                state: DiscoveryWatchState::InProgress,
                progress: Some(0.25),
                position_seconds: Some(30.0),
                duration_seconds: Some(120.0),
                last_watched_epoch_seconds: Some(1_700_000_000),
            })
        );
        assert_eq!(
            item.playback,
            Some(DiscoveryPlaybackAction {
                target_media_id: movie_uuid,
                target_media_type: DiscoveryMediaType::Movie,
                hint: DiscoveryPlaybackHint::Resume,
            })
        );
    }

    #[test]
    fn continue_watching_series_card_targets_episode_action() {
        let episode_uuid = Uuid::from_u128(20);
        let series_uuid = Uuid::from_u128(21);
        let poster_uuid = Uuid::from_u128(22);
        let source = ContinueWatchingItem {
            media_id: episode_uuid,
            media_type: VideoMediaType::Series,
            card_media_id: series_uuid,
            action_target: ContinueWatchingActionTarget {
                media_id: episode_uuid,
                media_type: VideoMediaType::Episode,
            },
            action_hint: ContinueWatchingActionHint::NextEpisode,
            position: 0.0,
            duration: 0.0,
            last_watched: 1_700_000_100,
            title: "Series title".to_string(),
            subtitle: Some("Next up: S1 E2".to_string()),
            poster_iid: Some(poster_uuid),
        };

        let item = discovery_item_from_continue_watching(&source);

        assert_eq!(item.id, format!("series:{series_uuid}"));
        assert_eq!(item.media_id, MediaID::Series(SeriesID(series_uuid)));
        assert_eq!(item.media_type, DiscoveryMediaType::Series);
        assert_eq!(item.title, "Series title");
        assert_eq!(item.subtitle.as_deref(), Some("Next up: S1 E2"));
        assert_eq!(item.poster_iid, Some(poster_uuid));
        assert_eq!(
            item.watch,
            Some(DiscoveryWatchSummary {
                state: DiscoveryWatchState::Unwatched,
                progress: None,
                position_seconds: Some(0.0),
                duration_seconds: Some(0.0),
                last_watched_epoch_seconds: Some(1_700_000_100),
            })
        );
        assert_eq!(
            item.playback,
            Some(DiscoveryPlaybackAction {
                target_media_id: episode_uuid,
                target_media_type: DiscoveryMediaType::Episode,
                hint: DiscoveryPlaybackHint::NextEpisode,
            })
        );
    }

    #[test]
    fn continue_watching_mapping_degrades_missing_title_and_bad_timing() {
        let episode_uuid = Uuid::from_u128(30);
        let source = ContinueWatchingItem {
            media_id: episode_uuid,
            media_type: VideoMediaType::Episode,
            card_media_id: episode_uuid,
            action_target: ContinueWatchingActionTarget {
                media_id: episode_uuid,
                media_type: VideoMediaType::Episode,
            },
            action_hint: ContinueWatchingActionHint::Resume,
            position: f32::NAN,
            duration: f32::INFINITY,
            last_watched: 1_700_000_200,
            title: "   ".to_string(),
            subtitle: None,
            poster_iid: None,
        };

        let item = discovery_item_from_continue_watching(&source);

        assert_eq!(item.id, format!("episode:{episode_uuid}"));
        assert_eq!(item.media_id, MediaID::Episode(EpisodeID(episode_uuid)));
        assert_eq!(item.title, "Untitled episode");
        assert_eq!(
            item.watch,
            Some(DiscoveryWatchSummary {
                state: DiscoveryWatchState::InProgress,
                progress: None,
                position_seconds: None,
                duration_seconds: None,
                last_watched_epoch_seconds: Some(1_700_000_200),
            })
        );
        assert_eq!(
            item.playback,
            Some(DiscoveryPlaybackAction {
                target_media_id: episode_uuid,
                target_media_type: DiscoveryMediaType::Episode,
                hint: DiscoveryPlaybackHint::Resume,
            })
        );
    }

    #[test]
    fn item_watch_status_summaries_are_stable_for_shelves() {
        let media_uuid = Uuid::from_u128(40);
        let in_progress = ItemWatchStatus::InProgress(InProgressItem {
            media_id: media_uuid,
            position: 45.0,
            duration: 90.0,
            last_watched: 1_700_000_300,
        });
        assert_eq!(
            DiscoveryWatchSummary::from_item_status(&in_progress),
            DiscoveryWatchSummary {
                state: DiscoveryWatchState::InProgress,
                progress: Some(0.5),
                position_seconds: Some(45.0),
                duration_seconds: Some(90.0),
                last_watched_epoch_seconds: Some(1_700_000_300),
            }
        );

        let completed = ItemWatchStatus::Completed(CompletedItem {
            media_id: MediaID::Movie(MovieID(media_uuid)),
            last_watched: 1_700_000_400,
        });
        assert_eq!(
            DiscoveryWatchSummary::from_item_status(&completed),
            DiscoveryWatchSummary {
                state: DiscoveryWatchState::Completed,
                progress: Some(1.0),
                position_seconds: None,
                duration_seconds: None,
                last_watched_epoch_seconds: Some(1_700_000_400),
            }
        );
    }

    #[test]
    fn section_constructor_applies_limit_without_rewriting_id() {
        let section = DiscoverySection::poster_row(
            DISCOVERY_SECTION_RECENTLY_ADDED,
            "Recently added",
            Some("New in your library".to_string()),
            vec![
                item_with_uuid(Uuid::from_u128(1)),
                item_with_uuid(Uuid::from_u128(2)),
                item_with_uuid(Uuid::from_u128(3)),
            ],
            2,
        );

        assert_eq!(section.id, DISCOVERY_SECTION_RECENTLY_ADDED);
        assert_eq!(section.items.len(), 2);
        assert_eq!(
            section.items[0].id,
            format!("movie:{}", Uuid::from_u128(1))
        );
        assert_eq!(
            section.items[1].id,
            format!("movie:{}", Uuid::from_u128(2))
        );
    }

    #[test]
    fn release_year_parses_iso_like_dates_only() {
        assert_eq!(release_year_from_date("2024-03-10"), Some(2024));
        assert_eq!(release_year_from_date("1987"), Some(1987));
        assert_eq!(release_year_from_date("87-01-01"), None);
        assert_eq!(release_year_from_date("unknown"), None);
    }

    #[test]
    fn watch_progress_ratio_is_clamped_and_handles_bad_duration() {
        assert_eq!(progress_ratio(50.0, 100.0), Some(0.5));
        assert_eq!(progress_ratio(150.0, 100.0), Some(1.0));
        assert_eq!(progress_ratio(5.0, 0.0), None);
    }
}
