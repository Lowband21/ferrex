use std::collections::HashMap;

use crate::{
    common::{
        focus::SpatialDirection,
        messages::{DomainMessage, DomainUpdateResult},
    },
    domains::{
        media::selectors,
        ui::{
            messages::UiMessage,
            playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage,
            theme,
            views::detail::{
                DetailArtAspect, DetailInterfaceMode, DetailLayoutInput,
                DetailLayoutPlan, solve_detail_layout,
            },
            widgets::image_for,
        },
    },
    infra::shader_widgets::poster::{
        PosterFace, PosterInstanceKey, animation::AnimationBehavior,
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

const TENFOOT_HEADER_HEIGHT: f32 = 0.0;
const HERO_STILL_ASPECT: f32 = 16.0 / 9.0;
const HERO_STILL_MAX_CONTENT_FRACTION: f32 = 0.42;
const PANEL_POSTER_ASPECT: f32 = 2.0 / 3.0;
const PANEL_STILL_ASPECT: f32 = 16.0 / 9.0;
const TWO_ROW_PANEL_ROWS: usize = 2;

fn tenfoot_detail_layout_plan(state: &State) -> DetailLayoutPlan {
    solve_detail_layout(
        DetailLayoutInput::from_runtime(
            state.window_size.width,
            state.window_size.height,
            TENFOOT_HEADER_HEIGHT,
            DetailInterfaceMode::TenFoot,
            &state.domains.ui.state.size_provider,
            &state.domains.ui.state.scaled_layout,
        )
        .with_hero_art_aspect(tenfoot_detail_art_aspect(state)),
    )
}

fn tenfoot_detail_art_aspect(state: &State) -> DetailArtAspect {
    if matches!(
        state.domains.ui.state.view,
        crate::domains::ui::types::ViewState::EpisodeDetail { .. }
    ) {
        DetailArtAspect::Still
    } else {
        DetailArtAspect::Poster
    }
}

fn hero_padding(plan: &DetailLayoutPlan) -> f32 {
    (plan.page_padding_y * 0.55)
        .min(plan.available_height * 0.028)
        .clamp(16.0, 30.0)
}

fn hero_height(plan: &DetailLayoutPlan) -> f32 {
    plan.backdrop
        .height
        .max(plan.hero_art.height + hero_padding(plan) * 2.0)
}

fn panel_rows(plan: &DetailLayoutPlan) -> usize {
    plan.rail.visible_rows.max(1)
}

fn panel_body_padding(plan: &DetailLayoutPlan) -> f32 {
    (plan.rail.gap * 1.2).clamp(14.0, 24.0)
}

fn panel_header_height(plan: &DetailLayoutPlan) -> f32 {
    (plan.action_cluster.button_height * 0.67).clamp(38.0, 54.0)
}

fn panel_header_body_gap(plan: &DetailLayoutPlan) -> f32 {
    (plan.section_grid.gap * 0.66).clamp(10.0, 18.0)
}

fn panel_body_height(plan: &DetailLayoutPlan) -> f32 {
    let rows = panel_rows(plan);
    rows as f32 * plan.rail.card_height
        + rows.saturating_sub(1) as f32 * plan.rail.gap
        + panel_body_padding(plan) * 2.0
}

fn panel_height(plan: &DetailLayoutPlan) -> f32 {
    panel_header_height(plan)
        + panel_header_body_gap(plan)
        + panel_body_height(plan)
}

fn scroll_follow_margin(plan: &DetailLayoutPlan) -> f32 {
    (plan.page_padding_y * 0.82).clamp(28.0, 56.0)
}

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
        rows: usize,
    ) -> usize {
        bounded_panel_window_start(
            *self.panel_windows.get(panel).unwrap_or(&0),
            None,
            total,
            columns,
            rows,
        )
    }

    fn follow_focus_window(
        &mut self,
        data: &TenFootDetailData,
        focus: &TenFootDetailFocusId,
        columns: usize,
        rows: usize,
    ) {
        let Some((panel, index, total)) = data.panel_item_position(focus)
        else {
            return;
        };

        let current = *self.panel_windows.get(&panel).unwrap_or(&0);
        let next = bounded_panel_window_start(
            current,
            Some(index),
            total,
            columns.max(1),
            rows.max(1),
        );
        self.panel_windows.insert(panel, next);
    }

    fn scroll_task_for_focus(
        &mut self,
        data: &TenFootDetailData,
        focus: &TenFootDetailFocusId,
        plan: &DetailLayoutPlan,
        fallback_height: f32,
    ) -> Task<UiMessage> {
        let Some((top, height)) = data.focus_vertical_bounds(focus, plan)
        else {
            return Task::none();
        };

        let viewport_height =
            self.viewport_height.max(fallback_height).max(1.0);
        let visible_top = self.scroll_y;
        let visible_bottom = visible_top + viewport_height;
        let margin = scroll_follow_margin(plan);
        let target = if top < visible_top + margin {
            (top - margin).max(0.0)
        } else if top + height > visible_bottom - margin {
            (top + height + margin - viewport_height).max(0.0)
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
        plan: &DetailLayoutPlan,
    ) -> Option<(f32, f32)> {
        match focus {
            TenFootDetailFocusId::Action(_) => {
                Some((plan.page_padding_y, hero_height(plan)))
            }
            TenFootDetailFocusId::PanelItem { panel, .. } => {
                let panel_index = self.panel_index(panel)?;
                let panel_height = panel_height(plan);
                let top = plan.page_padding_y
                    + hero_height(plan)
                    + plan.hero_gap
                    + panel_index as f32 * (panel_height + plan.hero_gap);
                Some((top, panel_height))
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
            let plan = tenfoot_detail_layout_plan(state);
            let current =
                state.domains.ui.state.tenfoot_detail.resolved_focus(&data);
            let columns =
                visible_panel_columns_for_width(state.window_size.width, &plan);
            let rows = panel_rows(&plan);
            let Some(next) =
                data.move_focus(current.as_ref(), direction, columns)
            else {
                return DomainUpdateResult::task(Task::none());
            };
            let fallback_height = state.window_size.height;
            let task = {
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(next.clone());
                detail.follow_focus_window(&data, &next, columns, rows);
                detail.scroll_task_for_focus(
                    &data,
                    &next,
                    &plan,
                    fallback_height,
                )
            };
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        TenFootDetailMessage::Focus(focus) => {
            let data = TenFootDetailData::from_state(state);
            if !data.contains_focus(&focus) {
                return DomainUpdateResult::task(Task::none());
            }
            let plan = tenfoot_detail_layout_plan(state);
            let columns =
                visible_panel_columns_for_width(state.window_size.width, &plan);
            let rows = panel_rows(&plan);
            let fallback_height = state.window_size.height;
            let task = {
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(focus.clone());
                detail.follow_focus_window(&data, &focus, columns, rows);
                detail.scroll_task_for_focus(
                    &data,
                    &focus,
                    &plan,
                    fallback_height,
                )
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
                let plan = tenfoot_detail_layout_plan(state);
                let columns = visible_panel_columns_for_width(
                    state.window_size.width,
                    &plan,
                );
                let rows = panel_rows(&plan);
                let detail = &mut state.domains.ui.state.tenfoot_detail;
                detail.focus_id = Some(focus.clone());
                detail.follow_focus_window(&data, &focus, columns, rows);
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
    let plan = tenfoot_detail_layout_plan(state);
    let columns =
        visible_panel_columns_for_width(state.window_size.width, &plan);

    let mut panels: Column<'_, UiMessage> = column![].spacing(plan.hero_gap);
    for panel in &data.panels {
        panels = panels.push(view_panel(
            state,
            detail_state,
            panel,
            focused.as_ref(),
            &plan,
            columns,
        ));
    }

    let stage =
        column![view_hero(state, &data, focused.as_ref(), &plan), panels]
            .spacing(plan.hero_gap)
            .width(Length::Fixed(plan.content_width));

    let content = container(stage)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding([plan.page_padding_y, plan.page_padding_x]);

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
    plan: &DetailLayoutPlan,
) -> Element<'a, UiMessage> {
    let image = view_detail_image(state, &data.image, plan, true);

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

    let mut actions = Row::new().spacing(plan.action_cluster.gap);
    for spec in &data.actions {
        let focus_id = TenFootDetailFocusId::Action(spec.action);
        let is_focused = focused == Some(&focus_id);
        actions = actions
            .push(focusable_action_button(spec, focus_id, is_focused, plan));
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
            .spacing(plan.hero_gap)
            .align_y(iced::Alignment::End),
    )
    .height(Length::Fixed(hero_height(plan)))
    .width(Length::Fill)
    .padding(hero_padding(plan))
    .style(tenfoot_hero_scrim_style())
    .into()
}

fn focusable_action_button<'a>(
    spec: &DetailActionSpec,
    focus_id: TenFootDetailFocusId,
    focused: bool,
    plan: &DetailLayoutPlan,
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
        .width(Length::Fixed(plan.action_cluster.button_width))
        .height(Length::Fixed(plan.action_cluster.button_height))
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
    plan: &DetailLayoutPlan,
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
    let rows = panel_rows(plan);
    let start = bounded_panel_window_start(
        detail_state.panel_window_start(&panel.id, total, columns, rows),
        focused_index,
        total,
        columns,
        rows,
    );
    let visible_count = columns.max(1) * rows;
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
    .height(Length::Fixed(panel_header_height(plan)))
    .align_y(iced::Alignment::Center);

    let body: Element<'a, UiMessage> = if panel.items.is_empty() {
        container(
            text(panel.empty_message.clone())
                .size(22)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .width(Length::Fill)
        .height(Length::Fixed(panel_body_height(plan)))
        .padding(panel_body_padding(plan))
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

        let mut grid = Column::new().spacing(plan.rail.gap);
        for row_items in visible_items.chunks(columns.max(1)) {
            let mut row = Row::new().spacing(plan.rail.gap);
            for (index, item) in row_items.iter().copied() {
                let focus_id = TenFootDetailFocusId::PanelItem {
                    panel: panel.id.clone(),
                    item: item.id(),
                };
                let is_focused = focused == Some(&focus_id);
                row = row.push(view_panel_card(
                    state, item, focus_id, is_focused, index, plan,
                ));
            }
            grid = grid.push(row);
        }

        container(grid)
            .width(Length::Fill)
            .height(Length::Fixed(panel_body_height(plan)))
            .padding(panel_body_padding(plan))
            .style(tenfoot_panel_style(false))
            .into()
    };

    column![header, body]
        .spacing(panel_header_body_gap(plan))
        .width(Length::Fill)
        .into()
}

fn view_panel_card<'a>(
    state: &'a State,
    item: &TenFootDetailPanelItem,
    focus_id: TenFootDetailFocusId,
    focused: bool,
    _index: usize,
    plan: &DetailLayoutPlan,
) -> Element<'a, UiMessage> {
    let image = view_panel_item_image(state, &item.image(), plan, focused);
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
    .spacing(plan.rail.gap.min(16.0))
    .align_y(iced::Alignment::Center);

    let button_element = button(
        container(content)
            .padding((plan.rail.gap * 0.9).clamp(10.0, 16.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
    )
    .padding(0)
    .width(Length::Fixed(plan.rail.card_width))
    .height(Length::Fixed(plan.rail.card_height))
    .style(tenfoot_button_style(focused))
    .on_press(TenFootDetailMessage::Activate(focus_id.clone()).into());

    mouse_area(button_element)
        .on_enter(TenFootDetailMessage::Focus(focus_id).into())
        .into()
}

fn hero_image_size(image: &DetailImage, plan: &DetailLayoutPlan) -> (f32, f32) {
    match image {
        DetailImage::Still { .. } => {
            let max_width = (plan.content_width
                * HERO_STILL_MAX_CONTENT_FRACTION)
                .max(plan.hero_art.width)
                .max(1.0);
            let desired_height = (plan.hero_art.height * 0.70).max(1.0);
            let desired_width = desired_height * HERO_STILL_ASPECT;
            if desired_width > max_width {
                (max_width, max_width / HERO_STILL_ASPECT)
            } else {
                (desired_width, desired_height)
            }
        }
        DetailImage::Poster { .. } | DetailImage::None => {
            (plan.hero_art.width, plan.hero_art.height)
        }
    }
}

fn panel_image_size(
    image: &DetailImage,
    plan: &DetailLayoutPlan,
) -> (f32, f32) {
    match image {
        DetailImage::Still { .. } => {
            let width = (plan.rail.card_height * 0.76).max(72.0);
            (width, width / PANEL_STILL_ASPECT)
        }
        DetailImage::Poster { .. } | DetailImage::None => {
            let height = (plan.rail.card_height * 0.72)
                .min((plan.rail.card_height - 16.0).max(1.0))
                .max(48.0);
            (height * PANEL_POSTER_ASPECT, height)
        }
    }
}

fn view_detail_image<'a>(
    state: &'a State,
    image: &DetailImage,
    plan: &DetailLayoutPlan,
    priority_visible: bool,
) -> Element<'a, UiMessage> {
    let (width, height) = hero_image_size(image, plan);
    match image {
        DetailImage::Poster {
            media_uuid,
            iid,
            placeholder,
        } => {
            let (face, rotation_y) = poster_menu_face(state, *media_uuid);
            let mut poster = image_for(*media_uuid)
                .iid(*iid)
                .skip_request(iid.is_none())
                .request_size(ImageSize::Poster(
                    state.domains.settings.display.detail_poster_quality,
                ))
                .display_size(width, height)
                .radius(plan.hero_art.corner_radius)
                .priority(if priority_visible {
                    Priority::Visible
                } else {
                    Priority::Preload
                })
                .placeholder(*placeholder)
                .tight_bounds()
                .animation_behavior(AnimationBehavior::flip_then_fade())
                .face(face);

            if let Some(rotation_y) = rotation_y {
                poster = poster.rotation_y(rotation_y);
            }

            poster.into()
        }
        DetailImage::Still { media_uuid, iid } => image_for(*media_uuid)
            .iid(*iid)
            .skip_request(iid.is_none())
            .request_size(ImageSize::thumbnail())
            .display_size(width, height)
            .radius(plan.hero_art.corner_radius)
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
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(tenfoot_panel_style(false))
        .into(),
    }
}

fn view_panel_item_image<'a>(
    state: &'a State,
    image: &DetailImage,
    plan: &DetailLayoutPlan,
    focused: bool,
) -> Element<'a, UiMessage> {
    let (width, height) = panel_image_size(image, plan);
    let radius = (plan.hero_art.corner_radius * 0.55).clamp(2.0, 6.0);
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
            .display_size(width, height)
            .radius(radius)
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
            .display_size(width, height)
            .radius(radius)
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
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into(),
    }
}

fn poster_menu_face(
    state: &State,
    poster_id: Uuid,
) -> (PosterFace, Option<f32>) {
    let instance_key = PosterInstanceKey::standalone(poster_id);
    if let Some(menu_state) =
        state.domains.ui.state.poster_menu_states.get(&instance_key)
    {
        (menu_state.face_from_angle(), Some(menu_state.angle))
    } else if state.domains.ui.state.poster_menu_open.as_ref()
        == Some(&instance_key)
    {
        (PosterFace::Back, Some(std::f32::consts::PI))
    } else {
        (PosterFace::Front, None)
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

pub fn visible_panel_columns_for_width(
    width: f32,
    plan: &DetailLayoutPlan,
) -> usize {
    let available = (width - plan.page_padding_x * 2.0)
        .max(1.0)
        .min(plan.content_width);
    let card_width = plan.rail.card_width.max(1.0);
    let gap = plan.rail.gap.max(0.0);
    ((available + gap) / (card_width + gap)).floor().max(1.0) as usize
}

pub fn bounded_panel_window_start(
    current_start: usize,
    focused_index: Option<usize>,
    total: usize,
    columns: usize,
    rows: usize,
) -> usize {
    if total == 0 {
        return 0;
    }

    let columns = columns.max(1);
    let rows = rows.max(1);
    let visible_count = (columns * rows).min(total).max(1);
    let max_start = total.saturating_sub(visible_count);
    let mut start = current_start.min(max_start);
    start = (start / columns) * columns;

    if let Some(index) = focused_index.map(|idx| idx.min(total - 1)) {
        if index < start {
            start = (index / columns) * columns;
        } else if index >= start + visible_count {
            let focus_row = index / columns;
            start = focus_row.saturating_add(1).saturating_sub(rows) * columns;
        }
    }

    start.min(max_start)
}

pub fn bounded_two_row_window_start(
    current_start: usize,
    focused_index: Option<usize>,
    total: usize,
    columns: usize,
) -> usize {
    bounded_panel_window_start(
        current_start,
        focused_index,
        total,
        columns,
        TWO_ROW_PANEL_ROWS,
    )
}

fn tenfoot_detail_page_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgba(
            0.015, 0.014, 0.02, 0.22,
        ))),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn tenfoot_hero_scrim_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgba(
            0.0, 0.0, 0.0, 0.48,
        ))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
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
            radius: 3.0.into(),
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
            radius: 3.0.into(),
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
            radius: 2.0.into(),
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
                radius: 4.0.into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{
        constants::layout::{calculations::ScaledLayout, grid},
        design_tokens::{ScalingContext, SizeProvider},
    };

    fn layout_plan(width: f32, height: f32, scale: f32) -> DetailLayoutPlan {
        let sizes =
            SizeProvider::new(ScalingContext::new().with_user_scale(scale));
        let layout = ScaledLayout::new(sizes.scale, grid::EFFECTIVE_SPACING);
        solve_detail_layout(DetailLayoutInput::from_runtime(
            width,
            height,
            TENFOOT_HEADER_HEIGHT,
            DetailInterfaceMode::TenFoot,
            &sizes,
            &layout,
        ))
    }

    fn season_id(value: u128) -> SeasonID {
        SeasonID(Uuid::from_u128(value))
    }

    fn episode_id(value: u128) -> EpisodeID {
        EpisodeID(Uuid::from_u128(value))
    }

    fn action_spec(action: TenFootDetailAction) -> DetailActionSpec {
        DetailActionSpec {
            action,
            label: format!("{action:?}"),
            subtitle: "Test action".to_string(),
            activation: TenFootDetailActivation::Back,
        }
    }

    fn episode_item(index: usize) -> TenFootDetailPanelItem {
        TenFootDetailPanelItem::Episode(EpisodePanelItem {
            id: episode_id(index as u128 + 1),
            title: format!("Episode {index}"),
            subtitle: format!("E{index}"),
            context: "Open details".to_string(),
            still_iid: None,
        })
    }

    fn detail_data(panel_lengths: &[usize]) -> TenFootDetailData {
        let panels = panel_lengths
            .iter()
            .enumerate()
            .map(|(panel_index, item_count)| TenFootDetailPanel {
                id: if panel_index == 0 {
                    TenFootDetailPanelId::SeasonEpisodes(season_id(1))
                } else {
                    TenFootDetailPanelId::EpisodeSiblings(season_id(
                        panel_index as u128 + 1,
                    ))
                },
                empty_message: "No episodes".to_string(),
                items: (0..*item_count)
                    .map(|item_index| {
                        episode_item(panel_index * 100 + item_index)
                    })
                    .collect(),
            })
            .collect();

        TenFootDetailData {
            eyebrow: "Details".to_string(),
            title: "Focus Fixture".to_string(),
            subtitle: "Fixture".to_string(),
            metadata: Vec::new(),
            overview: "Overview".to_string(),
            image: DetailImage::None,
            actions: vec![
                action_spec(TenFootDetailAction::Primary),
                action_spec(TenFootDetailAction::StartOver),
                action_spec(TenFootDetailAction::Back),
            ],
            panels,
            notice: None,
        }
    }

    fn panel_focus(
        data: &TenFootDetailData,
        panel_index: usize,
        item_index: usize,
    ) -> TenFootDetailFocusId {
        let panel = &data.panels[panel_index];
        TenFootDetailFocusId::PanelItem {
            panel: panel.id.clone(),
            item: panel.items[item_index].id(),
        }
    }

    #[test]
    fn focus_movement_uses_columns_from_viewport_plans() {
        let data = detail_data(&[8, 6]);
        let hd = layout_plan(1_280.0, 800.0, 1.0);
        let full_hd = layout_plan(1_920.0, 1_080.0, 1.0);
        let hd_columns = visible_panel_columns_for_width(1_280.0, &hd);
        let full_hd_columns =
            visible_panel_columns_for_width(1_920.0, &full_hd);

        assert_eq!(hd_columns, 3);
        assert_eq!(full_hd_columns, 5);
        assert_eq!(
            data.move_focus(
                Some(&TenFootDetailFocusId::Action(
                    TenFootDetailAction::Primary,
                )),
                SpatialDirection::Down,
                hd_columns,
            ),
            Some(panel_focus(&data, 0, 0))
        );
        assert_eq!(
            data.move_focus(
                Some(&panel_focus(&data, 0, 1)),
                SpatialDirection::Up,
                hd_columns,
            ),
            Some(TenFootDetailFocusId::Action(TenFootDetailAction::StartOver))
        );
        assert_eq!(
            data.move_focus(
                Some(&panel_focus(&data, 0, 2)),
                SpatialDirection::Down,
                hd_columns,
            ),
            Some(panel_focus(&data, 0, 5))
        );
        assert_eq!(
            data.move_focus(
                Some(&panel_focus(&data, 0, 5)),
                SpatialDirection::Down,
                hd_columns,
            ),
            Some(panel_focus(&data, 1, 2))
        );
        assert_eq!(
            data.move_focus(
                Some(&panel_focus(&data, 0, 2)),
                SpatialDirection::Down,
                full_hd_columns,
            ),
            Some(panel_focus(&data, 0, 7))
        );
    }

    #[test]
    fn focus_vertical_bounds_follow_viewport_layout_plans() {
        let data = detail_data(&[8, 6]);
        let full_hd = layout_plan(1_920.0, 1_080.0, 1.0);
        let short = layout_plan(1_280.0, 560.0, 1.0);
        let action_focus =
            TenFootDetailFocusId::Action(TenFootDetailAction::Primary);
        let first_panel_focus = panel_focus(&data, 0, 0);
        let second_panel_focus = panel_focus(&data, 1, 0);

        assert_eq!(panel_rows(&full_hd), 2);
        assert_eq!(panel_rows(&short), 1);
        assert_eq!(
            data.focus_vertical_bounds(&action_focus, &full_hd),
            Some((full_hd.page_padding_y, hero_height(&full_hd)))
        );

        let first_panel_bounds = data
            .focus_vertical_bounds(&first_panel_focus, &full_hd)
            .expect("first panel bounds");
        assert!(
            (first_panel_bounds.0
                - (full_hd.page_padding_y
                    + hero_height(&full_hd)
                    + full_hd.hero_gap))
                .abs()
                < 0.01
        );
        assert!((first_panel_bounds.1 - panel_height(&full_hd)).abs() < 0.01);

        let second_panel_bounds = data
            .focus_vertical_bounds(&second_panel_focus, &full_hd)
            .expect("second panel bounds");
        assert!(
            (second_panel_bounds.0
                - (first_panel_bounds.0
                    + panel_height(&full_hd)
                    + full_hd.hero_gap))
                .abs()
                < 0.01
        );

        let short_panel_bounds = data
            .focus_vertical_bounds(&first_panel_focus, &short)
            .expect("short panel bounds");
        assert!(short_panel_bounds.0 < first_panel_bounds.0);
        assert!(short_panel_bounds.1 < first_panel_bounds.1);
    }
}
