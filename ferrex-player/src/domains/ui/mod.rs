//! UI/View domain
//!
//! Contains all UI-related state and logic

pub mod background_ui;
pub mod components;
pub mod feedback_ui;
pub mod header_ui;
pub mod interaction_ui;
pub mod library_ui;
pub mod menu;
pub mod messages;
pub mod motion_controller;
pub mod playback_ui;
pub mod scroll_manager;
pub mod search_surface;
pub mod settings_ui;
pub mod shell_ui;
pub mod tabs;
pub mod theme;
pub mod types;
pub mod update;
pub mod update_handlers;
pub mod utils;
pub mod view_model_ui;
pub mod view_models;
pub mod views;
pub mod widgets;
pub mod window_ui;
pub mod windows;

pub use motion_controller::MotionController;

use crate::{
    common::messages::{CrossDomainEvent, DomainMessage},
    domains::ui::{
        menu::PosterMenuState,
        messages::UiMessage as UIMessage,
        scroll_manager::ScrollPositionManager,
        shell_ui::Scope,
        types::ViewState,
        view_model_ui::ViewModelMessage,
        views::{
            carousel::CarouselState,
            virtual_carousel::{CarouselFocus, CarouselRegistry},
        },
    },
    infra::{
        constants::layout::calculations::ScaledLayout,
        design_tokens::{ScalingContext, SizeProvider},
        repository::{
            EpisodeYoke, MovieYoke, SeasonYoke, SeriesYoke,
            accessor::{Accessor, ReadOnly},
            yoke_cache::YokeCache,
        },
        shader_widgets::{
            background::state::BackgroundShaderState, poster::PosterInstanceKey,
        },
    },
};

use ferrex_core::player_prelude::{
    LibraryId, SortBy, SortOrder, UiDecade, UiGenre, UiResolution,
    UiWatchStatus,
};

use iced::Task;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use uuid::Uuid;

/// UI domain state
#[derive(Debug)]
pub struct UIDomainState {
    pub view: ViewState,

    pub repo_accessor: Accessor<ReadOnly>,

    pub movie_yoke_cache: YokeCache<MovieYoke>,
    pub series_yoke_cache: YokeCache<SeriesYoke>,
    pub season_yoke_cache: YokeCache<SeasonYoke>,
    pub episode_yoke_cache: YokeCache<EpisodeYoke>,

    pub movies_carousel: CarouselState,
    pub tv_carousel: CarouselState,

    pub scope: Scope,
    pub sort_by: SortBy,
    pub sort_order: SortOrder,
    pub loading: bool,
    pub error_message: Option<String>,
    pub window_size: iced::Size,

    // UI scaling state
    /// Composable scaling context (user preference, system DPI, accessibility)
    pub scaling_context: ScalingContext,
    /// Pre-computed scaled token values (fonts, spacing, icons, animations)
    pub size_provider: SizeProvider,
    /// Pre-computed scaled layout dimensions for virtual grids/carousels
    pub scaled_layout: ScaledLayout,
    /// Preview value during slider drag (not applied to UI until release)
    pub scale_slider_preview: Option<f32>,
    /// Text input value for manual scale entry
    pub scale_text_input: String,

    pub expanded_shows: HashSet<String>,
    pub hovered_media_id: Option<PosterInstanceKey>,

    pub theme_color_cache: parking_lot::RwLock<HashMap<Uuid, iced::Color>>,

    pub current_library_id: Option<LibraryId>,

    pub last_prefetch_tick: Option<Instant>,
    pub scroll_manager: ScrollPositionManager,

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

    // Motion controller
    pub motion_controller: motion_controller::MotionController,

    // Virtual carousel registry (new module)
    pub carousel_registry: CarouselRegistry,

    // Carousel focus controller - tracks which carousel receives keyboard events
    pub carousel_focus: CarouselFocus,

    // Poster menu open state (single target for now)
    pub poster_menu_open: Option<PosterInstanceKey>,
    pub poster_menu_states: HashMap<PosterInstanceKey, PosterMenuState>,

    // Toast notification manager
    pub toast_manager: feedback_ui::ToastManager,

    #[cfg(feature = "debug-cache-overlay")]
    pub cache_overlay_sample: Option<
        crate::domains::ui::views::cache_debug_overlay::CacheOverlaySample,
    >,
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
                self.state.scope = Scope::Library(*library_id);

                // Reset view state when library changes
                self.state.expanded_shows.clear();
                self.state.hovered_media_id = None;

                // Update filters - this already triggers refresh, no need to call RefreshViewModels
                Task::done(DomainMessage::Ui(
                    ViewModelMessage::UpdateViewModelFilters.into(),
                ))
            }
            CrossDomainEvent::LibrarySelectHome => {
                log::info!("UI domain handling LibrarySelectAll event");

                // Clear library selection - show all libraries
                self.state.current_library_id = None;
                self.state.scope = Scope::Home;

                // Reset view state
                self.state.expanded_shows.clear();
                self.state.hovered_media_id = None;

                // Update filters - this already triggers refresh, no need to call RefreshViewModels
                Task::done(DomainMessage::Ui(
                    ViewModelMessage::UpdateViewModelFilters.into(),
                ))
            }
            CrossDomainEvent::RequestLibraryRefresh => {
                // This is for actual data refresh, not just filter changes
                Task::done(DomainMessage::Ui(
                    ViewModelMessage::RefreshViewModels.into(),
                ))
            }
            CrossDomainEvent::SeriesChildrenChanged(series_id) => {
                // Invalidate cached yoke for the series and refresh
                self.state.series_yoke_cache.remove(&series_id.to_uuid());
                Task::done(DomainMessage::Ui(
                    ViewModelMessage::RefreshViewModels.into(),
                ))
            }
            CrossDomainEvent::SeasonChildrenChanged(season_id) => {
                // Invalidate cached yoke for the season and refresh
                self.state.season_yoke_cache.remove(&season_id.to_uuid());
                Task::done(DomainMessage::Ui(
                    ViewModelMessage::RefreshViewModels.into(),
                ))
            }
            CrossDomainEvent::RequestViewModelRefresh => {
                // Refresh all ViewModels when media has been loaded
                log::info!(
                    "UI domain received RequestViewModelRefresh event - display_mode: {:?}, current_library_id: {:?}",
                    self.state.scope,
                    self.state.current_library_id
                );

                // Ensure we're in a valid display mode
                if matches!(self.state.scope, Scope::Home) {
                    // Good - show all libraries
                    log::info!("UI: In Curated mode - will show all libraries");
                } else if matches!(self.state.scope, Scope::Library(_))
                    && self.state.current_library_id.is_none()
                {
                    // Bad state - Library mode but no library selected
                    log::warn!(
                        "UI: In Library mode but no library selected - switching to Curated"
                    );
                    self.state.scope = Scope::Home;
                }

                Task::done(DomainMessage::Ui(
                    ViewModelMessage::RefreshViewModels.into(),
                ))
            }
            _ => Task::none(),
        }
    }
}
