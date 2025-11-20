//! UI/View domain
//!
//! Contains all UI-related state and logic moved from the monolithic State

pub mod background_state;
pub mod components;
pub mod messages;
pub mod scroll_manager;
pub mod tabs;
pub mod theme;
pub mod transitions;
pub mod types;
pub mod update;
pub mod update_handlers;
pub mod view_models;
pub mod views;
pub mod widgets;
pub mod windows;
pub mod yoke_cache;

use self::views::carousel::CarouselState;
use crate::{
    common::messages::{CrossDomainEvent, DomainMessage},
    domains::ui::{
        background_state::BackgroundShaderState,
        messages::Message as UIMessage,
        scroll_manager::ScrollPositionManager,
        types::{DisplayMode, ViewState},
    },
    infra::{
        repository::{
            EpisodeYoke, MovieYoke, SeasonYoke, SeriesYoke,
            accessor::{Accessor, ReadOnly},
        },
        widgets::poster::poster_animation_types::PosterAnimationType,
    },
};
use ferrex_core::player_prelude::{
    LibraryID, SortBy, SortOrder, UiDecade, UiGenre, UiResolution,
    UiWatchStatus,
};

use iced::Task;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use uuid::Uuid;
use yoke_cache::YokeCache;

/// UI domain state - moved from monolithic State
#[derive(Debug)]
pub struct UIDomainState {
    pub view: ViewState,

    pub default_widget_animation: PosterAnimationType,

    pub repo_accessor: Accessor<ReadOnly>,

    pub movie_yoke_cache: YokeCache<MovieYoke>,
    pub series_yoke_cache: YokeCache<SeriesYoke>,
    pub season_yoke_cache: YokeCache<SeasonYoke>,
    pub episode_yoke_cache: YokeCache<EpisodeYoke>,

    pub movies_carousel: CarouselState,
    pub tv_carousel: CarouselState,

    pub display_mode: DisplayMode,
    pub sort_by: SortBy,
    pub sort_order: SortOrder,
    pub loading: bool,
    pub error_message: Option<String>,
    pub window_size: iced::Size,
    pub expanded_shows: HashSet<String>,
    pub hovered_media_id: Option<Uuid>,

    pub theme_color_cache: parking_lot::RwLock<HashMap<Uuid, iced::Color>>,

    pub current_library_id: Option<LibraryID>,

    pub last_scroll_position: f32,
    pub scroll_stopped_time: Option<Instant>,
    pub last_scroll_time: Option<Instant>,
    pub last_check_task_created: Option<Instant>,
    pub scroll_manager: ScrollPositionManager,
    /// Force the next scroll-stop check even if recent scroll activity continues.
    pub force_scroll_stop_check: bool,

    // Background and visual state
    pub background_shader_state: BackgroundShaderState,

    // Header/navigation state
    pub search_query: String,
    pub show_library_menu: bool,
    pub library_menu_target: Option<Uuid>,
    pub is_fullscreen: bool,

    // Filter panel state (enum-based)
    pub show_filter_panel: bool,
    pub selected_genres: Vec<UiGenre>,
    pub selected_decade: Option<UiDecade>,
    pub selected_resolution: UiResolution,
    pub selected_watch_status: UiWatchStatus,

    // Carousel states
    pub show_seasons_carousel: Option<CarouselState>,
    pub season_episodes_carousel: Option<CarouselState>,

    // Dialog states
    pub show_clear_database_confirm: bool,

    // Navigation history for back button functionality
    pub navigation_history: Vec<ViewState>,

    // Keep UI alive while poster flip animations are running
    pub poster_anim_active_until: Option<std::time::Instant>,
}

impl UIDomainState {}

#[derive(Debug)]
pub struct UIDomain {
    pub state: UIDomainState,
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::all_functions
)]
impl UIDomain {
    pub fn new(state: UIDomainState) -> Self {
        Self { state }
    }

    /// Update function - delegates to existing update_ui logic
    pub fn update(&mut self, _message: UIMessage) -> Task<DomainMessage> {
        // This will call the existing update_ui function
        // For now, we return Task::none() to make it compile
        Task::none()
    }

    pub fn handle_event(
        &mut self,
        event: &CrossDomainEvent,
    ) -> Task<DomainMessage> {
        match event {
            CrossDomainEvent::LibraryChanged(library_id) => {
                log::info!(
                    "UI domain handling LibraryChanged event for library {}",
                    library_id
                );
                // Store the library ID in UI domain state
                self.state.current_library_id = Some(*library_id);
                // Library has been selected - now switch to Library display mode
                self.state.display_mode = DisplayMode::Library;
                // Reset view state when library changes
                self.state.expanded_shows.clear();
                self.state.hovered_media_id = None;
                // Update filters - this already triggers refresh, no need to call RefreshViewModels
                Task::done(DomainMessage::Ui(UIMessage::UpdateViewModelFilters))
            }
            CrossDomainEvent::LibrarySelectAll => {
                log::info!("UI domain handling LibrarySelectAll event");
                // Clear library selection - show all libraries
                self.state.current_library_id = None;
                self.state.display_mode = DisplayMode::Curated;
                // Reset view state
                self.state.expanded_shows.clear();
                self.state.hovered_media_id = None;
                // Update filters - this already triggers refresh, no need to call RefreshViewModels
                Task::done(DomainMessage::Ui(UIMessage::UpdateViewModelFilters))
            }
            CrossDomainEvent::RequestLibraryRefresh => {
                // This is for actual data refresh, not just filter changes
                Task::done(DomainMessage::Ui(UIMessage::RefreshViewModels))
            }
            CrossDomainEvent::RequestViewModelRefresh => {
                // Refresh all ViewModels when media has been loaded
                log::info!(
                    "UI domain received RequestViewModelRefresh event - display_mode: {:?}, current_library_id: {:?}",
                    self.state.display_mode,
                    self.state.current_library_id
                );

                // Ensure we're in a valid display mode
                if matches!(self.state.display_mode, DisplayMode::Curated) {
                    // Good - show all libraries
                    log::info!("UI: In Curated mode - will show all libraries");
                } else if matches!(
                    self.state.display_mode,
                    DisplayMode::Library
                ) && self.state.current_library_id.is_none()
                {
                    // Bad state - Library mode but no library selected
                    log::warn!(
                        "UI: In Library mode but no library selected - switching to Curated"
                    );
                    self.state.display_mode = DisplayMode::Curated;
                }

                Task::done(DomainMessage::Ui(UIMessage::RefreshViewModels))
            }
            _ => Task::none(),
        }
    }
}
