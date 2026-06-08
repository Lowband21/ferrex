use std::collections::HashMap;

use crate::{
    common::{
        focus::SpatialDirection,
        messages::{DomainMessage, DomainUpdateResult},
    },
    domains::ui::{
        messages::UiMessage,
        playback_ui::PlaybackMessage,
        shell_ui::{Scope, UiShellMessage},
        tabs::{TabId, TabState},
        theme,
        views::virtual_carousel::types::CarouselKey,
        widgets::image_for,
    },
    state::State,
};

use ferrex_core::player_prelude::{
    ImageSize, LibraryId, Media, MediaID, MovieID, MovieLike, Priority,
    SeriesID, SeriesLike,
};
use iced::{
    Background, Border, Color, Element, Length, Shadow, Task, Theme, Vector,
    widget::{
        Column, Row, button, column, container, mouse_area,
        operation::scroll_to, row, scrollable, text,
    },
};
use uuid::Uuid;

const PAGE_PADDING_X: f32 = 72.0;
const PAGE_PADDING_Y: f32 = 40.0;
const HERO_HEIGHT: f32 = 390.0;
const HERO_POSTER_WIDTH: f32 = 190.0;
const HERO_POSTER_HEIGHT: f32 = 285.0;
const ACTION_WIDTH: f32 = 260.0;
const ACTION_HEIGHT: f32 = 64.0;
const RAIL_CARD_WIDTH: f32 = 214.0;
const RAIL_CARD_HEIGHT: f32 = 326.0;
const LIBRARY_CARD_HEIGHT: f32 = 190.0;
const COMMAND_CARD_HEIGHT: f32 = 170.0;
const RAIL_GAP: f32 = 24.0;
const RAIL_SECTION_GAP: f32 = 42.0;
const RAIL_HEADER_HEIGHT: f32 = 42.0;
const SCROLL_FOLLOW_MARGIN: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenFootRailId {
    ContinueWatching,
    RecentMovies,
    RecentSeries,
    Libraries,
    Commands,
}

impl TenFootRailId {
    fn title(self) -> &'static str {
        match self {
            Self::ContinueWatching => "Continue Watching",
            Self::RecentMovies => "Recent Movies",
            Self::RecentSeries => "Recent Series",
            Self::Libraries => "Libraries",
            Self::Commands => "Commands",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootMediaKind {
    Movie(MovieID),
    Series(SeriesID),
}

impl TenFootMediaKind {
    fn uuid(&self) -> Uuid {
        match self {
            Self::Movie(id) => id.to_uuid(),
            Self::Series(id) => id.to_uuid(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TenFootCommand {
    Search,
    Display,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootCardId {
    Media(TenFootMediaKind),
    Library(LibraryId),
    Command(TenFootCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TenFootFocusId {
    HeroPrimary,
    HeroDetails,
    RailCard {
        rail: TenFootRailId,
        card: TenFootCardId,
    },
}

#[derive(Debug, Clone)]
pub struct TenFootHomeState {
    pub focus_id: Option<TenFootFocusId>,
    pub preview_media: Option<TenFootMediaKind>,
    pub scrollable_id: iced::widget::Id,
    pub scroll_y: f32,
    pub viewport_height: f32,
    rail_windows: HashMap<TenFootRailId, usize>,
}

impl Default for TenFootHomeState {
    fn default() -> Self {
        Self {
            focus_id: None,
            preview_media: None,
            scrollable_id: iced::widget::Id::unique(),
            scroll_y: 0.0,
            viewport_height: 0.0,
            rail_windows: HashMap::new(),
        }
    }
}

impl TenFootHomeState {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolved_focus(&self, data: &TenFootHomeData) -> Option<TenFootFocusId> {
        self.focus_id
            .as_ref()
            .filter(|focus| data.contains_focus(focus))
            .cloned()
            .or_else(|| data.first_focus())
    }

    fn active_preview_media(
        &self,
        data: &TenFootHomeData,
    ) -> Option<TenFootMediaItem> {
        self.preview_media
            .as_ref()
            .and_then(|kind| data.media_item(kind))
            .or_else(|| data.hero.clone())
    }

    fn sync_preview_from_focus(
        &mut self,
        data: &TenFootHomeData,
        focus: &TenFootFocusId,
    ) {
        if let Some(item) = data.media_item_for_focus(focus) {
            self.preview_media = Some(item.kind.clone());
        } else if self
            .preview_media
            .as_ref()
            .is_some_and(|kind| data.media_item(kind).is_none())
        {
            self.preview_media =
                data.hero.as_ref().map(|item| item.kind.clone());
        }
    }

    pub fn rail_window_start(
        &self,
        rail: TenFootRailId,
        total: usize,
        visible_count: usize,
    ) -> usize {
        bounded_window_start(
            *self.rail_windows.get(&rail).unwrap_or(&0),
            None,
            total,
            visible_count,
        )
    }

    fn follow_focus_window(
        &mut self,
        data: &TenFootHomeData,
        focus: &TenFootFocusId,
        visible_count: usize,
    ) {
        let Some((rail, index, total)) = data.focus_rail_position(focus) else {
            return;
        };

        let current = *self.rail_windows.get(&rail).unwrap_or(&0);
        let next = bounded_window_start(
            current,
            Some(index),
            total,
            visible_count.max(1),
        );
        self.rail_windows.insert(rail, next);
    }

    fn scroll_task_for_focus(
        &mut self,
        data: &TenFootHomeData,
        focus: &TenFootFocusId,
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
pub enum TenFootHomeMessage {
    Move(SpatialDirection),
    ActivateFocused,
    Activate(TenFootFocusId),
    Focus(TenFootFocusId),
    Search,
    Back,
    Scrolled(scrollable::Viewport),
}

impl From<TenFootHomeMessage> for UiMessage {
    fn from(message: TenFootHomeMessage) -> Self {
        UiMessage::TenFootHome(message)
    }
}

#[derive(Debug, Clone)]
pub struct TenFootMediaItem {
    pub kind: TenFootMediaKind,
    pub title: String,
    pub subtitle: String,
    pub overview: Option<String>,
    pub poster_iid: Option<Uuid>,
    pub backdrop_iid: Option<Uuid>,
}

impl TenFootMediaItem {
    fn card_id(&self) -> TenFootCardId {
        TenFootCardId::Media(self.kind.clone())
    }
}

#[derive(Debug, Clone)]
pub struct TenFootLibraryItem {
    pub id: LibraryId,
    pub name: String,
    pub kind_label: String,
    pub count_label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TenFootCommandItem {
    pub command: TenFootCommand,
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone)]
pub enum TenFootCard {
    Media(TenFootMediaItem),
    Library(TenFootLibraryItem),
    Command(TenFootCommandItem),
}

impl TenFootCard {
    fn id(&self) -> TenFootCardId {
        match self {
            Self::Media(item) => item.card_id(),
            Self::Library(item) => TenFootCardId::Library(item.id),
            Self::Command(item) => TenFootCardId::Command(item.command),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TenFootRail {
    pub id: TenFootRailId,
    pub empty_message: String,
    pub cards: Vec<TenFootCard>,
}

impl TenFootRail {
    fn new(
        id: TenFootRailId,
        empty_message: impl Into<String>,
        cards: Vec<TenFootCard>,
    ) -> Self {
        Self {
            id,
            empty_message: empty_message.into(),
            cards,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TenFootHomeData {
    pub hero: Option<TenFootMediaItem>,
    pub rails: Vec<TenFootRail>,
}

impl TenFootHomeData {
    fn from_state(state: &State) -> Self {
        let home_state = match state.tab_manager.get_tab(TabId::Home) {
            Some(TabState::Home(home)) => Some(home),
            _ => None,
        };

        let continue_items = home_state
            .map(|home| {
                home.continue_watching
                    .iter()
                    .filter_map(|id| media_item_from_uuid(state, *id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let recent_movies = home_state
            .map(|home| {
                home.recent_movies
                    .iter()
                    .filter_map(|id| media_item_from_uuid(state, *id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let recent_series = home_state
            .map(|home| {
                home.recent_series
                    .iter()
                    .filter_map(|id| media_item_from_uuid(state, *id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let library_cards = state
            .domains
            .library
            .state
            .libraries
            .iter()
            .filter(|library| library.enabled)
            .map(|library| {
                TenFootCard::Library(TenFootLibraryItem {
                    id: library.id,
                    name: library.name.clone(),
                    kind_label: match library.library_type {
                        ferrex_core::player_prelude::LibraryType::Movies => {
                            "Movies".to_string()
                        }
                        ferrex_core::player_prelude::LibraryType::Series => {
                            "Series".to_string()
                        }
                    },
                    count_label: library_count_label(state, library.id),
                })
            })
            .collect::<Vec<_>>();

        let commands = vec![
            TenFootCard::Command(TenFootCommandItem {
                command: TenFootCommand::Search,
                title: "Search".to_string(),
                subtitle: "Find movies, shows, and episodes".to_string(),
            }),
            TenFootCard::Command(TenFootCommandItem {
                command: TenFootCommand::Display,
                title: if state.is_fullscreen
                    || state.domains.ui.state.is_fullscreen
                {
                    "Exit Fullscreen".to_string()
                } else {
                    "Fullscreen".to_string()
                },
                subtitle: "Toggle the display mode".to_string(),
            }),
        ];

        let hero = continue_items
            .first()
            .cloned()
            .or_else(|| recent_movies.first().cloned())
            .or_else(|| recent_series.first().cloned());

        Self {
            hero,
            rails: vec![
                TenFootRail::new(
                    TenFootRailId::ContinueWatching,
                    "Nothing is in progress yet.",
                    continue_items
                        .into_iter()
                        .map(TenFootCard::Media)
                        .collect(),
                ),
                TenFootRail::new(
                    TenFootRailId::RecentMovies,
                    "No recent movies are available.",
                    recent_movies.into_iter().map(TenFootCard::Media).collect(),
                ),
                TenFootRail::new(
                    TenFootRailId::RecentSeries,
                    "No recent series are available.",
                    recent_series.into_iter().map(TenFootCard::Media).collect(),
                ),
                TenFootRail::new(
                    TenFootRailId::Libraries,
                    "No enabled libraries are available.",
                    library_cards,
                ),
                TenFootRail::new(TenFootRailId::Commands, "", commands),
            ],
        }
    }

    fn first_focus(&self) -> Option<TenFootFocusId> {
        if self.hero.is_some() {
            return Some(TenFootFocusId::HeroPrimary);
        }
        self.first_focus_in_rail_from(0)
    }

    fn contains_focus(&self, focus: &TenFootFocusId) -> bool {
        match focus {
            TenFootFocusId::HeroPrimary | TenFootFocusId::HeroDetails => {
                self.hero.is_some()
            }
            TenFootFocusId::RailCard { rail, card } => self
                .rail(*rail)
                .is_some_and(|rail| rail.cards.iter().any(|c| c.id() == *card)),
        }
    }

    fn rail(&self, rail: TenFootRailId) -> Option<&TenFootRail> {
        self.rails.iter().find(|candidate| candidate.id == rail)
    }

    fn rail_index(&self, rail: TenFootRailId) -> Option<usize> {
        self.rails.iter().position(|candidate| candidate.id == rail)
    }

    fn first_focus_in_rail_from(&self, start: usize) -> Option<TenFootFocusId> {
        self.rails.iter().skip(start).find_map(|rail| {
            rail.cards.first().map(|card| TenFootFocusId::RailCard {
                rail: rail.id,
                card: card.id(),
            })
        })
    }

    fn previous_focusable_rail(&self, before: usize) -> Option<&TenFootRail> {
        self.rails
            .iter()
            .take(before)
            .rev()
            .find(|rail| !rail.cards.is_empty())
    }

    fn next_focusable_rail(&self, after: usize) -> Option<&TenFootRail> {
        self.rails
            .iter()
            .skip(after + 1)
            .find(|rail| !rail.cards.is_empty())
    }

    fn focus_for_rail_index(
        &self,
        rail: &TenFootRail,
        preferred_index: usize,
    ) -> Option<TenFootFocusId> {
        let index = preferred_index.min(rail.cards.len().saturating_sub(1));
        rail.cards.get(index).map(|card| TenFootFocusId::RailCard {
            rail: rail.id,
            card: card.id(),
        })
    }

    fn rail_position(
        &self,
        rail: TenFootRailId,
        card: &TenFootCardId,
    ) -> Option<(usize, usize)> {
        let rail = self.rail(rail)?;
        let index = rail
            .cards
            .iter()
            .position(|candidate| candidate.id() == *card)?;
        Some((index, rail.cards.len()))
    }

    fn focus_rail_position(
        &self,
        focus: &TenFootFocusId,
    ) -> Option<(TenFootRailId, usize, usize)> {
        let TenFootFocusId::RailCard { rail, card } = focus else {
            return None;
        };
        let (index, total) = self.rail_position(*rail, card)?;
        Some((*rail, index, total))
    }

    fn media_item(&self, kind: &TenFootMediaKind) -> Option<TenFootMediaItem> {
        self.rails
            .iter()
            .flat_map(|rail| rail.cards.iter())
            .find_map(|card| match card {
                TenFootCard::Media(item) if &item.kind == kind => {
                    Some(item.clone())
                }
                _ => None,
            })
            .or_else(|| {
                self.hero
                    .as_ref()
                    .filter(|item| &item.kind == kind)
                    .cloned()
            })
    }

    fn media_item_for_focus(
        &self,
        focus: &TenFootFocusId,
    ) -> Option<TenFootMediaItem> {
        let TenFootFocusId::RailCard { rail, card } = focus else {
            return None;
        };
        let TenFootCardId::Media(kind) = card else {
            return None;
        };
        self.rail(*rail)?
            .cards
            .iter()
            .find_map(|candidate| match candidate {
                TenFootCard::Media(item) if &item.kind == kind => {
                    Some(item.clone())
                }
                _ => None,
            })
    }

    fn preview_for_focus(
        &self,
        focus: Option<&TenFootFocusId>,
        home_state: &TenFootHomeState,
    ) -> TenFootHeroPreview {
        match focus {
            Some(focus) => match focus {
                TenFootFocusId::RailCard {
                    card: TenFootCardId::Media(_),
                    ..
                } => self
                    .media_item_for_focus(focus)
                    .map(TenFootHeroPreview::Media)
                    .unwrap_or_else(|| {
                        home_state
                            .active_preview_media(self)
                            .map(TenFootHeroPreview::Media)
                            .unwrap_or(TenFootHeroPreview::Empty)
                    }),
                TenFootFocusId::RailCard {
                    rail,
                    card: TenFootCardId::Library(id),
                } => self
                    .rail(*rail)
                    .and_then(|rail| {
                        rail.cards.iter().find_map(
                            |candidate| match candidate {
                                TenFootCard::Library(item)
                                    if item.id == *id =>
                                {
                                    Some(TenFootHeroPreview::Library(
                                        item.clone(),
                                    ))
                                }
                                _ => None,
                            },
                        )
                    })
                    .unwrap_or(TenFootHeroPreview::Empty),
                TenFootFocusId::RailCard {
                    rail,
                    card: TenFootCardId::Command(command),
                } => self
                    .rail(*rail)
                    .and_then(|rail| {
                        rail.cards.iter().find_map(
                            |candidate| match candidate {
                                TenFootCard::Command(item)
                                    if item.command == *command =>
                                {
                                    Some(TenFootHeroPreview::Command(
                                        item.clone(),
                                    ))
                                }
                                _ => None,
                            },
                        )
                    })
                    .unwrap_or(TenFootHeroPreview::Empty),
                TenFootFocusId::HeroPrimary | TenFootFocusId::HeroDetails => {
                    home_state
                        .active_preview_media(self)
                        .map(TenFootHeroPreview::Media)
                        .unwrap_or(TenFootHeroPreview::Empty)
                }
            },
            None => home_state
                .active_preview_media(self)
                .map(TenFootHeroPreview::Media)
                .unwrap_or(TenFootHeroPreview::Empty),
        }
    }

    fn move_focus(
        &self,
        current: Option<&TenFootFocusId>,
        direction: SpatialDirection,
    ) -> Option<TenFootFocusId> {
        let current = current
            .filter(|focus| self.contains_focus(focus))
            .cloned()
            .or_else(|| self.first_focus())?;

        match current {
            TenFootFocusId::HeroPrimary => match direction {
                SpatialDirection::Right => Some(TenFootFocusId::HeroDetails),
                SpatialDirection::Down => self.first_focus_in_rail_from(0),
                _ => Some(TenFootFocusId::HeroPrimary),
            },
            TenFootFocusId::HeroDetails => match direction {
                SpatialDirection::Left => Some(TenFootFocusId::HeroPrimary),
                SpatialDirection::Down => self.first_focus_in_rail_from(0),
                _ => Some(TenFootFocusId::HeroDetails),
            },
            TenFootFocusId::RailCard { rail, card } => {
                let rail_index = self.rail_index(rail)?;
                let rail_ref = self.rail(rail)?;
                let card_index = rail_ref
                    .cards
                    .iter()
                    .position(|candidate| candidate.id() == card)?;

                match direction {
                    SpatialDirection::Left => {
                        if card_index > 0 {
                            self.focus_for_rail_index(rail_ref, card_index - 1)
                        } else {
                            Some(TenFootFocusId::RailCard { rail, card })
                        }
                    }
                    SpatialDirection::Right => {
                        if card_index + 1 < rail_ref.cards.len() {
                            self.focus_for_rail_index(rail_ref, card_index + 1)
                        } else {
                            Some(TenFootFocusId::RailCard { rail, card })
                        }
                    }
                    SpatialDirection::Up => self
                        .previous_focusable_rail(rail_index)
                        .and_then(|previous| {
                            self.focus_for_rail_index(previous, card_index)
                        })
                        .or_else(|| {
                            if self.hero.is_some() {
                                Some(TenFootFocusId::HeroPrimary)
                            } else {
                                Some(TenFootFocusId::RailCard {
                                    rail,
                                    card: card.clone(),
                                })
                            }
                        }),
                    SpatialDirection::Down => self
                        .next_focusable_rail(rail_index)
                        .and_then(|next| {
                            self.focus_for_rail_index(next, card_index)
                        })
                        .or_else(|| {
                            Some(TenFootFocusId::RailCard {
                                rail,
                                card: card.clone(),
                            })
                        }),
                }
            }
        }
    }

    fn focus_vertical_bounds(
        &self,
        focus: &TenFootFocusId,
    ) -> Option<(f32, f32)> {
        match focus {
            TenFootFocusId::HeroPrimary | TenFootFocusId::HeroDetails => {
                Some((0.0, HERO_HEIGHT))
            }
            TenFootFocusId::RailCard { rail, .. } => {
                let index = self.rail_index(*rail)?;
                let top = HERO_HEIGHT
                    + RAIL_SECTION_GAP
                    + index as f32
                        * (RAIL_HEADER_HEIGHT
                            + RAIL_CARD_HEIGHT
                            + RAIL_SECTION_GAP);
                Some((top, RAIL_HEADER_HEIGHT + RAIL_CARD_HEIGHT))
            }
        }
    }
}

#[derive(Debug, Clone)]
enum TenFootHeroPreview {
    Media(TenFootMediaItem),
    Library(TenFootLibraryItem),
    Command(TenFootCommandItem),
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TenFootActivation {
    PlayMedia(MediaID),
    PlaySeriesNextEpisode(SeriesID),
    ViewMovieDetails(MovieID),
    ViewTvShow(SeriesID),
    SelectLibrary(LibraryId),
    Search,
    ToggleFullscreen,
}

pub fn is_tenfoot_home_route(state: &State) -> bool {
    state.interface_mode.is_tenfoot()
        && state.is_authenticated
        && matches!(
            state.domains.ui.state.view,
            crate::domains::ui::types::ViewState::Library
                | crate::domains::ui::types::ViewState::LibraryManagement
                | crate::domains::ui::types::ViewState::AdminDashboard
                | crate::domains::ui::types::ViewState::AdminUsers
                | crate::domains::ui::types::ViewState::UserSettings
        )
}

pub fn update_tenfoot_home(
    state: &mut State,
    message: TenFootHomeMessage,
) -> DomainUpdateResult {
    match message {
        TenFootHomeMessage::Move(direction) => {
            let data = TenFootHomeData::from_state(state);
            let current =
                state.domains.ui.state.tenfoot_home.resolved_focus(&data);
            let Some(next) = data.move_focus(current.as_ref(), direction)
            else {
                return DomainUpdateResult::task(Task::none());
            };

            let visible_count =
                visible_cards_for_width(state.window_size.width);
            let fallback_height = state.window_size.height;
            let task = {
                let home = &mut state.domains.ui.state.tenfoot_home;
                home.focus_id = Some(next.clone());
                home.sync_preview_from_focus(&data, &next);
                home.follow_focus_window(&data, &next, visible_count);
                home.scroll_task_for_focus(&data, &next, fallback_height)
            };

            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        TenFootHomeMessage::Focus(focus) => {
            let data = TenFootHomeData::from_state(state);
            if !data.contains_focus(&focus) {
                return DomainUpdateResult::task(Task::none());
            }
            let visible_count =
                visible_cards_for_width(state.window_size.width);
            let fallback_height = state.window_size.height;
            let task = {
                let home = &mut state.domains.ui.state.tenfoot_home;
                home.focus_id = Some(focus.clone());
                home.sync_preview_from_focus(&data, &focus);
                home.follow_focus_window(&data, &focus, visible_count);
                home.scroll_task_for_focus(&data, &focus, fallback_height)
            };
            DomainUpdateResult::task(task.map(DomainMessage::Ui))
        }
        TenFootHomeMessage::ActivateFocused => {
            let data = TenFootHomeData::from_state(state);
            let focus =
                state.domains.ui.state.tenfoot_home.resolved_focus(&data);
            let activation = focus.as_ref().and_then(|focus| {
                activation_for_focus(
                    &data,
                    focus,
                    state.domains.ui.state.tenfoot_home.preview_media.as_ref(),
                )
            });
            DomainUpdateResult::task(task_for_activation(activation))
        }
        TenFootHomeMessage::Activate(focus) => {
            let data = TenFootHomeData::from_state(state);
            if !data.contains_focus(&focus) {
                return DomainUpdateResult::task(Task::none());
            }
            let activation = activation_for_focus(
                &data,
                &focus,
                state.domains.ui.state.tenfoot_home.preview_media.as_ref(),
            );
            {
                let visible_count =
                    visible_cards_for_width(state.window_size.width);
                let home = &mut state.domains.ui.state.tenfoot_home;
                home.focus_id = Some(focus.clone());
                home.sync_preview_from_focus(&data, &focus);
                home.follow_focus_window(&data, &focus, visible_count);
            }
            DomainUpdateResult::task(task_for_activation(activation))
        }
        TenFootHomeMessage::Search => DomainUpdateResult::task(Task::done(
            DomainMessage::Ui(UiShellMessage::OpenSearchOverlay.into()),
        )),
        TenFootHomeMessage::Back => DomainUpdateResult::task(Task::done(
            DomainMessage::Ui(UiShellMessage::NavigateBack.into()),
        )),
        TenFootHomeMessage::Scrolled(viewport) => {
            let offset = viewport.absolute_offset();
            let bounds = viewport.bounds();
            state.domains.ui.state.tenfoot_home.scroll_y = offset.y;
            state.domains.ui.state.tenfoot_home.viewport_height = bounds.height;
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

pub fn view_tenfoot_home(state: &State) -> Element<'_, UiMessage> {
    let data = TenFootHomeData::from_state(state);
    let home_state = &state.domains.ui.state.tenfoot_home;
    let focused = home_state.resolved_focus(&data);
    let preview = data.preview_for_focus(focused.as_ref(), home_state);
    let visible_count = visible_cards_for_width(state.window_size.width);

    let mut rail_column: Column<'_, UiMessage> =
        column![].spacing(RAIL_SECTION_GAP);
    for rail in &data.rails {
        rail_column = rail_column.push(view_rail(
            state,
            rail,
            focused.as_ref(),
            visible_count,
        ));
    }

    let content =
        column![view_hero(state, &preview, focused.as_ref()), rail_column]
            .spacing(RAIL_SECTION_GAP)
            .padding([PAGE_PADDING_Y, PAGE_PADDING_X])
            .width(Length::Fill);

    let scroll = scrollable(content)
        .id(home_state.scrollable_id.clone())
        .on_scroll(|viewport| TenFootHomeMessage::Scrolled(viewport).into())
        .width(Length::Fill)
        .height(Length::Fill);

    container(scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(tenfoot_page_style())
        .into()
}

fn view_hero<'a>(
    state: &'a State,
    preview: &TenFootHeroPreview,
    focused: Option<&TenFootFocusId>,
) -> Element<'a, UiMessage> {
    let heading = row![
        column![
            text("Ferrex Home")
                .size(42)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text("Use arrows to move • Enter/Space to activate • / or S to search")
                .size(20)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(6),
    ]
    .width(Length::Fill);

    let body: Element<'a, UiMessage> = match preview {
        TenFootHeroPreview::Media(item) => {
            let title = text(item.title.clone())
                .size(56)
                .color(theme::MediaServerTheme::TEXT_PRIMARY);
            let subtitle = text(item.subtitle.clone())
                .size(24)
                .color(theme::MediaServerTheme::TEXT_SECONDARY);
            let overview = text(
                item.overview
                    .clone()
                    .unwrap_or_else(|| "No overview available.".to_string()),
            )
            .size(22)
            .color(theme::MediaServerTheme::TEXT_SECONDARY)
            .width(Length::Fill);

            let primary_focused = focused == Some(&TenFootFocusId::HeroPrimary);
            let details_focused = focused == Some(&TenFootFocusId::HeroDetails);

            let actions = row![
                focusable_action_button(
                    "Play",
                    "Start watching",
                    TenFootFocusId::HeroPrimary,
                    primary_focused,
                ),
                focusable_action_button(
                    "Details",
                    "Open detail view",
                    TenFootFocusId::HeroDetails,
                    details_focused,
                ),
            ]
            .spacing(18);

            let poster = view_media_image(
                state,
                item,
                HERO_POSTER_WIDTH,
                HERO_POSTER_HEIGHT,
                primary_focused || details_focused,
            );

            row![
                column![title, subtitle, overview, actions]
                    .spacing(18)
                    .width(Length::Fill),
                poster,
            ]
            .spacing(42)
            .align_y(iced::Alignment::Center)
            .into()
        }
        TenFootHeroPreview::Library(item) => column![
            text(item.name.clone())
                .size(54)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(item.kind_label.clone())
                .size(26)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
            text(
                item.count_label
                    .clone()
                    .unwrap_or_else(|| "Open this library".to_string())
            )
            .size(22)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(18)
        .width(Length::Fill)
        .into(),
        TenFootHeroPreview::Command(item) => column![
            text(item.title.clone())
                .size(54)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text(item.subtitle.clone())
                .size(26)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(18)
        .width(Length::Fill)
        .into(),
        TenFootHeroPreview::Empty => column![
            text("Ready when your library is")
                .size(54)
                .color(theme::MediaServerTheme::TEXT_PRIMARY),
            text("Recent media will appear here after libraries load.")
                .size(24)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        ]
        .spacing(18)
        .width(Length::Fill)
        .into(),
    };

    container(column![heading, body].spacing(30))
        .height(Length::Fixed(HERO_HEIGHT))
        .width(Length::Fill)
        .padding(34)
        .style(tenfoot_panel_style(false))
        .into()
}

fn focusable_action_button<'a>(
    label: &'static str,
    subtitle: &'static str,
    focus_id: TenFootFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    let content = column![
        text(label)
            .size(24)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(subtitle)
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
        .on_press(TenFootHomeMessage::Activate(focus_id.clone()).into());

    mouse_area(button_element)
        .on_enter(TenFootHomeMessage::Focus(focus_id).into())
        .into()
}

fn view_rail<'a>(
    state: &'a State,
    rail: &TenFootRail,
    focused: Option<&TenFootFocusId>,
    visible_count: usize,
) -> Element<'a, UiMessage> {
    let total = rail.cards.len();
    let focused_index = focused.and_then(|focus| match focus {
        TenFootFocusId::RailCard {
            rail: focused_rail,
            card,
        } if *focused_rail == rail.id => rail
            .cards
            .iter()
            .position(|candidate| candidate.id() == *card),
        _ => None,
    });
    let start = bounded_window_start(
        state.domains.ui.state.tenfoot_home.rail_window_start(
            rail.id,
            total,
            visible_count,
        ),
        focused_index,
        total,
        visible_count,
    );
    let end = (start + visible_count).min(total);

    let range_label = if total == 0 {
        "0".to_string()
    } else {
        format!("{}–{} of {}", start + 1, end, total)
    };

    let header = row![
        text(rail.id.title())
            .size(30)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(range_label)
            .size(18)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(18)
    .align_y(iced::Alignment::Center);

    let body: Element<'a, UiMessage> = if rail.cards.is_empty() {
        container(
            text(rail.empty_message.clone())
                .size(22)
                .color(theme::MediaServerTheme::TEXT_SECONDARY),
        )
        .width(Length::Fill)
        .height(Length::Fixed(RAIL_CARD_HEIGHT))
        .padding(28)
        .align_y(iced::Alignment::Center)
        .style(tenfoot_panel_style(false))
        .into()
    } else {
        let mut cards: Row<'a, UiMessage> = Row::new().spacing(RAIL_GAP);
        if start > 0 {
            cards = cards.push(edge_hint("‹"));
        }
        for card in rail.cards.iter().skip(start).take(visible_count) {
            let focus_id = TenFootFocusId::RailCard {
                rail: rail.id,
                card: card.id(),
            };
            let is_focused = focused == Some(&focus_id);
            cards = cards
                .push(view_card(state, rail.id, card, focus_id, is_focused));
        }
        if end < total {
            cards = cards.push(edge_hint("›"));
        }
        cards.into()
    };

    column![header, body].spacing(14).width(Length::Fill).into()
}

fn edge_hint<'a>(label: &'static str) -> Element<'a, UiMessage> {
    container(
        text(label)
            .size(42)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(RAIL_CARD_HEIGHT))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center)
    .into()
}

fn view_card<'a>(
    state: &'a State,
    rail: TenFootRailId,
    card: &TenFootCard,
    focus_id: TenFootFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    match card {
        TenFootCard::Media(item) => {
            view_media_card(state, rail, item, focus_id, focused)
        }
        TenFootCard::Library(item) => {
            view_library_card(item, focus_id, focused)
        }
        TenFootCard::Command(item) => {
            view_command_card(item, focus_id, focused)
        }
    }
}

fn view_media_card<'a>(
    state: &'a State,
    rail: TenFootRailId,
    item: &TenFootMediaItem,
    focus_id: TenFootFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    let image = view_media_image(state, item, 164.0, 246.0, focused);
    let activation_hint = if rail == TenFootRailId::ContinueWatching {
        "Play"
    } else {
        "Details"
    };

    let content = column![
        image,
        text(item.title.clone())
            .size(20)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(format!("{} • {}", item.subtitle, activation_hint))
            .size(15)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(10)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);

    card_button(content.into(), focus_id, focused, RAIL_CARD_HEIGHT)
}

fn view_media_image<'a>(
    state: &'a State,
    item: &TenFootMediaItem,
    width: f32,
    height: f32,
    focused: bool,
) -> Element<'a, UiMessage> {
    let poster_quality = state.domains.settings.display.library_poster_quality;
    let mut image = image_for(item.kind.uuid())
        .iid(item.poster_iid)
        .skip_request(item.poster_iid.is_none())
        .size(ImageSize::Poster(poster_quality))
        .radius(16.0)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .priority(if focused {
            Priority::Visible
        } else {
            Priority::Preload
        })
        .placeholder(match item.kind {
            TenFootMediaKind::Movie(_) => lucide_icons::Icon::Film,
            TenFootMediaKind::Series(_) => lucide_icons::Icon::Tv,
        })
        .no_animation();

    image = image.carousel_key(CarouselKey::Custom("TenFootHome"));
    image.into()
}

fn view_library_card<'a>(
    item: &TenFootLibraryItem,
    focus_id: TenFootFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    let count = item
        .count_label
        .clone()
        .unwrap_or_else(|| "Open library".to_string());
    let content = column![
        text("Library")
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(item.name.clone())
            .size(25)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(item.kind_label.clone())
            .size(18)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
        text(count)
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(8)
    .width(Length::Fill);

    card_button(content.into(), focus_id, focused, LIBRARY_CARD_HEIGHT)
}

fn view_command_card<'a>(
    item: &TenFootCommandItem,
    focus_id: TenFootFocusId,
    focused: bool,
) -> Element<'a, UiMessage> {
    let content = column![
        text(match item.command {
            TenFootCommand::Search => "⌕",
            TenFootCommand::Display => "▣",
        })
        .size(38)
        .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(item.title.clone())
            .size(25)
            .color(theme::MediaServerTheme::TEXT_PRIMARY),
        text(item.subtitle.clone())
            .size(16)
            .color(theme::MediaServerTheme::TEXT_SECONDARY),
    ]
    .spacing(8)
    .width(Length::Fill)
    .align_x(iced::Alignment::Center);

    card_button(content.into(), focus_id, focused, COMMAND_CARD_HEIGHT)
}

fn card_button<'a>(
    content: Element<'a, UiMessage>,
    focus_id: TenFootFocusId,
    focused: bool,
    height: f32,
) -> Element<'a, UiMessage> {
    let button_element = button(
        container(content)
            .padding(18)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center),
    )
    .padding(0)
    .width(Length::Fixed(RAIL_CARD_WIDTH))
    .height(Length::Fixed(height))
    .style(tenfoot_button_style(focused))
    .on_press(TenFootHomeMessage::Activate(focus_id.clone()).into());

    mouse_area(button_element)
        .on_enter(TenFootHomeMessage::Focus(focus_id).into())
        .into()
}

fn media_item_from_uuid(state: &State, id: Uuid) -> Option<TenFootMediaItem> {
    let accessor = &state.domains.ui.state.repo_accessor;
    if let Ok(Media::Movie(movie)) = accessor.get(&MediaID::Movie(MovieID(id)))
    {
        return Some(TenFootMediaItem {
            kind: TenFootMediaKind::Movie(movie.id),
            title: movie.title().to_string(),
            subtitle: movie
                .release_year()
                .map(|year| format!("Movie • {year}"))
                .unwrap_or_else(|| "Movie".to_string()),
            overview: movie.details.overview.clone(),
            poster_iid: movie.details.primary_poster_iid,
            backdrop_iid: movie.details.primary_backdrop_iid,
        });
    }

    if let Ok(Media::Series(series)) =
        accessor.get(&MediaID::Series(SeriesID(id)))
    {
        return Some(TenFootMediaItem {
            kind: TenFootMediaKind::Series(series.id),
            title: series.title().to_string(),
            subtitle: "Series".to_string(),
            overview: series.details.overview.clone(),
            poster_iid: series.details.primary_poster_iid,
            backdrop_iid: series.details.primary_backdrop_iid,
        });
    }

    None
}

fn library_count_label(state: &State, library_id: LibraryId) -> Option<String> {
    let Some(TabState::Library(tab)) =
        state.tab_manager.get_tab(TabId::Library(library_id))
    else {
        return None;
    };

    let count = tab.cached_index_ids.len();
    let unit = match tab.library_type {
        crate::infra::api_types::LibraryType::Movies => {
            if count == 1 {
                "movie"
            } else {
                "movies"
            }
        }
        crate::infra::api_types::LibraryType::Series => {
            if count == 1 {
                "series"
            } else {
                "series"
            }
        }
    };
    Some(format!("{count} {unit}"))
}

fn activation_for_focus(
    data: &TenFootHomeData,
    focus: &TenFootFocusId,
    preview_media: Option<&TenFootMediaKind>,
) -> Option<TenFootActivation> {
    match focus {
        TenFootFocusId::HeroPrimary => preview_media
            .and_then(|kind| data.media_item(kind))
            .or_else(|| data.hero.clone())
            .map(|item| play_activation(&item.kind)),
        TenFootFocusId::HeroDetails => preview_media
            .and_then(|kind| data.media_item(kind))
            .or_else(|| data.hero.clone())
            .map(|item| details_activation(&item.kind)),
        TenFootFocusId::RailCard { rail, card } => match card {
            TenFootCardId::Media(kind) => {
                if *rail == TenFootRailId::ContinueWatching {
                    Some(play_activation(kind))
                } else {
                    Some(details_activation(kind))
                }
            }
            TenFootCardId::Library(library_id) => {
                Some(TenFootActivation::SelectLibrary(*library_id))
            }
            TenFootCardId::Command(command) => match command {
                TenFootCommand::Search => Some(TenFootActivation::Search),
                TenFootCommand::Display => {
                    Some(TenFootActivation::ToggleFullscreen)
                }
            },
        },
    }
}

fn play_activation(kind: &TenFootMediaKind) -> TenFootActivation {
    match kind {
        TenFootMediaKind::Movie(id) => {
            TenFootActivation::PlayMedia(MediaID::Movie(*id))
        }
        TenFootMediaKind::Series(id) => {
            TenFootActivation::PlaySeriesNextEpisode(*id)
        }
    }
}

fn details_activation(kind: &TenFootMediaKind) -> TenFootActivation {
    match kind {
        TenFootMediaKind::Movie(id) => TenFootActivation::ViewMovieDetails(*id),
        TenFootMediaKind::Series(id) => TenFootActivation::ViewTvShow(*id),
    }
}

fn task_for_activation(
    activation: Option<TenFootActivation>,
) -> Task<DomainMessage> {
    match activation {
        Some(TenFootActivation::PlayMedia(media_id)) => {
            Task::done(DomainMessage::Ui(
                PlaybackMessage::PlayMediaWithId(media_id).into(),
            ))
        }
        Some(TenFootActivation::PlaySeriesNextEpisode(series_id)) => {
            Task::done(DomainMessage::Ui(
                PlaybackMessage::PlaySeriesNextEpisode(series_id).into(),
            ))
        }
        Some(TenFootActivation::ViewMovieDetails(movie_id)) => {
            Task::done(DomainMessage::Ui(
                UiShellMessage::ViewMovieDetails(movie_id).into(),
            ))
        }
        Some(TenFootActivation::ViewTvShow(series_id)) => Task::done(
            DomainMessage::Ui(UiShellMessage::ViewTvShow(series_id).into()),
        ),
        Some(TenFootActivation::SelectLibrary(library_id)) => {
            Task::done(DomainMessage::Ui(
                UiShellMessage::SelectScope(Scope::Library(library_id)).into(),
            ))
        }
        Some(TenFootActivation::Search) => Task::done(DomainMessage::Ui(
            UiShellMessage::OpenSearchOverlay.into(),
        )),
        Some(TenFootActivation::ToggleFullscreen) => Task::done(
            DomainMessage::Ui(UiShellMessage::ToggleFullscreen.into()),
        ),
        None => Task::none(),
    }
}

pub fn visible_cards_for_width(width: f32) -> usize {
    let available = (width - PAGE_PADDING_X * 2.0).max(RAIL_CARD_WIDTH);
    let per_card = RAIL_CARD_WIDTH + RAIL_GAP;
    ((available + RAIL_GAP) / per_card).floor().max(1.0) as usize
}

pub fn bounded_window_start(
    current_start: usize,
    focused_index: Option<usize>,
    total: usize,
    visible_count: usize,
) -> usize {
    if total == 0 {
        return 0;
    }

    let visible_count = visible_count.max(1).min(total);
    let max_start = total.saturating_sub(visible_count);
    let mut start = current_start.min(max_start);

    if let Some(index) = focused_index.map(|idx| idx.min(total - 1)) {
        if index < start {
            start = index;
        } else if index >= start + visible_count {
            start = index + 1 - visible_count;
        }
    }

    start.min(max_start)
}

fn tenfoot_page_style() -> impl Fn(&Theme) -> container::Style + Clone {
    |_| container::Style {
        text_color: Some(theme::MediaServerTheme::TEXT_PRIMARY),
        background: Some(Background::Color(Color::from_rgb(
            0.015, 0.014, 0.02,
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
            Color::from_rgba(0.08, 0.075, 0.10, 0.88)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(id: u128) -> TenFootMediaItem {
        TenFootMediaItem {
            kind: TenFootMediaKind::Movie(MovieID(Uuid::from_u128(id))),
            title: format!("Movie {id}"),
            subtitle: "Movie".to_string(),
            overview: None,
            poster_iid: None,
            backdrop_iid: None,
        }
    }

    fn data_with_empty_middle_rail() -> TenFootHomeData {
        TenFootHomeData {
            hero: Some(movie(1)),
            rails: vec![
                TenFootRail::new(
                    TenFootRailId::ContinueWatching,
                    "empty",
                    vec![TenFootCard::Media(movie(1))],
                ),
                TenFootRail::new(TenFootRailId::RecentMovies, "empty", vec![]),
                TenFootRail::new(
                    TenFootRailId::RecentSeries,
                    "empty",
                    vec![TenFootCard::Media(movie(2))],
                ),
                TenFootRail::new(
                    TenFootRailId::Commands,
                    "empty",
                    vec![TenFootCard::Command(TenFootCommandItem {
                        command: TenFootCommand::Search,
                        title: "Search".to_string(),
                        subtitle: "Find".to_string(),
                    })],
                ),
            ],
        }
    }

    #[test]
    fn bounded_window_follows_focused_card() {
        assert_eq!(bounded_window_start(0, Some(0), 10, 4), 0);
        assert_eq!(bounded_window_start(0, Some(4), 10, 4), 1);
        assert_eq!(bounded_window_start(4, Some(3), 10, 4), 3);
        assert_eq!(bounded_window_start(9, Some(9), 10, 4), 6);
        assert_eq!(bounded_window_start(4, None, 3, 10), 0);
    }

    #[test]
    fn vertical_navigation_skips_empty_rails() {
        let data = data_with_empty_middle_rail();
        let first = data
            .move_focus(
                Some(&TenFootFocusId::HeroPrimary),
                SpatialDirection::Down,
            )
            .expect("down from hero");
        assert_eq!(
            first,
            TenFootFocusId::RailCard {
                rail: TenFootRailId::ContinueWatching,
                card: TenFootCardId::Media(TenFootMediaKind::Movie(MovieID(
                    Uuid::from_u128(1)
                ))),
            }
        );

        let second = data
            .move_focus(Some(&first), SpatialDirection::Down)
            .expect("down to next populated rail");
        assert_eq!(
            second,
            TenFootFocusId::RailCard {
                rail: TenFootRailId::RecentSeries,
                card: TenFootCardId::Media(TenFootMediaKind::Movie(MovieID(
                    Uuid::from_u128(2)
                ))),
            }
        );
    }

    #[test]
    fn continue_watching_activation_plays_media() {
        let data = data_with_empty_middle_rail();
        let focus = TenFootFocusId::RailCard {
            rail: TenFootRailId::ContinueWatching,
            card: TenFootCardId::Media(TenFootMediaKind::Movie(MovieID(
                Uuid::from_u128(1),
            ))),
        };

        assert_eq!(
            activation_for_focus(&data, &focus, None),
            Some(TenFootActivation::PlayMedia(MediaID::Movie(MovieID(
                Uuid::from_u128(1),
            ))))
        );
    }
}
