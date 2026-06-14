use std::collections::HashMap;

use crate::{
    common::{
        focus::SpatialDirection,
        messages::{DomainMessage, DomainUpdateResult},
    },
    domains::{
        media::selectors,
        ui::{
            messages::UiMessage, playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage, theme, widgets::image_for,
        },
    },
    state::State,
};

use ferrex_core::player_prelude::{
    EpisodeID, ImageSize, Media, MediaID, MediaIDLike, MovieID, MovieLike,
    Priority, SeasonID, SeriesID, SeriesLike,
};
use iced::{
    Background, Border, Color, Element, Length, Shadow, Task, Theme, Vector,
    widget::{
        Column, Row, Space, button, column, container, mouse_area,
        operation::scroll_to, row, scrollable, text,
    },
};
use uuid::Uuid;

const PAGE_PADDING_X: f32 = 72.0;
const PAGE_PADDING_Y: f32 = 42.0;
const HERO_HEIGHT: f32 = 430.0;
const HERO_IMAGE_WIDTH: f32 = 250.0;
const HERO_POSTER_HEIGHT: f32 = 375.0;
const HERO_STILL_WIDTH: f32 = 470.0;
const HERO_STILL_HEIGHT: f32 = 265.0;
const ACTION_WIDTH: f32 = 245.0;
const ACTION_HEIGHT: f32 = 70.0;
const PANEL_GAP: f32 = 34.0;
const PANEL_HEADER_HEIGHT: f32 = 44.0;
const PANEL_ROW_HEIGHT: f32 = 154.0;
const PANEL_ROW_GAP: f32 = 18.0;
const PANEL_CARD_WIDTH: f32 = 312.0;
const PANEL_CARD_GAP: f32 = 18.0;
const SCROLL_FOLLOW_MARGIN: f32 = 44.0;
const PANEL_ROWS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenFootDetailAction {
    Primary,
    StartOver,
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootDetailPanelId {
    SeriesSeasons(SeriesID),
    SeriesEpisodes(SeriesID),
    SeasonEpisodes(SeasonID),
    EpisodeSiblings(SeasonID),
}

impl TenFootDetailPanelId {
    fn title(&self) -> &'static str {
        match self {
            Self::SeriesSeasons(_) => "Seasons",
            Self::SeriesEpisodes(_) => "Episodes",
            Self::SeasonEpisodes(_) => "Episodes",
            Self::EpisodeSiblings(_) => "This Season",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootDetailItemId {
    Season(SeasonID),
    Episode(EpisodeID),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootDetailFocusId {
    Action(TenFootDetailAction),
    PanelItem {
        panel: TenFootDetailPanelId,
        item: TenFootDetailItemId,
    },
}

#[derive(Debug, Clone)]
pub struct TenFootDetailState {
    pub focus_id: Option<TenFootDetailFocusId>,
    pub scrollable_id: iced::widget::Id,
    pub scroll_y: f32,
    pub viewport_height: f32,
    panel_windows: HashMap<TenFootDetailPanelId, usize>,
}

impl Default for TenFootDetailState {
    fn default() -> Self {
        Self {
            focus_id: None,
            scrollable_id: iced::widget::Id::unique(),
            scroll_y: 0.0,
            viewport_height: 0.0,
            panel_windows: HashMap::new(),
        }
    }
}

impl TenFootDetailState {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolved_focus(
        &self,
        data: &TenFootDetailData,
    ) -> Option<TenFootDetailFocusId> {
        self.focus_id
            .as_ref()
            .filter(|focus| data.contains_focus(focus))
            .cloned()
            .or_else(|| data.first_focus())
    }

    fn panel_window_start(
        &self,
        panel: &TenFootDetailPanelId,
        total: usize,
        columns: usize,
    ) -> usize {
        bounded_two_row_window_start(
            *self.panel_windows.get(panel).unwrap_or(&0),
            None,
            total,
            columns,
        )
    }

    fn follow_focus_window(
        &mut self,
        data: &TenFootDetailData,
        focus: &TenFootDetailFocusId,
        columns: usize,
    ) {
        let Some((panel, index, total)) = data.panel_item_position(focus)
        else {
            return;
        };

        let current = *self.panel_windows.get(&panel).unwrap_or(&0);
        let next = bounded_two_row_window_start(
            current,
            Some(index),
            total,
            columns.max(1),
        );
        self.panel_windows.insert(panel, next);
    }

    fn scroll_task_for_focus(
        &mut self,
        data: &TenFootDetailData,
        focus: &TenFootDetailFocusId,
        fallback_height: f32,
    ) -> Task<UiMessage> {
        let Some((top, height)) = data.focus_vertical_bounds(focus) else {
            return Task::none();
        };

        let viewport_height =
            self.viewport_height.max(fallback_height).max(1.0);
        let visible_top = self.scroll_y;
        let visible_bottom = visible_top + viewport_height;
        let target = if top < visible_top + SCROLL_FOLLOW_MARGIN {
            (top - SCROLL_FOLLOW_MARGIN).max(0.0)
        } else if top + height > visible_bottom - SCROLL_FOLLOW_MARGIN {
            (top + height + SCROLL_FOLLOW_MARGIN - viewport_height).max(0.0)
        } else {
            return Task::none();
        };

        if (target - self.scroll_y).abs() < 1.0 {
            return Task::none();
        }

        self.scroll_y = target;
        scroll_to::<UiMessage>(
            self.scrollable_id.clone(),
            iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: target },
        )
    }
}

#[derive(Debug, Clone)]
pub enum TenFootDetailMessage {
    Move(SpatialDirection),
    ActivateFocused,
    Activate(TenFootDetailFocusId),
    Focus(TenFootDetailFocusId),
    Back,
    Scrolled(scrollable::Viewport),
}

impl From<TenFootDetailMessage> for UiMessage {
    fn from(message: TenFootDetailMessage) -> Self {
        UiMessage::TenFootDetail(message)
    }
}

#[derive(Debug, Clone)]
struct DetailActionSpec {
    action: TenFootDetailAction,
    label: String,
    subtitle: String,
    activation: TenFootDetailActivation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TenFootDetailActivation {
    PlayMedia(MediaID),
    PlayMediaFromStart(MediaID),
    PlaySeriesNextEpisode(SeriesID),
    ViewSeason(SeriesID, SeasonID),
    ViewEpisode(EpisodeID),
    Back,
}

#[derive(Debug, Clone)]
enum DetailImage {
    Poster {
        media_uuid: Uuid,
        iid: Option<Uuid>,
        placeholder: lucide_icons::Icon,
    },
    Still {
        media_uuid: Uuid,
        iid: Option<Uuid>,
    },
    None,
}

#[derive(Debug, Clone)]
struct TenFootDetailPanel {
    id: TenFootDetailPanelId,
    empty_message: String,
    items: Vec<TenFootDetailPanelItem>,
}

#[derive(Debug, Clone)]
enum TenFootDetailPanelItem {
    Season(SeasonPanelItem),
    Episode(EpisodePanelItem),
}

impl TenFootDetailPanelItem {
    fn id(&self) -> TenFootDetailItemId {
        match self {
            Self::Season(item) => TenFootDetailItemId::Season(item.id),
            Self::Episode(item) => TenFootDetailItemId::Episode(item.id),
        }
    }

    fn activation(&self) -> TenFootDetailActivation {
        match self {
            Self::Season(item) => {
                TenFootDetailActivation::ViewSeason(item.series_id, item.id)
            }
            Self::Episode(item) => {
                TenFootDetailActivation::ViewEpisode(item.id)
            }
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Season(item) => &item.title,
            Self::Episode(item) => &item.title,
        }
    }

    fn subtitle(&self) -> &str {
        match self {
            Self::Season(item) => &item.subtitle,
            Self::Episode(item) => &item.subtitle,
        }
    }

    fn context(&self) -> &str {
        match self {
            Self::Season(item) => &item.context,
            Self::Episode(item) => &item.context,
        }
    }

    fn image(&self) -> DetailImage {
        match self {
            Self::Season(item) => DetailImage::Poster {
                media_uuid: item.id.to_uuid(),
                iid: item.poster_iid,
                placeholder: lucide_icons::Icon::Tv,
            },
            Self::Episode(item) => DetailImage::Still {
                media_uuid: item.id.to_uuid(),
                iid: item.still_iid,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct SeasonPanelItem {
    id: SeasonID,
    series_id: SeriesID,
    title: String,
    subtitle: String,
    context: String,
    poster_iid: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct EpisodePanelItem {
    id: EpisodeID,
    title: String,
    subtitle: String,
    context: String,
    still_iid: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct TenFootDetailData {
    eyebrow: String,
    title: String,
    subtitle: String,
    metadata: Vec<String>,
    overview: String,
    image: DetailImage,
    actions: Vec<DetailActionSpec>,
    panels: Vec<TenFootDetailPanel>,
    notice: Option<String>,
}

impl TenFootDetailData {
    fn from_state(state: &State) -> Self {
        match state.domains.ui.state.view.clone() {
            crate::domains::ui::types::ViewState::MovieDetail {
                movie_id,
                ..
            } => Self::movie(state, movie_id),
            crate::domains::ui::types::ViewState::SeriesDetail {
                series_id,
                ..
            } => Self::series(state, series_id),
            crate::domains::ui::types::ViewState::SeasonDetail {
                series_id,
                season_id,
                ..
            } => Self::season(state, series_id, season_id),
            crate::domains::ui::types::ViewState::EpisodeDetail {
                episode_id,
                ..
            } => Self::episode(state, episode_id),
            _ => Self::unavailable(
                "Details unavailable",
                "This route is not a 10-foot detail route.",
            ),
        }
    }

    fn movie(state: &State, movie_id: MovieID) -> Self {
        let media_id = MediaID::Movie(movie_id);
        let Ok(Media::Movie(movie)) =
            state.domains.ui.state.repo_accessor.get(&media_id)
        else {
            return Self::unavailable(
                "Movie unavailable",
                format!(
                    "Movie {} is not present in the local repository, so no playable file or ordering can be shown.",
                    movie_id.to_uuid()
                ),
            );
        };

        let watch = watch_info_for_media(state, &media_id);
        let mut metadata = Vec::new();
        if let Some(year) = movie.release_year() {
            metadata.push(year.to_string());
        }
        if let Some(runtime) = movie.details.runtime {
            metadata.push(runtime_label(runtime));
        } else if let Some(duration) = movie
            .file
            .media_file_metadata
            .as_ref()
            .and_then(|meta| meta.duration)
        {
            metadata.push(duration_label(duration));
        }
        if let Some(rating) = movie.details.vote_average {
            metadata.push(format!("★ {:.1}", rating));
        }
        if let Some(content_rating) = movie.details.content_rating.as_ref() {
            metadata.push(content_rating.clone());
        }
        if let Some(progress) = watch.progress_label() {
            metadata.push(progress);
        }

        let genres = movie
            .details
            .genres
            .iter()
            .map(|genre| genre.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let subtitle = if genres.is_empty() {
            "Movie".to_string()
        } else {
            format!("Movie • {genres}")
        };

        Self {
            eyebrow: "Movie Details".to_string(),
            title: movie.title().to_string(),
            subtitle,
            metadata,
            overview: movie.details.overview.clone().unwrap_or_else(|| {
                "No overview is available for this movie.".to_string()
            }),
            image: DetailImage::Poster {
                media_uuid: movie_id.to_uuid(),
                iid: movie.details.primary_poster_iid,
                placeholder: lucide_icons::Icon::Film,
            },
            actions: action_specs(
                Some(TenFootDetailActivation::PlayMedia(media_id)),
                primary_label_for_watch_info(watch),
                if watch.in_progress {
                    "Continue from the saved position"
                } else {
                    "Start playback"
                },
                start_over_available_for_watch_info(watch).then_some(
                    TenFootDetailActivation::PlayMediaFromStart(media_id),
                ),
            ),
            panels: Vec::new(),
            notice: None,
        }
    }

    fn series(state: &State, series_id: SeriesID) -> Self {
        let media_id = MediaID::Series(series_id);
        let Ok(Media::Series(series)) =
            state.domains.ui.state.repo_accessor.get(&media_id)
        else {
            return Self::unavailable(
                "Series unavailable",
                format!(
                    "Series {} is not present in the local repository, so season ordering and playable episode mapping are unavailable.",
                    series_id.to_uuid()
                ),
            );
        };

        let seasons_result = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_series_seasons(&series_id);
        let seasons_unavailable = seasons_result.is_err();
        let mut seasons = seasons_result.unwrap_or_default();
        seasons.sort_by_key(|season| season.season_number.value());

        let mut episode_fetch_failed = false;
        let mut episodes = Vec::new();
        for season in &seasons {
            match state
                .domains
                .ui
                .state
                .repo_accessor
                .get_season_episodes(&season.id)
            {
                Ok(mut season_episodes) => {
                    season_episodes
                        .sort_by_key(|episode| episode.episode_number.value());
                    episodes.extend(season_episodes);
                }
                Err(_) => {
                    episode_fetch_failed = true;
                }
            }
        }
        episodes.sort_by_key(|episode| {
            (
                episode.season_number.value(),
                episode.episode_number.value(),
            )
        });

        let next_episode =
            selectors::select_next_episode_for_series(state, series_id);
        let next_watch = next_episode
            .map(|id| watch_info_for_media(state, &MediaID::Episode(id)))
            .unwrap_or_default();
        let first_episode =
            selectors::select_first_episode_for_series(state, series_id);
        let has_any_watch_state = episodes.iter().any(|episode| {
            watch_info_for_media(state, &MediaID::Episode(episode.id))
                .has_watch_state
        });

        let mut metadata = Vec::new();
        if let Some(first_air_date) = series.details.first_air_date.as_ref() {
            if let Some(year) = first_air_date.split('-').next() {
                metadata.push(year.to_string());
            }
        }
        if !seasons.is_empty() {
            metadata.push(plural_label(seasons.len(), "season", "seasons"));
        } else if let Some(count) = series.details.number_of_seasons {
            metadata.push(plural_label(count as usize, "season", "seasons"));
        }
        if !episodes.is_empty() {
            metadata.push(plural_label(episodes.len(), "episode", "episodes"));
        } else if let Some(count) = series.details.number_of_episodes {
            metadata.push(plural_label(count as usize, "episode", "episodes"));
        }
        if let Some(rating) = series.details.vote_average {
            metadata.push(format!("★ {:.1}", rating));
        }
        if let Some(status) = series.details.status.as_ref() {
            metadata.push(status.clone());
        }

        let genres = series
            .details
            .genres
            .iter()
            .map(|genre| genre.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let subtitle = if genres.is_empty() {
            "Series".to_string()
        } else {
            format!("Series • {genres}")
        };

        let mut panels = Vec::new();
        panels.push(TenFootDetailPanel {
            id: TenFootDetailPanelId::SeriesSeasons(series_id),
            empty_message: if seasons_unavailable {
                format!(
                    "Local season rows for series {} could not be read; season ordering is unavailable.",
                    series_id.to_uuid()
                )
            } else {
                format!(
                    "No local season rows were found for series {}. Season ordering cannot be inferred without local IDs.",
                    series_id.to_uuid()
                )
            },
            items: seasons
                .iter()
                .map(|season| season_panel_item(season))
                .collect(),
        });
        panels.push(TenFootDetailPanel {
            id: TenFootDetailPanelId::SeriesEpisodes(series_id),
            empty_message: if episode_fetch_failed {
                format!(
                    "Some season episode rows for series {} could not be read; playable episode ordering is incomplete.",
                    series_id.to_uuid()
                )
            } else {
                format!(
                    "No local episode rows were found for series {}. Playable episode mapping is unavailable.",
                    series_id.to_uuid()
                )
            },
            items: episodes
                .iter()
                .map(|episode| episode_panel_item(episode))
                .collect(),
        });

        let primary_activation = next_episode
            .map(|_| TenFootDetailActivation::PlaySeriesNextEpisode(series_id));
        let start_over_activation = first_episode.and_then(|id| {
            has_any_watch_state.then_some(
                TenFootDetailActivation::PlayMediaFromStart(MediaID::Episode(
                    id,
                )),
            )
        });

        let notice = if primary_activation.is_none() {
            Some(format!(
                "No local playable episode mapping is available for series {}. Primary playback is disabled until an episode row exists.",
                series_id.to_uuid()
            ))
        } else {
            None
        };

        Self {
            eyebrow: "Series Details".to_string(),
            title: series.title().to_string(),
            subtitle,
            metadata,
            overview: series.details.overview.clone().unwrap_or_else(|| {
                "No overview is available for this series.".to_string()
            }),
            image: DetailImage::Poster {
                media_uuid: series_id.to_uuid(),
                iid: series.details.primary_poster_iid,
                placeholder: lucide_icons::Icon::Tv,
            },
            actions: action_specs(
                primary_activation,
                primary_label_for_watch_info(next_watch),
                if next_watch.in_progress {
                    "Resume the next episode"
                } else {
                    "Play the next episode"
                },
                start_over_activation,
            ),
            panels,
            notice,
        }
    }

    fn season(state: &State, series_id: SeriesID, season_id: SeasonID) -> Self {
        let media_id = MediaID::Season(season_id);
        let Ok(Media::Season(season)) =
            state.domains.ui.state.repo_accessor.get(&media_id)
        else {
            return Self::unavailable(
                "Season unavailable",
                format!(
                    "Season {} is not present in the local repository, so episode ordering and playable mapping are unavailable.",
                    season_id.to_uuid()
                ),
            );
        };

        let episodes_result = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_episodes(&season_id);
        let episodes_unavailable = episodes_result.is_err();
        let mut episodes = episodes_result.unwrap_or_default();
        episodes.sort_by_key(|episode| episode.episode_number.value());

        let next_episode =
            selectors::select_next_episode_for_season(state, season_id);
        let next_watch = next_episode
            .map(|id| watch_info_for_media(state, &MediaID::Episode(id)))
            .unwrap_or_default();
        let first_episode =
            selectors::select_first_episode_for_season(state, season_id);
        let has_any_watch_state = episodes.iter().any(|episode| {
            watch_info_for_media(state, &MediaID::Episode(episode.id))
                .has_watch_state
        });

        let series_title = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Series(series_id))
            .ok()
            .and_then(|media| match media {
                Media::Series(series) => Some(series.title().to_string()),
                _ => None,
            });

        let mut metadata = Vec::new();
        metadata.push(season_title(&season));
        if !episodes.is_empty() {
            metadata.push(plural_label(episodes.len(), "episode", "episodes"));
        } else {
            metadata.push(plural_label(
                season.details.episode_count as usize,
                "episode",
                "episodes",
            ));
        }
        if let Some(air_date) = season.details.air_date.as_ref() {
            if let Some(year) = air_date.split('-').next() {
                metadata.push(year.to_string());
            }
        }
        if let Some(runtime) = season.details.runtime {
            metadata.push(format!("{} min avg", runtime));
        }

        let primary_activation = next_episode
            .map(|id| TenFootDetailActivation::PlayMedia(MediaID::Episode(id)));
        let start_over_activation = first_episode.and_then(|id| {
            has_any_watch_state.then_some(
                TenFootDetailActivation::PlayMediaFromStart(MediaID::Episode(
                    id,
                )),
            )
        });

        let notice = if primary_activation.is_none() {
            Some(format!(
                "No local playable episodes were found for season {}. Primary playback is disabled until episode rows exist.",
                season_id.to_uuid()
            ))
        } else {
            None
        };

        Self {
            eyebrow: "Season Details".to_string(),
            title: season
                .details
                .name
                .clone()
                .is_empty()
                .then(|| season_title(&season))
                .unwrap_or_else(|| season.details.name.clone()),
            subtitle: series_title.unwrap_or_else(|| {
                format!(
                    "Parent series {} is unavailable locally",
                    series_id.to_uuid()
                )
            }),
            metadata,
            overview: season.details.overview.clone().unwrap_or_else(|| {
                "No overview is available for this season.".to_string()
            }),
            image: DetailImage::Poster {
                media_uuid: season_id.to_uuid(),
                iid: season.details.primary_poster_iid,
                placeholder: lucide_icons::Icon::Tv,
            },
            actions: action_specs(
                primary_activation,
                primary_label_for_watch_info(next_watch),
                if next_watch.in_progress {
                    "Resume this season"
                } else {
                    "Play this season"
                },
                start_over_activation,
            ),
            panels: vec![TenFootDetailPanel {
                id: TenFootDetailPanelId::SeasonEpisodes(season_id),
                empty_message: if episodes_unavailable {
                    format!(
                        "Local episode rows for season {} could not be read; episode ordering is unavailable.",
                        season_id.to_uuid()
                    )
                } else {
                    format!(
                        "No local episode rows were found for season {}. Playable mapping cannot be shown.",
                        season_id.to_uuid()
                    )
                },
                items: episodes
                    .iter()
                    .map(|episode| episode_panel_item(episode))
                    .collect(),
            }],
            notice,
        }
    }

    fn episode(state: &State, episode_id: EpisodeID) -> Self {
        let media_id = MediaID::Episode(episode_id);
        let Ok(Media::Episode(episode)) =
            state.domains.ui.state.repo_accessor.get(&media_id)
        else {
            return Self::unavailable(
                "Episode unavailable",
                format!(
                    "Episode {} is not present in the local repository, so no playable file or sibling ordering can be shown.",
                    episode_id.to_uuid()
                ),
            );
        };

        let watch = watch_info_for_media(state, &media_id);
        let siblings_result = state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_episodes(&episode.season_id);
        let siblings_unavailable = siblings_result.is_err();
        let mut siblings = siblings_result.unwrap_or_default();
        siblings.sort_by_key(|candidate| candidate.episode_number.value());

        let series_title = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Series(episode.series_id))
            .ok()
            .and_then(|media| match media {
                Media::Series(series) => Some(series.title().to_string()),
                _ => None,
            });
        let season_title = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Season(episode.season_id))
            .ok()
            .and_then(|media| match media {
                Media::Season(season) => Some(season_title(&season)),
                _ => None,
            });

        let mut metadata = Vec::new();
        metadata.push(format!(
            "S{:02}E{:02}",
            episode.season_number.value(),
            episode.episode_number.value()
        ));
        if let Some(air_date) = episode.details.air_date.as_ref() {
            metadata.push(air_date.clone());
        }
        if let Some(runtime) = episode.details.runtime {
            metadata.push(format!("{} min", runtime));
        }
        if let Some(rating) = episode.details.vote_average {
            metadata.push(format!("★ {:.1}", rating));
        }
        if let Some(progress) = watch.progress_label() {
            metadata.push(progress);
        }

        let subtitle = [series_title, season_title]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" • ");

        let panel_empty = if siblings_unavailable {
            format!(
                "Local episode rows for season {} could not be read; sibling episode ordering is unavailable.",
                episode.season_id.to_uuid()
            )
        } else {
            format!(
                "No local sibling episode rows were found for season {}. Episode ordering cannot be shown.",
                episode.season_id.to_uuid()
            )
        };

        Self {
            eyebrow: "Episode Details".to_string(),
            title: if episode.details.name.is_empty() {
                format!("Episode {}", episode.episode_number.value())
            } else {
                episode.details.name.clone()
            },
            subtitle: if subtitle.is_empty() {
                format!(
                    "Series {} • Season {}",
                    episode.series_id.to_uuid(),
                    episode.season_id.to_uuid()
                )
            } else {
                subtitle
            },
            metadata,
            overview: episode.details.overview.clone().unwrap_or_else(|| {
                "No overview is available for this episode.".to_string()
            }),
            image: DetailImage::Still {
                media_uuid: episode_id.to_uuid(),
                iid: episode.details.primary_still_iid,
            },
            actions: action_specs(
                Some(TenFootDetailActivation::PlayMedia(media_id)),
                primary_label_for_watch_info(watch),
                if watch.in_progress {
                    "Continue from the saved position"
                } else {
                    "Start playback"
                },
                start_over_available_for_watch_info(watch).then_some(
                    TenFootDetailActivation::PlayMediaFromStart(media_id),
                ),
            ),
            panels: vec![TenFootDetailPanel {
                id: TenFootDetailPanelId::EpisodeSiblings(episode.season_id),
                empty_message: panel_empty,
                items: siblings
                    .iter()
                    .map(|episode| episode_panel_item(episode))
                    .collect(),
            }],
            notice: None,
        }
    }

    fn unavailable(
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            eyebrow: "Details".to_string(),
            title: title.into(),
            subtitle: "Local data unavailable".to_string(),
            metadata: Vec::new(),
            overview: message.into(),
            image: DetailImage::None,
            actions: action_specs(None, "Play", "Unavailable", None),
            panels: Vec::new(),
            notice: Some(
                "Use Back to return to the previous screen.".to_string(),
            ),
        }
    }

    fn first_focus(&self) -> Option<TenFootDetailFocusId> {
        self.actions
            .first()
            .map(|action| TenFootDetailFocusId::Action(action.action))
            .or_else(|| self.first_panel_focus_from(0, 0))
    }

    fn contains_focus(&self, focus: &TenFootDetailFocusId) -> bool {
        match focus {
            TenFootDetailFocusId::Action(action) => {
                self.actions.iter().any(|spec| spec.action == *action)
            }
            TenFootDetailFocusId::PanelItem { panel, item } => {
                self.panel(panel).is_some_and(|panel| {
                    panel.items.iter().any(|i| i.id() == *item)
                })
            }
        }
    }

    fn panel(
        &self,
        panel: &TenFootDetailPanelId,
    ) -> Option<&TenFootDetailPanel> {
        self.panels.iter().find(|candidate| &candidate.id == panel)
    }

    fn panel_index(&self, panel: &TenFootDetailPanelId) -> Option<usize> {
        self.panels
            .iter()
            .position(|candidate| &candidate.id == panel)
    }

    fn first_panel_focus_from(
        &self,
        start: usize,
        preferred_index: usize,
    ) -> Option<TenFootDetailFocusId> {
        self.panels.iter().skip(start).find_map(|panel| {
            if panel.items.is_empty() {
                return None;
            }
            self.focus_for_panel_index(panel, preferred_index)
        })
    }

    fn previous_populated_panel(
        &self,
        before: usize,
    ) -> Option<&TenFootDetailPanel> {
        self.panels
            .iter()
            .take(before)
            .rev()
            .find(|panel| !panel.items.is_empty())
    }

    fn next_populated_panel(
        &self,
        after: usize,
    ) -> Option<&TenFootDetailPanel> {
        self.panels
            .iter()
            .skip(after + 1)
            .find(|panel| !panel.items.is_empty())
    }

    fn focus_for_panel_index(
        &self,
        panel: &TenFootDetailPanel,
        preferred_index: usize,
    ) -> Option<TenFootDetailFocusId> {
        let index = preferred_index.min(panel.items.len().saturating_sub(1));
        panel
            .items
            .get(index)
            .map(|item| TenFootDetailFocusId::PanelItem {
                panel: panel.id.clone(),
                item: item.id(),
            })
    }

    fn action_index(&self, action: TenFootDetailAction) -> Option<usize> {
        self.actions.iter().position(|spec| spec.action == action)
    }

    fn focus_for_action_index(
        &self,
        index: usize,
    ) -> Option<TenFootDetailFocusId> {
        let index = index.min(self.actions.len().saturating_sub(1));
        self.actions
            .get(index)
            .map(|spec| TenFootDetailFocusId::Action(spec.action))
    }

    fn panel_item_position(
        &self,
        focus: &TenFootDetailFocusId,
    ) -> Option<(TenFootDetailPanelId, usize, usize)> {
        let TenFootDetailFocusId::PanelItem { panel, item } = focus else {
            return None;
        };
        let panel_ref = self.panel(panel)?;
        let index = panel_ref
            .items
            .iter()
            .position(|candidate| candidate.id() == *item)?;
        Some((panel.clone(), index, panel_ref.items.len()))
    }

    fn activation_for_focus(
        &self,
        focus: &TenFootDetailFocusId,
    ) -> Option<TenFootDetailActivation> {
        match focus {
            TenFootDetailFocusId::Action(action) => self
                .actions
                .iter()
                .find(|spec| spec.action == *action)
                .map(|spec| spec.activation.clone()),
            TenFootDetailFocusId::PanelItem { panel, item } => self
                .panel(panel)?
                .items
                .iter()
                .find(|candidate| candidate.id() == *item)
                .map(|item| item.activation()),
        }
    }

    fn move_focus(
        &self,
        current: Option<&TenFootDetailFocusId>,
        direction: SpatialDirection,
        columns: usize,
    ) -> Option<TenFootDetailFocusId> {
        let current = current
            .filter(|focus| self.contains_focus(focus))
            .cloned()
            .or_else(|| self.first_focus())?;
        let columns = columns.max(1);

        match current {
            TenFootDetailFocusId::Action(action) => {
                let action_index = self.action_index(action).unwrap_or(0);
                match direction {
                    SpatialDirection::Left => {
                        if action_index > 0 {
                            self.focus_for_action_index(action_index - 1)
                        } else {
                            Some(TenFootDetailFocusId::Action(action))
                        }
                    }
                    SpatialDirection::Right => {
                        if action_index + 1 < self.actions.len() {
                            self.focus_for_action_index(action_index + 1)
                        } else {
                            Some(TenFootDetailFocusId::Action(action))
                        }
                    }
                    SpatialDirection::Down => self
                        .first_panel_focus_from(0, action_index)
                        .or(Some(TenFootDetailFocusId::Action(action))),
                    SpatialDirection::Up => {
                        Some(TenFootDetailFocusId::Action(action))
                    }
                }
            }
            TenFootDetailFocusId::PanelItem { panel, item } => {
                let panel_index = self.panel_index(&panel)?;
                let panel_ref = self.panel(&panel)?;
                let item_index = panel_ref
                    .items
                    .iter()
                    .position(|candidate| candidate.id() == item)?;
                match direction {
                    SpatialDirection::Left => {
                        if item_index > 0 {
                            self.focus_for_panel_index(
                                panel_ref,
                                item_index - 1,
                            )
                        } else {
                            Some(TenFootDetailFocusId::PanelItem {
                                panel,
                                item,
                            })
                        }
                    }
                    SpatialDirection::Right => {
                        if item_index + 1 < panel_ref.items.len() {
                            self.focus_for_panel_index(
                                panel_ref,
                                item_index + 1,
                            )
                        } else {
                            Some(TenFootDetailFocusId::PanelItem {
                                panel,
                                item,
                            })
                        }
                    }
                    SpatialDirection::Up => {
                        if item_index >= columns {
                            self.focus_for_panel_index(
                                panel_ref,
                                item_index - columns,
                            )
                        } else if let Some(previous_panel) =
                            self.previous_populated_panel(panel_index)
                        {
                            let preferred = previous_panel
                                .items
                                .len()
                                .saturating_sub(columns)
                                + item_index.min(columns - 1);
                            self.focus_for_panel_index(
                                previous_panel,
                                preferred,
                            )
                        } else if !self.actions.is_empty() {
                            self.focus_for_action_index(
                                item_index.min(columns - 1),
                            )
                        } else {
                            Some(TenFootDetailFocusId::PanelItem {
                                panel,
                                item,
                            })
                        }
                    }
                    SpatialDirection::Down => {
                        if item_index + columns < panel_ref.items.len() {
                            self.focus_for_panel_index(
                                panel_ref,
                                item_index + columns,
                            )
                        } else if let Some(next_panel) =
                            self.next_populated_panel(panel_index)
                        {
                            self.focus_for_panel_index(
                                next_panel,
                                item_index % columns,
                            )
                        } else {
                            Some(TenFootDetailFocusId::PanelItem {
                                panel,
                                item,
                            })
                        }
                    }
                }
            }
        }
    }

    fn focus_vertical_bounds(
        &self,
        focus: &TenFootDetailFocusId,
    ) -> Option<(f32, f32)> {
        match focus {
            TenFootDetailFocusId::Action(_) => Some((0.0, HERO_HEIGHT)),
            TenFootDetailFocusId::PanelItem { panel, .. } => {
                let panel_index = self.panel_index(panel)?;
                let top = HERO_HEIGHT
                    + PANEL_GAP
                    + panel_index as f32 * (panel_height() + PANEL_GAP);
                Some((top, panel_height()))
            }
        }
    }
}

pub fn is_tenfoot_detail_route(state: &State) -> bool {
    state.interface_mode.is_tenfoot()
        && state.is_authenticated
        && matches!(
            state.domains.ui.state.view,
            crate::domains::ui::types::ViewState::MovieDetail { .. }
                | crate::domains::ui::types::ViewState::SeriesDetail { .. }
                | crate::domains::ui::types::ViewState::SeasonDetail { .. }
                | crate::domains::ui::types::ViewState::EpisodeDetail { .. }
        )
}

pub fn update_tenfoot_detail(
    state: &mut State,
    message: TenFootDetailMessage,
) -> DomainUpdateResult {
    match message {
        TenFootDetailMessage::Move(direction) => {
            let data = TenFootDetailData::from_state(state);
            let current =
                state.domains.ui.state.tenfoot_detail.resolved_focus(&data);
            let columns =
                visible_panel_columns_for_width(state.window_size.width);
            let Some(next) =
                data.move_focus(current.as_ref(), direction, columns)
            else {
                return DomainUpdateResult::task(Task::none());
            };
            let fallback_height = state.window_size.height;
            let task = {
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(next.clone());
                detail.follow_focus_window(&data, &next, columns);
                detail.scroll_task_for_focus(&data, &next, fallback_height)
            };
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        TenFootDetailMessage::Focus(focus) => {
            let data = TenFootDetailData::from_state(state);
            if !data.contains_focus(&focus) {
                return DomainUpdateResult::task(Task::none());
            }
            let columns =
                visible_panel_columns_for_width(state.window_size.width);
            let fallback_height = state.window_size.height;
            let task = {
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(focus.clone());
                detail.follow_focus_window(&data, &focus, columns);
                detail.scroll_task_for_focus(&data, &focus, fallback_height)
            };
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        TenFootDetailMessage::ActivateFocused => {
            let data = TenFootDetailData::from_state(state);
            let focus =
                state.domains.ui.state.tenfoot_detail.resolved_focus(&data);
            let activation = focus
                .as_ref()
                .and_then(|focus| data.activation_for_focus(focus));
            DomainUpdateResult::task(task_for_activation(activation))
        }
        TenFootDetailMessage::Activate(focus) => {
            let data = TenFootDetailData::from_state(state);
            if !data.contains_focus(&focus) {
                return DomainUpdateResult::task(Task::none());
            }
            let activation = data.activation_for_focus(&focus);
            {
                let columns =
                    visible_panel_columns_for_width(state.window_size.width);
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(focus.clone());
                detail.follow_focus_window(&data, &focus, columns);
            }
            DomainUpdateResult::task(task_for_activation(activation))
        }
        TenFootDetailMessage::Back => DomainUpdateResult::task(Task::done(
            DomainMessage::Ui(UiShellMessage::NavigateBack.into()),
        )),
        TenFootDetailMessage::Scrolled(viewport) => {
            let offset = viewport.absolute_offset();
            let bounds = viewport.bounds();
            state.domains.ui.state.tenfoot_detail.scroll_y = offset.y;
            state.domains.ui.state.tenfoot_detail.viewport_height =
                bounds.height;
            state
                .domains
                .ui
                .state
                .background_shader_state
                .set_vertical_scroll_px(offset.y);
            DomainUpdateResult::task(Task::none())
        }
    }
}

pub fn view_tenfoot_detail(state: &State) -> Element<'_, UiMessage> {
    let data = TenFootDetailData::from_state(state);
    let detail_state = &state.domains.ui.state.tenfoot_detail;
    let focused = detail_state.resolved_focus(&data);
    let columns = visible_panel_columns_for_width(state.window_size.width);

    let mut panels: Column<'_, UiMessage> = column![].spacing(PANEL_GAP);
    for panel in &data.panels {
        panels = panels.push(view_panel(
            state,
            detail_state,
            panel,
            focused.as_ref(),
            columns,
        ));
    }

    let content = column![view_hero(state, &data, focused.as_ref()), panels]
        .spacing(PANEL_GAP)
        .padding([PAGE_PADDING_Y, PAGE_PADDING_X])
        .width(Length::Fill);

    let scroll = scrollable(content)
        .id(detail_state.scrollable_id.clone())
        .on_scroll(|viewport| TenFootDetailMessage::Scrolled(viewport).into())
        .width(Length::Fill)
        .height(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(tenfoot_detail_page_style())
        .into()
}

fn view_hero<'a>(
    state: &'a State,
    data: &TenFootDetailData,
    focused: Option<&TenFootDetailFocusId>,
) -> Element<'a, UiMessage> {
    let image = view_detail_image(state, &data.image, true);

    let mut metadata_row: Row<'a, UiMessage> = Row::new().spacing(12);
    for item in &data.metadata {
        metadata_row = metadata_row.push(
            container(
                text(item.clone())
                    .size(20)
                    .color(theme::MediaServerTheme::TEXT_PRIMARY),
            )
            .padding([6, 12])
            .style(metadata_pill_style()),
        );
    }

    let mut actions = Row::new().spacing(16);
    for spec in &data.actions {
        let focus_id = TenFootDetailFocusId::Action(spec.action);
        let is_focused = focused == Some(&focus_id);
        actions =
            actions.push(focusable_action_button(spec, focus_id, is_focused));
    }

    let mut text_column = column![
        text(data.eyebrow.clone())
            .size(22)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(data.title.clone())
            .size(56)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(data.subtitle.clone())
            .size(24)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        metadata_row,
        text(data.overview.clone())
            .size(22)
            .color(theme::MediaServerTheme::TEXT_PRIMARY)
            .width(Length::Fill),
        actions,
    ]
    .spacing(15)
    .width(Length::Fill);

    if let Some(notice) = data.notice.as_ref() {
        text_column = text_column.push(
            container(
                text(notice.clone())
                    .size(19)
                    .color(theme::MediaServerTheme::TEXT_SECONDARY),
            )
            .padding(14)
            .width(Length::Fill)
            .style(notice_style()),
        );
    }

    container(
        row![image, text_column]
            .spacing(38)
            .align_y(iced::Alignment::Center),
    )
    .height(Length::Fixed(HERO_HEIGHT))
    .width(Length::Fill)
    .padding(30)
    .style(tenfoot_panel_style(false))
    .into()
}

fn focusable_action_button<'a>(
    spec: &DetailActionSpec,
    focus_id: TenFootDetailFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    let content = column![
        text(spec.label.clone())
            .size(25)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(spec.subtitle.clone())
            .size(15)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(4)
    .align_x(iced::Alignment::Center);

    let button_element = button(content)
        .padding([10, 18])
        .width(Length::Fixed(ACTION_WIDTH))
        .height(Length::Fixed(ACTION_HEIGHT))
        .style(tenfoot_button_style(focused))
        .on_press(TenFootDetailMessage::Activate(focus_id.clone()).into());

    mouse_area(button_element)
        .on_enter(TenFootDetailMessage::Focus(focus_id).into())
        .into()
}

fn view_panel<'a>(
    state: &'a State,
    detail_state: &TenFootDetailState,
    panel: &TenFootDetailPanel,
    focused: Option<&TenFootDetailFocusId>,
    columns: usize,
) -> Element<'a, UiMessage> {
    let total = panel.items.len();
    let focused_index = focused.and_then(|focus| match focus {
        TenFootDetailFocusId::PanelItem {
            panel: focused_panel,
            item,
        } if *focused_panel == panel.id => panel
            .items
            .iter()
            .position(|candidate| candidate.id() == *item),
        _ => None,
    });
    let start = bounded_two_row_window_start(
        detail_state.panel_window_start(&panel.id, total, columns),
        focused_index,
        total,
        columns,
    );
    let visible_count = columns.max(1) * PANEL_ROWS;
    let end = (start + visible_count).min(total);

    let range_label = if total == 0 {
        "0".to_string()
    } else {
        format!("{}–{} of {}", start + 1, end, total)
    };

    let before_after = before_after_copy(start, end, total);
    let header = row![
        text(panel.id.title())
            .size(31)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(range_label)
            .size(18)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(before_after)
            .size(18)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(18)
    .align_y(iced::Alignment::Center);

    let body: Element<'a, UiMessage> = if panel.items.is_empty() {
        container(
            text(panel.empty_message.clone())
                .size(22)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .width(Length::Fill)
        .height(Length::Fixed(panel_body_height()))
        .padding(28)
        .align_y(iced::Alignment::Center)
        .style(tenfoot_panel_style(false))
        .into()
    } else {
        let visible_items = panel
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_count)
            .collect::<Vec<_>>();

        let mut grid = Column::new().spacing(PANEL_ROW_GAP);
        for row_items in visible_items.chunks(columns.max(1)) {
            let mut row = Row::new().spacing(PANEL_CARD_GAP);
            for (index, item) in row_items.iter().copied() {
                let focus_id = TenFootDetailFocusId::PanelItem {
                    panel: panel.id.clone(),
                    item: item.id(),
                };
                let is_focused = focused == Some(&focus_id);
                row = row.push(view_panel_card(
                    state, item, focus_id, is_focused, index,
                ));
            }
            grid = grid.push(row);
        }

        container(grid)
            .width(Length::Fill)
            .height(Length::Fixed(panel_body_height()))
            .padding(18)
            .style(tenfoot_panel_style(false))
            .into()
    };

    column![header, body].spacing(12).width(Length::Fill).into()
}

fn view_panel_card<'a>(
    state: &'a State,
    item: &TenFootDetailPanelItem,
    focus_id: TenFootDetailFocusId,
    focused: bool,
    _index: usize,
) -> Element<'a, UiMessage> {
    let image = view_panel_item_image(state, &item.image(), focused);
    let content = row![
        image,
        column![
            text(item.title().to_string())
                .size(21)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(item.subtitle().to_string())
                .size(16)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(item.context().to_string())
                .size(15)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(6)
        .width(Length::Fill),
    ]
    .spacing(14)
    .align_y(iced::Alignment::Center);

    let button_element = button(
        container(content)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .padding(0)
    .width(Length::Fixed(PANEL_CARD_WIDTH))
    .height(Length::Fixed(PANEL_ROW_HEIGHT))
    .style(tenfoot_button_style(focused))
    .on_press(TenFootDetailMessage::Activate(focus_id.clone()).into());

    mouse_area(button_element)
        .on_enter(TenFootDetailMessage::Focus(focus_id).into())
        .into()
}

fn view_detail_image<'a>(
    state: &'a State,
    image: &DetailImage,
    priority_visible: bool,
) -> Element<'a, UiMessage> {
    match image {
        DetailImage::Poster {
            media_uuid,
            iid,
            placeholder,
        } => image_for(*media_uuid)
            .iid(*iid)
            .skip_request(iid.is_none())
            .request_size(ImageSize::Poster(
                state.domains.settings.display.detail_poster_quality,
            ))
            .display_size(HERO_IMAGE_WIDTH, HERO_POSTER_HEIGHT)
            .radius(18.0)
            .priority(if priority_visible {
                Priority::Visible
            } else {
                Priority::Preload
            })
            .placeholder(*placeholder)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailImage::Still { media_uuid, iid } => image_for(*media_uuid)
            .iid(*iid)
            .skip_request(iid.is_none())
            .request_size(ImageSize::thumbnail())
            .display_size(HERO_STILL_WIDTH, HERO_STILL_HEIGHT)
            .radius(18.0)
            .priority(if priority_visible {
                Priority::Visible
            } else {
                Priority::Preload
            })
            .placeholder(lucide_icons::Icon::Clapperboard)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailImage::None => container(
            text("No local image")
                .size(24)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .width(Length::Fixed(HERO_IMAGE_WIDTH))
        .height(Length::Fixed(HERO_POSTER_HEIGHT))
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(tenfoot_panel_style(false))
        .into(),
    }
}

fn view_panel_item_image<'a>(
    state: &'a State,
    image: &DetailImage,
    focused: bool,
) -> Element<'a, UiMessage> {
    match image {
        DetailImage::Poster {
            media_uuid,
            iid,
            placeholder,
        } => image_for(*media_uuid)
            .iid(*iid)
            .skip_request(iid.is_none())
            .request_size(ImageSize::Poster(
                state.domains.settings.display.library_poster_quality,
            ))
            .display_size(74.0, 110.0)
            .radius(10.0)
            .priority(if focused {
                Priority::Visible
            } else {
                Priority::Preload
            })
            .placeholder(*placeholder)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailImage::Still { media_uuid, iid } => image_for(*media_uuid)
            .iid(*iid)
            .skip_request(iid.is_none())
            .request_size(ImageSize::thumbnail())
            .display_size(118.0, 66.0)
            .radius(10.0)
            .priority(if focused {
                Priority::Visible
            } else {
                Priority::Preload
            })
            .placeholder(lucide_icons::Icon::Clapperboard)
            .tight_bounds()
            .no_animation()
            .into(),
        DetailImage::None => Space::new()
            .width(Length::Fixed(74.0))
            .height(Length::Fixed(110.0))
            .into(),
    }
}

fn action_specs(
    primary: Option<TenFootDetailActivation>,
    primary_label: &'static str,
    primary_subtitle: &'static str,
    start_over: Option<TenFootDetailActivation>,
) -> Vec<DetailActionSpec> {
    let mut actions = Vec::new();
    if let Some(activation) = primary {
        actions.push(DetailActionSpec {
            action: TenFootDetailAction::Primary,
            label: primary_label.to_string(),
            subtitle: primary_subtitle.to_string(),
            activation,
        });
    }
    if let Some(activation) = start_over {
        actions.push(DetailActionSpec {
            action: TenFootDetailAction::StartOver,
            label: "Start Over".to_string(),
            subtitle: "Play from 0:00".to_string(),
            activation,
        });
    }
    actions.push(DetailActionSpec {
        action: TenFootDetailAction::Back,
        label: "Back".to_string(),
        subtitle: "Return to the previous screen".to_string(),
        activation: TenFootDetailActivation::Back,
    });
    actions
}

fn task_for_activation(
    activation: Option<TenFootDetailActivation>,
) -> Task<DomainMessage> {
    match activation {
        Some(TenFootDetailActivation::PlayMedia(media_id)) => {
            Task::done(DomainMessage::Ui(
                PlaybackMessage::PlayMediaWithId(media_id).into(),
            ))
        }
        Some(TenFootDetailActivation::PlayMediaFromStart(media_id)) => {
            Task::done(DomainMessage::Ui(
                PlaybackMessage::PlayMediaWithIdFromStart(media_id).into(),
            ))
        }
        Some(TenFootDetailActivation::PlaySeriesNextEpisode(series_id)) => {
            Task::done(DomainMessage::Ui(
                PlaybackMessage::PlaySeriesNextEpisode(series_id).into(),
            ))
        }
        Some(TenFootDetailActivation::ViewSeason(series_id, season_id)) => {
            Task::done(DomainMessage::Ui(
                UiShellMessage::ViewSeason(series_id, season_id).into(),
            ))
        }
        Some(TenFootDetailActivation::ViewEpisode(episode_id)) => Task::done(
            DomainMessage::Ui(UiShellMessage::ViewEpisode(episode_id).into()),
        ),
        Some(TenFootDetailActivation::Back) => {
            Task::done(DomainMessage::Ui(UiShellMessage::NavigateBack.into()))
        }
        None => Task::none(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TenFootWatchInfo {
    pub has_watch_state: bool,
    pub in_progress: bool,
    pub position: f32,
    pub duration: f32,
}

impl TenFootWatchInfo {
    fn progress_label(self) -> Option<String> {
        if self.in_progress && self.duration > 0.0 {
            Some(format!(
                "Resume at {} • {}% watched",
                timestamp_label(self.position),
                ((self.position / self.duration).clamp(0.0, 1.0) * 100.0)
                    as u32
            ))
        } else if self.has_watch_state {
            Some("Watched".to_string())
        } else {
            None
        }
    }
}

fn watch_info_for_media(state: &State, media_id: &MediaID) -> TenFootWatchInfo {
    let Some(watch_state) = &state.domains.media.state.user_watch_state else {
        return TenFootWatchInfo::default();
    };

    if let Some(item) = watch_state.get_by_media_id(media_id.as_uuid()) {
        return TenFootWatchInfo {
            has_watch_state: true,
            in_progress: item.position > 0.0 && item.duration > 0.0,
            position: item.position,
            duration: item.duration,
        };
    }

    TenFootWatchInfo {
        has_watch_state: watch_state.completed.contains(media_id.as_uuid()),
        in_progress: false,
        position: 0.0,
        duration: 0.0,
    }
}

pub fn primary_label_for_watch_info(watch: TenFootWatchInfo) -> &'static str {
    if watch.in_progress { "Resume" } else { "Play" }
}

pub fn start_over_available_for_watch_info(watch: TenFootWatchInfo) -> bool {
    watch.has_watch_state
}

fn season_panel_item(
    season: &ferrex_core::player_prelude::SeasonReference,
) -> TenFootDetailPanelItem {
    let number = season.season_number.value();
    let title = if number == 0 {
        "Specials".to_string()
    } else if season.details.name.is_empty() {
        format!("Season {number}")
    } else {
        season.details.name.clone()
    };

    TenFootDetailPanelItem::Season(SeasonPanelItem {
        id: season.id,
        series_id: season.series_id,
        title,
        subtitle: if number == 0 {
            "Specials".to_string()
        } else {
            format!("Season {number}")
        },
        context: plural_label(
            season.details.episode_count as usize,
            "episode",
            "episodes",
        ),
        poster_iid: season.details.primary_poster_iid,
    })
}

fn episode_panel_item(
    episode: &ferrex_core::player_prelude::EpisodeReference,
) -> TenFootDetailPanelItem {
    TenFootDetailPanelItem::Episode(EpisodePanelItem {
        id: episode.id,
        title: if episode.details.name.is_empty() {
            format!("Episode {}", episode.episode_number.value())
        } else {
            episode.details.name.clone()
        },
        subtitle: format!(
            "S{:02}E{:02}",
            episode.season_number.value(),
            episode.episode_number.value()
        ),
        context: episode
            .details
            .runtime
            .map(|runtime| format!("{runtime} min • Open details"))
            .unwrap_or_else(|| "Open details".to_string()),
        still_iid: episode.details.primary_still_iid,
    })
}

fn season_title(
    season: &ferrex_core::player_prelude::SeasonReference,
) -> String {
    let number = season.season_number.value();
    if number == 0 {
        "Specials".to_string()
    } else {
        format!("Season {number}")
    }
}

fn runtime_label(minutes: u32) -> String {
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn duration_label(seconds: f64) -> String {
    let total_minutes = (seconds / 60.0).round().max(0.0) as u32;
    runtime_label(total_minutes)
}

fn timestamp_label(seconds: f32) -> String {
    let total = seconds.max(0.0).round() as u32;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    if hours > 0 {
        format!("{}h {:02}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

fn plural_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn before_after_copy(start: usize, end: usize, total: usize) -> String {
    match (start > 0, end < total) {
        (true, true) => format!("{} before • {} after", start, total - end),
        (true, false) => format!("{} before", start),
        (false, true) => format!("{} after", total - end),
        (false, false) => "All visible".to_string(),
    }
}

fn panel_body_height() -> f32 {
    PANEL_ROWS as f32 * PANEL_ROW_HEIGHT
        + (PANEL_ROWS.saturating_sub(1)) as f32 * PANEL_ROW_GAP
        + 36.0
}

fn panel_height() -> f32 {
    PANEL_HEADER_HEIGHT + 12.0 + panel_body_height()
}

pub fn visible_panel_columns_for_width(width: f32) -> usize {
    let available = (width - PAGE_PADDING_X * 2.0).max(PANEL_CARD_WIDTH);
    let per_card = PANEL_CARD_WIDTH + PANEL_CARD_GAP;
    ((available + PANEL_CARD_GAP) / per_card).floor().max(1.0) as usize
}

pub fn bounded_two_row_window_start(
    current_start: usize,
    focused_index: Option<usize>,
    total: usize,
    columns: usize,
) -> usize {
    if total == 0 {
        return 0;
    }

    let columns = columns.max(1);
    let visible_count = (columns * PANEL_ROWS).min(total).max(1);
    let max_start = total.saturating_sub(visible_count);
    let mut start = current_start.min(max_start);
    start = (start / columns) * columns;

    if let Some(index) = focused_index.map(|idx| idx.min(total - 1)) {
        if index < start {
            start = (index / columns) * columns;
        } else if index >= start + visible_count {
            let focus_row = index / columns;
            start = focus_row.saturating_add(1).saturating_sub(PANEL_ROWS)
                * columns;
        }
    }

    start.min(max_start)
}

fn tenfoot_detail_page_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgba(
            0.015, 0.014, 0.02, 0.82,
        ))),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn tenfoot_panel_style(
    focused: bool,
) -> impl Fn(&Theme) -> container::Style + Clone {
    move |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(if focused {
            Color::from_rgba(0.18, 0.08, 0.20, 0.92)
        } else {
            Color::from_rgba(0.08, 0.075, 0.10, 0.84)
        })),
        border: Border {
            color: if focused {
                theme::MediaServerTheme::ACCENT
            } else {
                Color::from_rgba(1.0, 1.0, 1.0, 0.10)
            },
            width: if focused { 3.0 } else { 1.0 },
            radius: 24.0.into(),
        },
        shadow: if focused {
            Shadow {
                color: theme::MediaServerTheme::ACCENT_GLOW,
                offset: Vector::new(0.0, 0.0),
                blur_radius: 28.0,
            }
        } else {
            Shadow::default()
        },
        snap: false,
    }
}

fn metadata_pill_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgba(
            1.0, 1.0, 1.0, 0.10,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.14),
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn notice_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgba(
            0.12, 0.10, 0.04, 0.72,
        ))),
        border: Border {
            color: Color::from_rgba(1.0, 0.85, 0.30, 0.35),
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn tenfoot_button_style(
    focused: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_, status| {
        let hovered =
            matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if focused {
            Color::from_rgba(0.30, 0.06, 0.34, 0.96)
        } else if hovered {
            Color::from_rgba(0.18, 0.08, 0.20, 0.94)
        } else {
            Color::from_rgba(0.09, 0.085, 0.11, 0.92)
        };
        let border_color = if focused {
            theme::MediaServerTheme::ACCENT
        } else if hovered {
            Color::from_rgba(1.0, 1.0, 1.0, 0.28)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.12)
        };

        button::Style {
            text_color: theme::MediaServerTheme::TEXT_PRIMARY,
            background: Some(Background::Color(background)),
            border: Border {
                color: border_color,
                width: if focused { 3.0 } else { 1.0 },
                radius: 22.0.into(),
            },
            shadow: if focused {
                Shadow {
                    color: theme::MediaServerTheme::ACCENT_GLOW,
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 24.0,
                }
            } else {
                Shadow::default()
            },
            snap: false,
        }
    }
}
