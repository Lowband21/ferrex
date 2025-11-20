use crate::api_types::{LibraryMediaCache, MediaReference, MovieReference, ScanProgress};
use crate::auth_manager::AuthManager;
use crate::image_types::ImageRequest;
use crate::media_library::Library;
use crate::media_store::MediaStore;
use crate::messages::{metadata::Message as MetadataMessage, DomainMessage};
use crate::models::{SeasonDetails, TvShowDetails};
use crate::player::PlayerState;
use crate::profiling::PROFILER;
use crate::security::SecureCredential;
use crate::transitions::{
    generate_random_gradient_center, BackdropTransitionState, ColorTransitionState,
    GradientTransitionState,
};
use crate::unified_image_service::UnifiedImageService;
use crate::view_models::ViewModel;
use crate::view_models::{AllViewModel, MoviesViewModel, TvViewModel};
use crate::views::carousel::CarouselState;
use crate::widgets::background_shader::{DepthRegion, EdgeTransition, RegionBorder};
use crate::widgets::{BackgroundEffect, DepthLayout};
use ferrex_core::api_types::MediaId;
use ferrex_core::watch_status::UserWatchState;
use ferrex_core::{EpisodeID, SeasonID, SeriesID};
use iced::Task;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Instant;
use uuid::Uuid;

/// Backdrop aspect ratio mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackdropAspectMode {
    /// Automatically select aspect ratio based on window dimensions
    Auto,
    /// Force 21:9 aspect ratio regardless of window dimensions
    Force21x9,
}

impl Default for BackdropAspectMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Persistent state for the background shader
#[derive(Debug, Clone)]
pub struct BackgroundShaderState {
    pub effect: BackgroundEffect,
    pub primary_color: iced::Color,
    pub secondary_color: iced::Color,
    pub backdrop_handle: Option<iced::widget::image::Handle>,
    pub backdrop_aspect_ratio: Option<f32>,
    pub backdrop_aspect_mode: BackdropAspectMode,
    pub scroll_offset: f32,
    pub gradient_center: (f32, f32),
    pub depth_layout: DepthLayout,

    // Transition states
    pub color_transitions: ColorTransitionState,
    pub backdrop_transitions: BackdropTransitionState,
    pub gradient_transitions: GradientTransitionState,
}

impl Default for BackgroundShaderState {
    fn default() -> Self {
        use crate::theme::MediaServerTheme;
        let primary = MediaServerTheme::SOFT_GREY_DARK;
        let secondary = MediaServerTheme::SOFT_GREY_LIGHT;
        let initial_center = generate_random_gradient_center();
        Self {
            effect: BackgroundEffect::Gradient,
            primary_color: primary,
            secondary_color: secondary,
            backdrop_handle: None,
            backdrop_aspect_ratio: None,
            backdrop_aspect_mode: BackdropAspectMode::default(),
            scroll_offset: 0.0,
            gradient_center: initial_center,
            depth_layout: DepthLayout {
                regions: Vec::new(),
                ambient_light_direction: iced::Vector::new(0.707, 0.707), // Light from bottom-right
                base_depth: 0.0,
                shadow_intensity: 0.4,
                shadow_distance: 40.0,
            },

            // Initialize transition states
            color_transitions: ColorTransitionState::new(primary, secondary),
            backdrop_transitions: BackdropTransitionState::new(),
            gradient_transitions: GradientTransitionState::new(initial_center),
        }
    }
}

impl BackgroundShaderState {
    /// Updates depth regions based on the current view and window size
    pub fn update_depth_lines(&mut self, view: &ViewState, window_width: f32, window_height: f32) {
        self.depth_layout.regions.clear();

        log::debug!(
            "Updating depth lines for view: {:?}, window: {}x{}",
            view,
            window_width,
            window_height
        );

        match view {
            ViewState::FirstRunSetup => {
                // No depth regions for first-run setup
            }
            ViewState::Library => {
                // Use consistent header height (errors will be toast notifications)
                let header_height = crate::constants::layout::header::HEIGHT;

                // Header region (sunken)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: window_width,
                        height: header_height,
                    },
                    depth: -5.0, // Header is 5 units deep
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: false, // No shadows for header
                    shadow_intensity: 0.0,
                    z_order: 0,
                    border: Some(RegionBorder {
                        width: 1.0,
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.2),
                        opacity: 1.0,
                    }),
                });

                // Content region (flat)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: header_height,
                        width: window_width,
                        height: window_height - header_height,
                    },
                    depth: 0.0, // Content is flat
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 1,
                    border: None,
                });

                log::debug!("Added library regions");
            }
            ViewState::MovieDetail { .. }
            | ViewState::TvShowDetail { .. }
            | ViewState::SeasonDetail { .. }
            | ViewState::EpisodeDetail { .. } => {
                // Account for scroll offset
                let scroll_offset = self.scroll_offset;
                // Calculate dynamic backdrop height based on aspect mode and window dimensions
                let backdrop_aspect = self.calculate_display_aspect(window_width, window_height);
                let backdrop_height = window_width / backdrop_aspect;
                let _metadata_offset = 150.0;
                // Content top is just backdrop height since header is outside scrollable
                let content_top = backdrop_height - scroll_offset;
                let poster_width = 300.0;
                let poster_height = 450.0;
                let poster_padding = 10.0;
                let poster_left = 0.0;
                let poster_right = poster_left + poster_width + 37.5;
                let poster_bottom = content_top + poster_height + poster_padding;

                // Backdrop region (flat, no shadows)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: 0.0, // Now starts at top since header is outside scrollable
                        width: window_width,
                        height: backdrop_height,
                    },
                    depth: 0.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: false,
                    shadow_intensity: 0.0,
                    z_order: 1,
                    border: None,
                });

                // Poster region (sunken)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: poster_left,
                        y: content_top,
                        width: poster_right,
                        height: poster_height + 30.0,
                    },
                    depth: -2.0,
                    edge_transition: EdgeTransition::Soft { width: 2.0 },
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 3,
                    border: None,
                });

                // Metadata region (surface level)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: poster_right,
                        y: content_top,
                        width: window_width - poster_right,
                        height: poster_height + 30.0,
                    },
                    depth: 0.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 2,
                    border: None,
                });

                // Content region below (slightly sunken)
                self.depth_layout.regions.push(DepthRegion {
                    bounds: iced::Rectangle {
                        x: 0.0,
                        y: poster_bottom,
                        width: window_width,
                        height: window_height.max(poster_bottom) - poster_bottom,
                    },
                    depth: -2.0,
                    edge_transition: EdgeTransition::Sharp,
                    edge_overrides: Default::default(),
                    shadow_enabled: true,
                    shadow_intensity: 1.0,
                    z_order: 2,
                    border: None,
                });

                log::debug!(
                    "Total depth regions for detail view: {}",
                    self.depth_layout.regions.len()
                );
            }
            _ => {
                // No depth lines for other views
            }
        }
    }

    /// Calculate content offset for detail views based on backdrop dimensions with known window height
    pub fn calculate_content_offset_with_height(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> f32 {
        use crate::constants::layout::header;

        // Calculate the display aspect based on current mode and actual window dimensions
        let backdrop_aspect = self.calculate_display_aspect(window_width, window_height);

        // Calculate the backdrop height based on window width
        let backdrop_height = window_width / backdrop_aspect;

        // Content starts after header + backdrop
        backdrop_height - header::HEIGHT - 25.0
    }

    /// Calculate the display aspect ratio based on mode and window dimensions
    pub fn calculate_display_aspect(&self, window_width: f32, window_height: f32) -> f32 {
        use crate::constants::layout::backdrop;

        match self.backdrop_aspect_mode {
            BackdropAspectMode::Force21x9 => backdrop::DISPLAY_ASPECT,
            BackdropAspectMode::Auto => {
                // Use 30:9 for wide windows, 21:9 for tall windows
                if window_width >= window_height {
                    backdrop::DISPLAY_ASPECT_ULTRAWIDE
                } else {
                    backdrop::DISPLAY_ASPECT
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum ViewState {
    #[default]
    Library,
    LibraryManagement, // New view for library management
    AdminDashboard,    // New comprehensive admin dashboard
    FirstRunSetup,     // First-run admin setup view
    Player,
    LoadingVideo {
        url: String,
    },
    VideoError {
        message: String,
    },
    MovieDetail {
        movie: MovieReference, // Store full reference for efficient access
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    TvShowDetail {
        series_id: SeriesID, // Keep as string for now, will convert to SeriesID later
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    SeasonDetail {
        series_id: SeriesID,
        season_id: SeasonID,
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    EpisodeDetail {
        episode_id: EpisodeID,                                // Keep as string for now
        backdrop_handle: Option<iced::widget::image::Handle>, // Cached backdrop handle
    },
    UserSettings,  // User settings and preferences view
}

impl ViewState {
    /// Returns true if this view should show the main header
    pub fn has_header(&self) -> bool {
        matches!(
            self,
            ViewState::Library
                | ViewState::LibraryManagement
                | ViewState::AdminDashboard
                | ViewState::UserSettings
                | ViewState::MovieDetail { .. }
                | ViewState::TvShowDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. }
        )
        // FirstRunSetup has no header
    }

    /// Returns true if this view should show the background shader
    pub fn has_background(&self) -> bool {
        !matches!(
            self,
            ViewState::Player | ViewState::LoadingVideo { .. } | ViewState::FirstRunSetup
        )
    }

    /// Returns header height in pixels if this view has a header
    pub fn header_height(&self) -> Option<f32> {
        if self.has_header() {
            match self {
                ViewState::Library => Some(crate::constants::layout::header::HEIGHT), // Main library header
                ViewState::MovieDetail { .. }
                | ViewState::TvShowDetail { .. }
                | ViewState::SeasonDetail { .. }
                | ViewState::EpisodeDetail { .. } => Some(crate::constants::layout::header::HEIGHT), // Same header height
                ViewState::LibraryManagement | ViewState::AdminDashboard | ViewState::UserSettings => {
                    Some(crate::constants::layout::header::HEIGHT)
                } // Same header height
                _ => None,
            }
        } else {
            None
        }
    }

    /// Returns layout regions for background shader effects
    /// (For future use with shadows and visual divisions)
    pub fn layout_regions(&self) -> LayoutRegions {
        LayoutRegions {
            header_height: self.header_height(),
            has_sidebar: matches!(self, ViewState::AdminDashboard),
            content_padding: match self {
                ViewState::Library => 0.0, // No padding, grid goes edge to edge
                ViewState::Player => 0.0,
                _ => 20.0, // Standard content padding
            },
        }
    }
}

/// Layout information for background shader effects
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct LayoutRegions {
    pub header_height: Option<f32>,
    pub has_sidebar: bool,
    pub content_padding: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ViewMode {
    All,
    #[default]
    Movies,
    TvShows,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortBy {
    #[default]
    DateAdded,
    Title,
    Year,
    Rating,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SortOrder {
    #[default]
    Descending,
    Ascending,
}

// Library form data for creating/editing libraries
#[derive(Debug, Clone)]
pub struct LibraryFormData {
    pub id: Uuid,
    pub name: String,
    pub library_type: String,
    pub paths: String, // comma-separated paths as entered by user
    pub scan_interval_minutes: String,
    pub enabled: bool,
    pub editing: bool, // true if editing existing library, false if creating new
}

/// Authentication credential type
#[derive(Debug, Clone)]
pub enum CredentialType {
    Password,
    Pin { max_length: usize },
}

/// Authentication mode for offline support
#[derive(Debug, Clone)]
pub enum AuthenticationMode {
    Online,
    Cached,  // Offline with cached credentials
    Limited, // Read-only mode when auth fails
}

/// Authentication flow state
#[derive(Debug, Clone)]
pub enum AuthenticationFlow {
    /// Initial state, checking if server needs setup
    CheckingSetup,

    /// First-run admin setup
    FirstRunSetup {
        username: String,
        password: SecureCredential,
        confirm_password: SecureCredential,
        display_name: String,
        setup_token: String,
        show_password: bool,
        error: Option<String>,
        loading: bool,
    },

    /// Loading users from server
    LoadingUsers,

    /// User selection screen
    SelectingUser {
        users: Vec<crate::auth_dto::UserListItemDto>,
        error: Option<String>,
    },

    /// Checking device status after user selection
    CheckingDevice { user: ferrex_core::user::User },

    /// Credential input (unified for password/PIN)
    EnteringCredentials {
        user: ferrex_core::user::User,
        input_type: CredentialType,
        input: SecureCredential,
        show_password: bool,
        remember_device: bool,
        error: Option<String>,
        attempts_remaining: Option<u8>,
        loading: bool,
    },

    /// Setting up PIN after first login
    SettingUpPin {
        user: ferrex_core::user::User,
        pin: SecureCredential,
        confirm_pin: SecureCredential,
        error: Option<String>,
    },

    /// Successfully authenticated
    Authenticated {
        user: ferrex_core::user::User,
        mode: AuthenticationMode,
    },
}

impl Default for AuthenticationFlow {
    fn default() -> Self {
        AuthenticationFlow::CheckingSetup
    }
}

/// Legacy compatibility - will be removed
pub type AuthViewState = AuthenticationFlow;

/// Settings subview state
#[derive(Debug, Clone, Default)]
pub enum SettingsSubview {
    #[default]
    Main,
    Profile,
    Preferences,
    Security,
    DeviceManagement,
}

/// Security settings state
#[derive(Debug, Clone)]
pub struct SecuritySettingsState {
    // Password change
    pub showing_password_change: bool,
    pub password_current: SecureCredential,
    pub password_new: SecureCredential,
    pub password_confirm: SecureCredential,
    pub password_show: bool,
    pub password_error: Option<String>,
    pub password_loading: bool,
    
    // PIN management
    pub showing_pin_change: bool,
    pub has_pin: bool,
    pub pin_current: SecureCredential,
    pub pin_new: SecureCredential,
    pub pin_confirm: SecureCredential,
    pub pin_error: Option<String>,
    pub pin_loading: bool,
}

impl Default for SecuritySettingsState {
    fn default() -> Self {
        Self {
            showing_password_change: false,
            password_current: SecureCredential::from(""),
            password_new: SecureCredential::from(""),
            password_confirm: SecureCredential::from(""),
            password_show: false,
            password_error: None,
            password_loading: false,
            showing_pin_change: false,
            has_pin: false,
            pin_current: SecureCredential::from(""),
            pin_new: SecureCredential::from(""),
            pin_confirm: SecureCredential::from(""),
            pin_error: None,
            pin_loading: false,
        }
    }
}

// Application state
#[derive(Debug)]
pub struct State {
    // Current view state
    pub view: ViewState,
    pub view_mode: ViewMode, // All, Movies, or TV Shows
    pub sort_by: SortBy,
    pub sort_order: SortOrder,

    // Server configuration
    pub server_url: String,

    // Library management
    pub libraries: Vec<Library>,
    pub current_library_id: Option<Uuid>,
    pub show_library_management: bool,

    // NEW ARCHITECTURE: Single source of truth
    pub media_store: Arc<StdRwLock<MediaStore>>,

    // NEW ARCHITECTURE: View models
    pub all_view_model: AllViewModel,
    pub movies_view_model: MoviesViewModel,
    pub tv_view_model: TvViewModel,

    // Metadata fetch service (being phased out)
    pub metadata_service: Option<Arc<crate::metadata_service::MetadataFetchService>>,

    // NEW SIMPLIFIED APPROACH: Batch metadata fetcher
    pub batch_metadata_fetcher: Option<Arc<crate::batch_metadata_fetcher::BatchMetadataFetcher>>,

    // NEW ARCHITECTURE: Metadata coordinator (being phased out)
    pub metadata_coordinator: crate::metadata_coordinator::MetadataCoordinator,

    // Per-library media cache for instant switching
    pub library_media_cache: HashMap<Uuid, LibraryMediaCache>,

    // Library form
    pub library_form_data: Option<LibraryFormData>,
    pub library_form_errors: Vec<String>,

    // Player module state
    pub player: PlayerState,

    // UI state
    pub loading: bool,
    pub error_message: Option<String>,
    pub scanning: bool,

    // Scan progress tracking
    pub active_scan_id: Option<String>,
    pub scan_progress: Option<ScanProgress>,
    pub show_scan_progress: bool, // Show scan progress overlay

    // Database maintenance
    pub show_clear_database_confirm: bool, // Show confirmation dialog for database clearing

    // Image services
    pub image_service: UnifiedImageService,
    pub image_receiver: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<ImageRequest>>>>,

    // UI state for expanded shows in TV view
    pub expanded_shows: HashSet<String>,

    // Window state
    pub window_size: iced::Size,

    // TV show detail data
    pub current_season_details: Option<SeasonDetails>,

    // Carousel states for detail views
    pub show_seasons_carousel: Option<CarouselState>,
    pub season_episodes_carousel: Option<CarouselState>,

    // Detail view data (stored here for proper lifetimes)
    pub current_show_seasons: Vec<crate::api_types::SeasonReference>,
    pub current_season_episodes: Vec<crate::api_types::EpisodeReference>,

    // Track which posters are being loaded
    pub loading_posters: HashSet<String>,
    pub tmdb_poster_urls: HashMap<String, String>, // media_id -> TMDB URL

    // Scroll throttling
    pub last_scroll_time: Option<Instant>,

    // Scroll velocity tracking
    pub scroll_velocity: f32,
    pub last_scroll_position: f32,
    pub scroll_samples: VecDeque<(Instant, f32)>, // (timestamp, position) for velocity calculation
    pub fast_scrolling: bool,
    pub scroll_stopped_time: Option<Instant>, // When scrolling stopped for debouncing

    // Scroll position persistence
    pub movies_scroll_position: Option<f32>, // Saved scroll position for movies view
    pub tv_shows_scroll_position: Option<f32>, // Saved scroll position for TV shows view

    // Hover state
    pub hovered_media_id: Option<String>,
    // Watch status tracking
    pub user_watch_state: Option<UserWatchState>,

    // Track recently attempted metadata fetches to prevent infinite loops
    pub metadata_fetch_attempts: HashMap<String, Instant>,

    // Global scroll position manager
    pub scroll_manager: crate::scroll_manager::ScrollPositionManager,

    // Persistent background shader state
    pub background_shader_state: BackgroundShaderState,

    // Header state
    pub search_query: String,
    pub show_library_menu: bool,
    pub library_menu_target: Option<Uuid>, // None for "All Libraries" menu
    pub is_fullscreen: bool,

    // Authentication state
    pub auth_manager: Option<crate::auth_manager::AuthManager>,
    pub api_client: Option<crate::api_client::ApiClient>,
    pub is_authenticated: bool,
    pub auth_flow: AuthenticationFlow,
    pub user_permissions: Option<ferrex_core::rbac::UserPermissions>,
    pub first_run_state: crate::views::first_run::FirstRunState,
    
    // Settings state
    pub settings_subview: SettingsSubview,
    pub security_settings_state: SecuritySettingsState,
    pub device_management_state: crate::views::settings::device_management::DeviceManagementState,
    pub auto_login_enabled: bool,
}

impl State {
    /// Fetch media details on-demand for a specific media item
    /// Checks if details already exist, and if not, fetches them from the server
    pub fn fetch_media_details_on_demand(
        &self,
        library_id: Uuid,
        media_id: MediaId,
    ) -> Task<DomainMessage> {
        // Check if the media already has details in MediaStore
        if let Ok(store) = self.media_store.read() {
            if let Some(media_ref) = store.get(&media_id) {
                // Check if we need to fetch details using the api_types helper
                let needs_fetch = match media_ref {
                    MediaReference::Movie(m) => crate::api_types::needs_details_fetch(&m.details),
                    MediaReference::Series(s) => crate::api_types::needs_details_fetch(&s.details),
                    MediaReference::Season(s) => crate::api_types::needs_details_fetch(&s.details),
                    MediaReference::Episode(e) => crate::api_types::needs_details_fetch(&e.details),
                };
                if !needs_fetch {
                    // Details already exist, no need to fetch
                    return Task::none();
                }
            }
        }

        // Details needed, fetch from server
        let server_url = self.server_url.clone();
        Task::perform(
            crate::media_library::fetch_media_details(server_url, library_id, media_id.clone()),
            move |result| match result {
                Ok(media_ref) => {
                    DomainMessage::Metadata(MetadataMessage::MediaDetailsUpdated(media_ref))
                }
                Err(e) => {
                    log::error!("Failed to fetch media details for {:?}: {}", media_id, e);
                    DomainMessage::Metadata(MetadataMessage::NoOp)
                }
            },
        )
    }

    /// Save the current scroll position based on the view mode
    pub fn save_scroll_position(&mut self) {
        match self.view_mode {
            ViewMode::Movies => {
                self.movies_scroll_position = Some(self.last_scroll_position);
            }
            ViewMode::TvShows => {
                self.tv_shows_scroll_position = Some(self.last_scroll_position);
            }
            ViewMode::All => {
                // In All mode, save current position
                // Grid visibility now handled by ViewModels
            }
        }
    }

    /// Check if a media item is currently visible in either movies or TV shows grid
    pub fn is_media_visible(&self, _media_id: &str) -> bool {
        // Visibility tracking removed - no longer needed with new architecture
        false
    }

    /// Mark visible posters for loading, respecting the concurrent loading limit
    /// Only marks as many posters as there are available loading slots
    pub fn mark_visible_posters_for_loading(&mut self) -> Vec<String> {
        PROFILER.start("mark_visible_posters");
        // Visibility tracking now handled by ViewModels
        PROFILER.end("mark_visible_posters");
        Vec::new()

        /* DEPRECATED - OLD IMPLEMENTATION
        const MAX_CONCURRENT_LOADS: usize = performance_config::posters::MAX_CONCURRENT_LOADS;
        const PRELOAD_AHEAD_ROWS: usize = performance_config::scrolling::PRELOAD_AHEAD_ROWS;
        const PRELOAD_BELOW_ROWS: usize = performance_config::scrolling::PRELOAD_BELOW_ROWS;

        // Calculate available slots
        let current_loading = self.loading_posters.len();
        let available_slots = MAX_CONCURRENT_LOADS.saturating_sub(current_loading);

        if available_slots == 0 {
            PROFILER.end("mark_visible_posters");
            return Vec::new();
        }

        let mut posters_to_mark = Vec::new();

        // Check movies in visible range first
        match self.view_mode {
            ViewMode::All | ViewMode::Movies => {
                for index in self.movies_grid_state.visible_range.clone() {
                    if posters_to_mark.len() >= available_slots {
                        break;
                    }

                    if let Some(movie) = self.movies.get(index) {
                        // Only mark if not already loading
                        if !self.loading_posters.contains(&movie.id) {
                            posters_to_mark.push(movie.id.clone());
                        }
                    }
                }

                // If we still have slots and no visible items need loading, preload ahead
                if posters_to_mark.len() < available_slots {
                    let preload_range =
                        self.movies_grid_state.get_preload_range(PRELOAD_AHEAD_ROWS);
                    for index in preload_range {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(movie) = self.movies.get(index) {
                            if !self.loading_posters.contains(&movie.id) {
                                posters_to_mark.push(movie.id.clone());
                            }
                        }
                    }
                }

                // If we still have slots after visible and preload ahead, load items below current position
                if posters_to_mark.len() < available_slots
                    && !self.movies_grid_state.visible_range.is_empty()
                {
                    let visible_end = self.movies_grid_state.visible_range.end;
                    let items_per_row = self.movies_grid_state.columns;
                    let below_end =
                        (visible_end + (PRELOAD_BELOW_ROWS * items_per_row)).min(self.movies.len());

                    for index in visible_end..below_end {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(movie) = self.movies.get(index) {
                            if !self.loading_posters.contains(&movie.id) {
                                posters_to_mark.push(movie.id.clone());
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Check TV shows in visible range if we still have slots
        if posters_to_mark.len() < available_slots {
            match self.view_mode {
                ViewMode::All | ViewMode::TvShows => {
                    for index in self.tv_shows_grid_state.visible_range.clone() {
                        if posters_to_mark.len() >= available_slots {
                            break;
                        }

                        if let Some(series) = self.series_references_sorted.get(index) {
                            let media_id = series.id.as_str();

                            // Only mark if not already loading
                            if !self.loading_posters.contains(media_id) {
                                posters_to_mark.push(media_id.to_string());
                            }
                        }
                    }

                    // If we still have slots and no visible shows need loading, preload ahead
                    if posters_to_mark.len() < available_slots {
                        let preload_range = self
                            .tv_shows_grid_state
                            .get_preload_range(PRELOAD_AHEAD_ROWS);
                        for index in preload_range {
                            if posters_to_mark.len() >= available_slots {
                                break;
                            }

                            if let Some(series) = self.series_references_sorted.get(index) {
                                let media_id = series.id.as_str();

                                if !self.loading_posters.contains(media_id) {
                                    posters_to_mark.push(media_id.to_string());
                                }
                            }
                        }
                    }

                    // If we still have slots after visible and preload ahead, load shows below current position
                    if posters_to_mark.len() < available_slots
                        && !self.tv_shows_grid_state.visible_range.is_empty()
                    {
                        let visible_end = self.tv_shows_grid_state.visible_range.end;
                        let items_per_row = self.tv_shows_grid_state.columns;
                        let below_end = (visible_end + (PRELOAD_BELOW_ROWS * items_per_row))
                            .min(self.series_references_sorted.len());

                        for index in visible_end..below_end {
                            if posters_to_mark.len() >= available_slots {
                                break;
                            }

                            if let Some(series) = self.series_references_sorted.get(index) {
                                let media_id = series.id.as_str();

                                if !self.loading_posters.contains(media_id) {
                                    posters_to_mark.push(media_id.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Mark the selected posters as loading
        for poster_id in &posters_to_mark {
            self.loading_posters.insert(poster_id.clone());
        }

        PROFILER.end("mark_visible_posters");
        posters_to_mark
        */
    }

    /// Get library_id for a media item from MediaStore
    pub fn get_library_id_for_media(&self, media_id: &MediaId) -> Option<Uuid> {
        if let Ok(store) = self.media_store.read() {
            store.get_library_id(media_id)
        } else {
            None
        }
    }

    /// Queue visible media items for background detail fetching
    pub fn queue_visible_media_for_details(&mut self) -> Vec<(MediaId, Uuid)> {
        // No longer needed - batch metadata fetching handles all items automatically
        // DEPRECATED: Return empty for compatibility
        // The MetadataCoordinator handles all queuing internally now
        Vec::new()

        // DEPRECATED: Old implementation preserved for reference
        /*
        if let Some(service) = &self.metadata_service {
            let mut items_to_fetch = Vec::new();

            // In "All" view, use carousel visibility
            if self.view_mode == ViewMode::All {
                // Queue visible movies from carousel
                let movie_range = self.movies_carousel.get_visible_range();
                for index in movie_range {
                    if let Some(movie) = self.movie_references.get(index) {
                        let media_id = MediaId::Movie(movie.id.clone());
                        if crate::api_types::needs_details_fetch(&movie.details) {
                            items_to_fetch.push((media_id, movie.file.library_id));
                        }
                    }
                }

                // Queue visible TV shows from carousel
                let tv_range = self.tv_shows_carousel.get_visible_range();
                for index in tv_range {
                    if let Some(series) = self.series_references_sorted.get(index) {
                        let media_id = MediaId::Series(series.id.clone());
                        if crate::api_types::needs_details_fetch(&series.details) {
                            items_to_fetch.push((media_id, series.library_id));
                        }
                    }
                }
            } else {
                // For Movies/TvShows views, use grid visibility
                if matches!(self.view_mode, ViewMode::Movies) {
                    for index in self.movies_grid_state.visible_range.clone() {
                        if let Some(movie) = self.movie_references.get(index) {
                            let media_id = MediaId::Movie(movie.id.clone());
                            // Check if details are needed
                            if crate::api_types::needs_details_fetch(&movie.details) {
                                items_to_fetch.push((media_id, movie.file.library_id));
                            }
                        }
                    }

                    // Also queue items in the preload range (2 rows ahead)
                    let preload_range = self.movies_grid_state.get_preload_range(2);
                    for index in preload_range {
                        if let Some(movie) = self.movie_references.get(index) {
                            let media_id = MediaId::Movie(movie.id.clone());
                            // Check if details are needed
                            if crate::api_types::needs_details_fetch(&movie.details) {
                                items_to_fetch.push((media_id, movie.file.library_id));
                            }
                        }
                    }
                }

                // Queue visible TV shows
                if matches!(self.view_mode, ViewMode::TvShows) {
                    for index in self.tv_shows_grid_state.visible_range.clone() {
                        if let Some(series) = self.series_references_sorted.get(index) {
                            let media_id = MediaId::Series(series.id.clone());
                            // Check if details are needed
                            if crate::api_types::needs_details_fetch(&series.details) {
                                items_to_fetch.push((media_id, series.library_id));
                            }
                        }
                    }

                    // Also queue items in the preload range (2 rows ahead)
                    let preload_range = self.tv_shows_grid_state.get_preload_range(2);
                    for index in preload_range {
                        if let Some(series) = self.series_references_sorted.get(index) {
                            let media_id = MediaId::Series(series.id.clone());
                            // Check if details are needed
                            if crate::api_types::needs_details_fetch(&series.details) {
                                items_to_fetch.push((media_id, series.library_id));
                            }
                        }
                    }
                }
            }

            // Queue detail view items
            match &self.view {
                ViewState::TvShowDetail { series_id, .. } => {
                    if let Some(series_ref) = self.series_references.get(series_id.as_str()) {
                        // Queue series details
                        if crate::api_types::needs_details_fetch(&series_ref.details) {
                            items_to_fetch.push((MediaId::Series(series_ref.id.clone()), series_ref.library_id));
                        }

                        // Queue all seasons for the show
                        if let Some(seasons) = self.season_references.get(series_ref.id.as_str()) {
                            for season in seasons {
                                if crate::api_types::needs_details_fetch(&season.details) {
                                    items_to_fetch.push((MediaId::Season(season.id.clone()), series_ref.library_id));
                                }
                            }
                        }
                    }
                }
                ViewState::SeasonDetail {
                    series_id,
                    season_id,
                    ..
                } => {
                    if let Some(series_ref) = self.series_references.get(series_id.as_str()) {
                        if let Some(seasons) = self.season_references.get(series_ref.id.as_str()) {
                            if let Some(season) = seasons.iter().find(|s| s.id.eq(season_id)) {
                                // Queue season details
                                if crate::api_types::needs_details_fetch(&season.details) {
                                    items_to_fetch.push((MediaId::Season(season.id.clone()), series_ref.library_id));
                                }

                                // Queue all episodes for the season
                                if let Some(episodes) =
                                    self.episode_references.get(season.id.as_str())
                                {
                                    for episode in episodes {
                                        if crate::api_types::needs_details_fetch(&episode.details) {
                                            items_to_fetch
                                                .push((MediaId::Episode(episode.id.clone()), series_ref.library_id));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            items_to_fetch
        } else {
            Vec::new()
        }
        */
    }

    // Metadata service initialization removed - using BatchMetadataFetcher instead
    pub fn init_metadata_service_with_sender(
        &mut self,
        _sender: tokio::sync::mpsc::UnboundedSender<crate::messages::DomainMessage>,
    ) {
        log::debug!("init_metadata_service_with_sender called but metadata service is deprecated");
        // No longer initializing metadata service - BatchMetadataFetcher handles all metadata fetching
    }

    // Metadata service initialization removed - using BatchMetadataFetcher instead
    pub fn init_metadata_service(&mut self) {
        log::debug!("init_metadata_service called but metadata service is deprecated");
        // No longer initializing metadata service - BatchMetadataFetcher handles all metadata fetching
    }

    /// Stop background details fetching (legacy method - kept for compatibility)
    pub fn stop_details_fetcher(&mut self) {
        // The metadata service handles its own lifecycle
        // This method is kept for compatibility but does nothing
    }

    /// Get poster IDs that should be loaded for visible MediaReference items
    pub fn get_posters_to_load_for_references(&self) -> Vec<String> {
        // Poster loading now handled by ViewModels and UnifiedImageService
        // This method is kept for compatibility but returns empty
        Vec::new()
    }

    /// Efficiently update sorted series references
    /// This method avoids cloning all SeriesReference objects unnecessarily
    pub fn update_sorted_series_references(&mut self) {
        use crate::profiling::PROFILER;
        PROFILER.start("update_sorted_series_references");

        // Sorting now handled by ViewModels internally
        // This method is kept for compatibility but does nothing

        PROFILER.end("update_sorted_series_references");
    }
}

impl State {
    /// Get a media reference by MediaId
    /// Returns the appropriate MediaReference type based on the MediaId variant
    pub fn get_media_by_id(&self, media_id: &MediaId) -> Option<crate::api_types::MediaReference> {
        if let Ok(store) = self.media_store.read() {
            store.get(media_id).cloned()
        } else {
            None
        }
    }

    /// Get episode count for a season
    pub fn get_season_episode_count(&self, season_id: &str) -> u32 {
        if let Ok(store) = self.media_store.read() {
            store.get_episodes(season_id).len() as u32
        } else {
            0
        }
    }

    // Legacy methods removed - use MediaStore directly

    // Watch status helper methods

    /// Get the watch progress for a specific media item
    /// Returns Some(progress) where progress is 0.0-1.0, or None if no watch state loaded
    pub fn get_media_progress(&self, media_id: &MediaId) -> Option<f32> {
        if let Some(ref watch_state) = self.user_watch_state {
            // Check if it's in progress
            if let Some(in_progress) = watch_state
                .in_progress
                .iter()
                .find(|item| &item.media_id == media_id)
            {
                if in_progress.duration > 0.0 {
                    return Some((in_progress.position / in_progress.duration).clamp(0.0, 1.0));
                }
            }

            // Check if it's completed
            if watch_state.completed.contains(media_id) {
                return Some(1.0);
            }

            // If we have watch state but item isn't in progress or completed, it's unwatched
            Some(0.0)
        } else {
            // No watch state loaded yet
            None
        }
    }

    /// Check if a media item has been watched (>= 95% completion)
    pub fn is_watched(&self, media_id: &MediaId) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state.completed.contains(media_id)
        } else {
            false
        }
    }

    /// Check if a media item is currently in progress
    pub fn is_in_progress(&self, media_id: &MediaId) -> bool {
        if let Some(ref watch_state) = self.user_watch_state {
            watch_state
                .in_progress
                .iter()
                .any(|item| &item.media_id == media_id)
        } else {
            false
        }
    }

    /// Get watch status for UI display
    /// Returns: 0.0 for unwatched, 0.0-0.95 for in-progress, 1.0 for watched
    pub fn get_watch_status(&self, media_id: &MediaId) -> f32 {
        self.get_media_progress(media_id).unwrap_or(0.0)
    }
}

impl Default for State {
    fn default() -> Self {
        // Create shared media store
        let media_store = Arc::new(StdRwLock::new(MediaStore::new()));

        // Create view models with the shared store
        let all_view_model = AllViewModel::new(Arc::clone(&media_store));
        let movies_view_model = MoviesViewModel::new(Arc::clone(&media_store));
        let tv_view_model = TvViewModel::new(Arc::clone(&media_store));

        // Subscribe ViewModels to MediaStore for change notifications
        // We need to create Arc wrappers for the ViewModels to subscribe them
        // This is done after State creation in State::new_with_server_url()

        Self {
            view: ViewState::default(),
            view_mode: ViewMode::default(),
            sort_by: SortBy::default(),
            sort_order: SortOrder::default(),
            server_url: String::new(),

            // Library management
            libraries: Vec::new(),
            current_library_id: None,
            show_library_management: false,

            // NEW ARCHITECTURE
            media_store,
            all_view_model,
            movies_view_model,
            tv_view_model,

            // Metadata fetch service
            metadata_service: None,
            batch_metadata_fetcher: None,

            // NEW ARCHITECTURE: Metadata coordinator
            metadata_coordinator: crate::metadata_coordinator::MetadataCoordinator::new(),
            library_media_cache: HashMap::new(),

            // Library form
            library_form_data: None,
            library_form_errors: Vec::new(),

            player: PlayerState::default(),
            loading: false,
            error_message: None,
            scanning: false,
            active_scan_id: None,
            scan_progress: None,
            show_scan_progress: false,
            show_clear_database_confirm: false,
            // TODO: Properly integrate UnifiedImageService with background task
            // For now, create with temporary values
            image_service: {
                let (service, _receiver) = UnifiedImageService::new(4);
                service
            },
            image_receiver: Arc::new(Mutex::new(None)), // Will be set during proper initialization
            expanded_shows: HashSet::new(),
            window_size: iced::Size::new(1280.0, 720.0), // Default window size
            current_season_details: None,
            show_seasons_carousel: None,
            season_episodes_carousel: None,
            current_show_seasons: Vec::new(),
            current_season_episodes: Vec::new(),
            loading_posters: HashSet::new(),
            tmdb_poster_urls: HashMap::new(),
            last_scroll_time: None,
            scroll_velocity: 0.0,
            last_scroll_position: 0.0,
            scroll_samples: VecDeque::with_capacity(5), // Keep last 5 samples for smoothing
            fast_scrolling: false,
            scroll_stopped_time: None,
            movies_scroll_position: None,
            tv_shows_scroll_position: None,
            hovered_media_id: None,
            user_watch_state: None,
            metadata_fetch_attempts: HashMap::new(),
            scroll_manager: crate::scroll_manager::ScrollPositionManager::new(),
            background_shader_state: BackgroundShaderState::default(),

            // Header state
            search_query: String::new(),
            show_library_menu: false,
            library_menu_target: None,
            is_fullscreen: false,

            // Authentication state
            auth_manager: None,
            api_client: None,
            is_authenticated: false,
            auth_flow: AuthenticationFlow::default(),
            user_permissions: None,
            first_run_state: crate::views::first_run::FirstRunState::default(),
            
            // Settings state
            settings_subview: SettingsSubview::default(),
            security_settings_state: SecuritySettingsState::default(),
            device_management_state: crate::views::settings::device_management::DeviceManagementState::default(),
            auto_login_enabled: false,
        }
    }
}
