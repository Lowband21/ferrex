//! Refactored State using domain-driven architecture
//!
//! This new State structure delegates domain-specific state to the DomainRegistry,
//! keeping only the view models and cross-cutting concerns at the top level.

use crate::domains::DomainRegistry;
use crate::domains::auth::AuthDomainState;
use crate::domains::auth::AuthManager;
use crate::domains::library::LibraryDomainState;
use crate::domains::media::MediaDomainState;
use crate::domains::metadata::MetadataDomainState;
use crate::domains::metadata::image_service::UnifiedImageService;
use crate::domains::player::PlayerDomain;
use crate::domains::search::SearchDomain;
use crate::domains::settings::SettingsDomainState;
use crate::domains::streaming::StreamingDomainState;
use crate::domains::ui::UIDomainState;
use crate::domains::ui::tabs::{TabId, TabManager};
use crate::domains::ui::types::DisplayMode;
use crate::domains::ui::views::carousel::CarouselState;
use crate::domains::ui::widgets::AnimationType;
use crate::domains::user_management::UserManagementDomainState;
use crate::infrastructure::ServiceBuilder;
use crate::infrastructure::adapters::ApiClientAdapter;
use crate::infrastructure::adapters::AuthManagerAdapter;
use crate::infrastructure::api_client::ApiClient;
use crate::infrastructure::constants::animation::DEFAULT_DURATION_MS;
use crate::infrastructure::constants::animation::DEFAULT_POSTER_ANIMATION;
use crate::infrastructure::constants::animation::PosterAnimationKind;
use crate::infrastructure::repository::{
    accessor::{Accessor, ReadOnly, ReadWrite},
    repository::MediaRepo,
};
use crate::infrastructure::services::api::ApiService;
use crate::infrastructure::services::settings::SettingsApiAdapter;
use crate::infrastructure::services::streaming::StreamingApiAdapter;
use crate::infrastructure::services::user_management::UserAdminApiAdapter;
use ferrex_core::player_prelude::{
    LibraryID, SortBy, SortOrder, UiResolution, UiWatchStatus,
};
use parking_lot::RwLock as StdRwLock;
use std::sync::Arc;

/// Application state - refactored to use domain-driven architecture
#[derive(Debug)]
pub struct State {
    /// Domain registry containing all domain-specific state
    pub domains: DomainRegistry,

    /// Tab manager for independent tab states (NEW ARCHITECTURE)
    pub tab_manager: TabManager,

    /// Server URL - needed by multiple domains
    pub server_url: String,

    /// Shared services and infrastructure
    pub api_service: Arc<dyn ApiService>,
    pub image_service: UnifiedImageService,
    pub image_receiver:
        Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,

    //pub batch_metadata_fetcher:
    //    Option<Arc<crate::domains::metadata::batch_fetcher::BatchMetadataFetcher>>,
    /// Top-level application state
    pub loading: bool,
    pub is_authenticated: bool,
    pub window_size: iced::Size,
    pub window_position: Option<iced::Point>,
    pub is_fullscreen: bool,

    /// MediaRepo for new architecture - single source of truth for media/library data
    pub media_repo: Arc<StdRwLock<Option<MediaRepo>>>,
}

impl State {
    /// Create a new State with the given server URL
    pub fn new(server_url: String) -> Self {
        // Create shared resources
        // Initialize MediaRepo (will be populated when libraries are loaded)
        let media_repo = Arc::new(StdRwLock::new(None));
        let ui_accessor: Accessor<ReadOnly> = Accessor::new(media_repo.clone());
        let lib_accessor: Accessor<ReadWrite> =
            Accessor::new(media_repo.clone());
        // Media and library domains should be combined
        let media_accessor: Accessor<ReadWrite> =
            Accessor::new(media_repo.clone());

        let api_client = ApiClient::new(server_url.clone());
        let (image_service, _receiver) = UnifiedImageService::new(16);

        // Create service builder/toggles first (used by multiple domains)
        let service_builder = ServiceBuilder::new();

        // RUS-136: Create single ApiClientAdapter instance to share across all domains
        let api_adapter =
            Arc::new(ApiClientAdapter::new(Arc::new(api_client.clone())));
        let api_service: Arc<dyn ApiService> = api_adapter.clone();

        // RUS-136: Create trait-based AuthService via adapter
        // Create AuthManager inline for the adapter (not stored in State)
        let auth_manager = AuthManager::new(api_client.clone());
        let mgr_arc = std::sync::Arc::new(auth_manager);
        let adapter = AuthManagerAdapter::new(mgr_arc);
        let auth_service = std::sync::Arc::new(adapter);

        // Create domain states with required services
        let auth_state =
            AuthDomainState::new(api_service.clone(), auth_service.clone());

        let library_state =
            LibraryDomainState::new(Some(api_service.clone()), lib_accessor);

        let media_state =
            MediaDomainState::new(media_accessor, Some(api_service.clone()));

        let metadata_state = MetadataDomainState::new(
            server_url.clone(),
            Some(api_service.clone()),
            image_service.clone(),
        );

        let ui_state = UIDomainState {
            view: crate::domains::ui::types::ViewState::Library,
            // Resolve default widget animation from constants
            default_widget_animation: {
                match DEFAULT_POSTER_ANIMATION {
                    PosterAnimationKind::None => AnimationType::None,
                    PosterAnimationKind::Fade => AnimationType::Fade {
                        duration: std::time::Duration::from_millis(DEFAULT_DURATION_MS),
                    },
                    PosterAnimationKind::Flip => AnimationType::flip(),
                }
            },
            repo_accessor: ui_accessor.clone(),
            // New zero-copy fields
            movie_yoke_cache: crate::domains::ui::yoke_cache::YokeCache::new(2048),
            series_yoke_cache: crate::domains::ui::yoke_cache::YokeCache::new(256),
            season_yoke_cache: crate::domains::ui::yoke_cache::YokeCache::new(512),
            episode_yoke_cache: crate::domains::ui::yoke_cache::YokeCache::new(2048),

            movies_carousel: CarouselState::new(0),
            tv_carousel: CarouselState::new(0),

            display_mode: DisplayMode::Curated,
            sort_by: SortBy::Title,
            sort_order: SortOrder::Ascending,
            loading: false,
            error_message: None,
            window_size: iced::Size::new(1280.0, 720.0),
            expanded_shows: std::collections::HashSet::new(),
            hovered_media_id: None,
            theme_color_cache: parking_lot::RwLock::new(std::collections::HashMap::new()),
            current_library_id: None,
            last_scroll_position: 0.0,
            scroll_stopped_time: None,
            last_scroll_time: None,
            last_check_task_created: None,
            scroll_manager: crate::domains::ui::scroll_manager::ScrollPositionManager::default(),
            background_shader_state:
                crate::domains::ui::background_state::BackgroundShaderState::default(),
            search_query: String::new(),
            show_library_menu: false,
            library_menu_target: None,
            is_fullscreen: false,
            show_filter_panel: false,
            selected_genres: Vec::new(),
            selected_decade: None,
            selected_resolution: UiResolution::Any,
            selected_watch_status: UiWatchStatus::Any,
            show_seasons_carousel: None,
            season_episodes_carousel: None,
            show_clear_database_confirm: false,
            navigation_history: Vec::new(),
            poster_anim_active_until: None,
        };

        // Create settings service adapter
        let api_arc = Arc::new(api_client.clone());
        let settings_adapter = SettingsApiAdapter::new(api_arc);
        let settings_service = Arc::new(settings_adapter);

        let settings_state = SettingsDomainState::new(
            auth_service.clone(),
            api_service.clone(),
            settings_service,
        );

        // Create streaming service adapter
        let api_arc_stream = Arc::new(api_client.clone());
        let streaming_adapter = StreamingApiAdapter::new(api_arc_stream);
        let streaming_service = Arc::new(streaming_adapter);

        let streaming_state = StreamingDomainState::new(
            api_service.clone(),
            streaming_service,
            ui_accessor.clone(),
        );

        let mut user_mgmt_state = UserManagementDomainState {
            api_service: Some(api_service.clone()),
            user_admin_service: None,
            ..Default::default()
        };

        if service_builder.toggles().prefer_trait_services {
            let api_arc = std::sync::Arc::new(api_client.clone());
            let adapter = UserAdminApiAdapter::new(api_arc);
            user_mgmt_state.user_admin_service =
                Some(std::sync::Arc::new(adapter));
        }

        let player_domain = PlayerDomain::new(Some(api_service.clone()));

        let search_domain =
            SearchDomain::new_with_metrics(Some(api_service.clone()));

        // Create domain registry
        let domains = DomainRegistry {
            auth: crate::domains::auth::AuthDomain::new(auth_state),
            library: crate::domains::library::LibraryDomain::new(library_state),
            media: crate::domains::media::MediaDomain::new(media_state),
            metadata: crate::domains::metadata::MetadataDomain::new(
                metadata_state,
            ),
            player: player_domain,
            ui: crate::domains::ui::UIDomain::new(ui_state),
            settings: crate::domains::settings::SettingsDomain::new(
                settings_state,
            ),
            streaming: crate::domains::streaming::StreamingDomain::new(
                streaming_state,
            ),
            user_management:
                crate::domains::user_management::UserManagementDomain::new(
                    user_mgmt_state,
                ),
            search: search_domain,
        };

        // Create tab manager (NEW ARCHITECTURE)
        let mut tab_manager = TabManager::new(ui_accessor.clone());
        // Initialize and activate the All tab at startup
        tab_manager.get_or_create_tab(crate::domains::ui::tabs::TabId::All);
        tab_manager.set_active_tab(crate::domains::ui::tabs::TabId::All);
        tab_manager.refresh_active_tab();
        log::info!(
            "[Startup] Initialized and activated All tab for curated view"
        );

        // NOTE: Tabs and views use the repo accessor pattern for data access
        log::info!(
            "[Architecture] TabManager created for independent tab state management"
        );

        Self {
            domains,
            tab_manager,
            server_url: server_url.clone(),
            api_service,
            image_service: image_service.clone(), // TODO: Fix this clone
            image_receiver: Arc::new(std::sync::Mutex::new(Some(_receiver))),
            loading: true,
            is_authenticated: false,
            window_size: iced::Size::new(1280.0, 720.0),
            window_position: None,
            is_fullscreen: false,
            media_repo,
        }
    }

    /// Helper method to access UI state (commonly accessed)
    pub fn view_state(&self) -> &crate::domains::ui::types::ViewState {
        &self.domains.ui.state.view
    }

    /// Helper method to check authentication
    pub fn is_authenticated(&self) -> bool {
        self.domains.auth.state.is_authenticated
    }

    /// Helper method to get current library ID
    pub fn current_library_id(&self) -> Option<LibraryID> {
        self.domains.library.state.current_library_id
    }

    /// Update TabManager with library information
    pub fn update_tab_manager_libraries(&mut self) {
        // Update TabManager with current libraries via the repo accessor
        self.tab_manager.update_libraries();
    }

    /// Get the active tab ID
    pub fn active_tab_id(&self) -> TabId {
        self.tab_manager.active_tab_id()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new("http://localhost:8000".to_string())
    }
}
