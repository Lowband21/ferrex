//! Refactored State using domain-driven architecture
//!
//! This new State structure delegates domain-specific state to the DomainRegistry,
//! keeping only the view models and cross-cutting concerns at the top level.

use crate::domains::media::store::MediaStoreSubscriber;
use crate::domains::ui::tabs::{TabId, TabManager};
use crate::domains::ui::view_models::AllViewModel;
use crate::domains::DomainRegistry;
use crate::infrastructure::api_types::Library;
use std::sync::{Arc, RwLock as StdRwLock, Weak};

/// Application state - refactored to use domain-driven architecture
#[derive(Debug)]
pub struct State {
    /// Domain registry containing all domain-specific state
    pub domains: DomainRegistry,

    /// Tab manager for independent tab states (NEW ARCHITECTURE)
    pub tab_manager: TabManager,

    /// View model for the All tab's carousel view
    pub all_view_model: AllViewModel,

    /// Server URL - needed by multiple domains
    pub server_url: String,

    /// Shared services and infrastructure
    pub api_client: Option<crate::infrastructure::ApiClient>,
    pub image_service: crate::domains::metadata::image_service::UnifiedImageService,
    pub image_receiver: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<()>>>>,

    /// Shared metadata services
    pub batch_metadata_fetcher:
        Option<Arc<crate::domains::metadata::batch_fetcher::BatchMetadataFetcher>>,

    /// Top-level application state
    pub loading: bool,
    pub is_authenticated: bool,
    pub window_size: iced::Size,
    pub is_fullscreen: bool,

    /// Notifier for MediaStore changes - triggers ViewModel refreshes
    pub media_store_notifier: Arc<crate::domains::media::store::MediaStoreNotifier>,
}

impl State {
    /// Create a new State with the given server URL
    pub fn new(server_url: String) -> Self {
        // Create shared resources
        let media_store = Arc::new(StdRwLock::new(
            crate::domains::media::store::MediaStore::new(),
        ));
        let api_client = crate::infrastructure::api_client::ApiClient::new(server_url.clone());
        let (image_service, _receiver) =
            crate::domains::metadata::image_service::UnifiedImageService::new(8);

        // Create service builder/toggles first (used by multiple domains)
        let service_builder = crate::infrastructure::services::ServiceBuilder::new();

        // RUS-136: Create single ApiClientAdapter instance to share across all domains
        let api_adapter = std::sync::Arc::new(
            crate::infrastructure::adapters::api_client_adapter::ApiClientAdapter::new(
                std::sync::Arc::new(api_client.clone()),
            ),
        );

        // RUS-136: Create trait-based AuthService via adapter
        // Create AuthManager inline for the adapter (not stored in State)
        let auth_manager = crate::domains::auth::manager::AuthManager::new(api_client.clone());
        let mgr_arc = std::sync::Arc::new(auth_manager);
        let adapter = crate::infrastructure::adapters::AuthManagerAdapter::new(mgr_arc);
        let auth_service = std::sync::Arc::new(adapter);

        // Create domain states with required services
        let auth_state =
            crate::domains::auth::AuthDomainState::new(api_adapter.clone(), auth_service.clone());

        let library_state = crate::domains::library::LibraryDomainState::new(
            media_store.clone(),
            Some(api_adapter.clone()),
        );

        let media_state = crate::domains::media::MediaDomainState::new(
            media_store.clone(),
            Some(api_adapter.clone()),
        );

        let metadata_state = crate::domains::metadata::MetadataDomainState::new(
            server_url.clone(),
            media_store.clone(),
            Some(api_adapter.clone()),
            image_service.clone(),
        );

        let ui_state = crate::domains::ui::UIDomainState::default();

        // Create settings service adapter
        let api_arc = std::sync::Arc::new(api_client.clone());
        let settings_adapter =
            crate::infrastructure::services::settings::SettingsApiAdapter::new(api_arc);
        let settings_service = std::sync::Arc::new(settings_adapter);

        let settings_state = crate::domains::settings::SettingsDomainState::new(
            auth_service.clone(),
            api_adapter.clone(),
            settings_service,
        );

        // Create streaming service adapter
        let api_arc = std::sync::Arc::new(api_client.clone());
        let streaming_adapter =
            crate::infrastructure::services::streaming::StreamingApiAdapter::new(api_arc);
        let streaming_service = std::sync::Arc::new(streaming_adapter);

        let streaming_state = crate::domains::streaming::StreamingDomainState::new(
            media_store.clone(),
            api_adapter.clone(),
            streaming_service,
        );

        let mut user_mgmt_state = crate::domains::user_management::UserManagementDomainState {
            api_service: Some(api_adapter.clone()),
            user_admin_service: None,
            ..Default::default()
        };

        if service_builder.toggles().prefer_trait_services {
            let api_arc = std::sync::Arc::new(api_client.clone());
            let adapter =
                crate::infrastructure::services::user_management::UserAdminApiAdapter::new(api_arc);
            user_mgmt_state.user_admin_service = Some(std::sync::Arc::new(adapter));
        }

        let player_domain = crate::domains::player::PlayerDomain::new(
            media_store.clone(),
            Some(api_adapter.clone()),
        );

        let search_domain = crate::domains::search::SearchDomain::new_with_metrics(
            media_store.clone(),
            Some(api_adapter.clone()),
        );

        // Create domain registry
        let domains = DomainRegistry {
            auth: crate::domains::auth::AuthDomain::new(auth_state),
            library: crate::domains::library::LibraryDomain::new(library_state),
            media: crate::domains::media::MediaDomain::new(media_state),
            metadata: crate::domains::metadata::MetadataDomain::new(metadata_state),
            player: player_domain,
            ui: crate::domains::ui::UIDomain::new(ui_state),
            settings: crate::domains::settings::SettingsDomain::new(settings_state),
            streaming: crate::domains::streaming::StreamingDomain::new(streaming_state),
            user_management: crate::domains::user_management::UserManagementDomain::new(
                user_mgmt_state,
            ),
            search: search_domain,
        };

        // Create MediaStore notifier with longer debounce to reduce refresh frequency
        // 250ms debounce reduces UI refresh load during initial library loading
        let media_store_notifier =
            Arc::new(crate::domains::media::store::MediaStoreNotifier::with_debounce(250));
        {
            if let Ok(mut store) = media_store.write() {
                store.subscribe(
                    Arc::downgrade(&media_store_notifier) as Weak<dyn MediaStoreSubscriber>
                );
                log::info!("[MediaStore] Notifier subscribed for ViewModel refresh tracking");
            } else {
                log::error!(
                    "[MediaStore] Failed to subscribe notifier - ViewModels won't auto-refresh"
                );
            }
        }

        // Create tab manager (NEW ARCHITECTURE)
        let mut tab_manager = TabManager::new(media_store.clone());

        // Initialize and activate the All tab at startup
        tab_manager.get_or_create_tab(crate::domains::ui::tabs::TabId::All);
        tab_manager.set_active_tab(crate::domains::ui::tabs::TabId::All);
        tab_manager.refresh_active_tab();
        log::info!("[Startup] Initialized and activated All tab for curated view");

        // Create view model for the All tab
        let all_view_model = AllViewModel::new(media_store.clone());

        // NOTE: ViewModels themselves are not directly subscribed to avoid Arc<ViewModel> complexity
        // Instead, MediaStoreNotifier tracks changes and triggers RefreshViewModels when needed
        log::info!("[Architecture] Using MediaStoreNotifier pattern for ViewModel updates");
        log::info!("[Architecture] TabManager created for independent tab state management");

        Self {
            domains,
            tab_manager,
            all_view_model,
            server_url: server_url.clone(),
            api_client: Some(api_client),
            image_service: image_service.clone(), // TODO: Fix this clone
            image_receiver: Arc::new(std::sync::Mutex::new(Some(_receiver))),
            batch_metadata_fetcher: None,
            loading: true,
            is_authenticated: false,
            window_size: iced::Size::new(1280.0, 720.0),
            is_fullscreen: false,
            media_store_notifier,
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
    pub fn current_library_id(&self) -> Option<uuid::Uuid> {
        self.domains.library.state.current_library_id
    }

    /// Update TabManager with library information
    pub fn update_tab_manager_libraries(&mut self) {
        // Update TabManager with current libraries
        self.tab_manager
            .update_libraries(&self.domains.library.state.libraries);

        // Also register each library's type for tab creation
        for library in &self.domains.library.state.libraries {
            if library.enabled {
                self.tab_manager
                    .register_library(library.id, library.library_type);
            }
        }
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
