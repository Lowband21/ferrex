//! State trait abstractions for domain isolation
//!
//! These traits allow domains to be tested in isolation by providing
//! abstract interfaces to the state they need, rather than requiring
//! the entire monolithic State struct.

use ferrex_core::{
    api::types::Library, domain::watch::UserWatchState, rbac::UserPermissions,
    user::User,
};
use ferrex_player::{
    api_client::ApiClient,
    auth_manager::AuthManager,
    media_store::MediaStore,
    metadata_service::MetadataService,
    state::{AuthenticationFlow, SortBy, SortOrder, ViewMode, ViewState},
};
use std::collections::HashMap;
use uuid::Uuid;

/// Authentication domain state requirements
pub trait AuthState {
    fn auth_manager(&self) -> &AuthManager;
    fn auth_manager_mut(&mut self) -> &mut AuthManager;
    fn api_client(&self) -> &ApiClient;
    fn api_client_mut(&mut self) -> &mut ApiClient;
    fn is_authenticated(&self) -> bool;
    fn set_authenticated(&mut self, authenticated: bool);
    fn auth_flow(&self) -> &AuthenticationFlow;
    fn set_auth_flow(&mut self, flow: AuthenticationFlow);
    fn user_permissions(&self) -> Option<&UserPermissions>;
    fn set_user_permissions(&mut self, permissions: Option<UserPermissions>);
    fn set_error(&mut self, error: Option<String>);
}

/// Library domain state requirements
pub trait LibraryState {
    fn libraries(&self) -> &[Library];
    fn set_libraries(&mut self, libraries: Vec<Library>);
    fn current_library_id(&self) -> Option<Uuid>;
    fn set_current_library_id(&mut self, id: Option<Uuid>);
    fn media_store(&self) -> &MediaStore;
    fn media_store_mut(&mut self) -> &mut MediaStore;
    fn api_client(&self) -> &ApiClient;
    fn set_loading(&mut self, loading: bool);
    fn set_error(&mut self, error: Option<String>);
}

/// Media domain state requirements
pub trait MediaState {
    fn view_state(&self) -> &ViewState;
    fn set_view_state(&mut self, view: ViewState);
    fn media_store(&self) -> &MediaStore;
    fn media_store_mut(&mut self) -> &mut MediaStore;
    fn user_watch_state(&self) -> Option<&UserWatchState>;
    fn set_user_watch_state(&mut self, state: Option<UserWatchState>);
    fn api_client(&self) -> &ApiClient;
    fn current_library_id(&self) -> Option<Uuid>;
}

/// UI domain state requirements
pub trait UIState {
    fn view_mode(&self) -> ViewMode;
    fn set_view_mode(&mut self, mode: ViewMode);
    fn sort_by(&self) -> SortBy;
    fn set_sort_by(&mut self, sort: SortBy);
    fn sort_order(&self) -> SortOrder;
    fn set_sort_order(&mut self, order: SortOrder);
    fn window_size(&self) -> iced::Size;
    fn set_window_size(&mut self, size: iced::Size);
    fn search_query(&self) -> &str;
    fn set_search_query(&mut self, query: String);
    fn is_fullscreen(&self) -> bool;
    fn set_fullscreen(&mut self, fullscreen: bool);
}

/// Metadata domain state requirements
pub trait MetadataState {
    fn metadata_service(&self) -> &MetadataService;
    fn metadata_service_mut(&mut self) -> &mut MetadataService;
    fn media_store(&self) -> &MediaStore;
    fn api_client(&self) -> &ApiClient;
    fn loading_posters(&self) -> &HashMap<String, bool>;
    fn loading_posters_mut(&mut self) -> &mut HashMap<String, bool>;
}

/// Settings domain state requirements
pub trait SettingsState {
    fn user_permissions(&self) -> Option<&UserPermissions>;
    fn auth_manager(&self) -> &AuthManager;
    fn api_client(&self) -> &ApiClient;
    fn auto_login_enabled(&self) -> bool;
    fn set_auto_login_enabled(&mut self, enabled: bool);
}

/// Streaming domain state requirements
pub trait StreamingState {
    fn api_client(&self) -> &ApiClient;
    fn current_library_id(&self) -> Option<Uuid>;
    fn media_store(&self) -> &MediaStore;
}

/// User management domain state requirements
pub trait UserManagementState {
    fn api_client(&self) -> &ApiClient;
    fn user_permissions(&self) -> Option<&UserPermissions>;
    fn auth_manager(&self) -> &AuthManager;
}

/// A trait that combines all domain states for integration testing
pub trait IntegrationState:
    AuthState
    + LibraryState
    + MediaState
    + UIState
    + MetadataState
    + SettingsState
    + StreamingState
    + UserManagementState
{
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example of a minimal mock state for testing auth domain
    struct MockAuthState {
        auth_manager: AuthManager,
        api_client: ApiClient,
        is_authenticated: bool,
        auth_flow: AuthenticationFlow,
        user_permissions: Option<UserPermissions>,
        error: Option<String>,
    }

    impl AuthState for MockAuthState {
        fn auth_manager(&self) -> &AuthManager {
            &self.auth_manager
        }

        fn auth_manager_mut(&mut self) -> &mut AuthManager {
            &mut self.auth_manager
        }

        fn api_client(&self) -> &ApiClient {
            &self.api_client
        }

        fn api_client_mut(&mut self) -> &mut ApiClient {
            &mut self.api_client
        }

        fn is_authenticated(&self) -> bool {
            self.is_authenticated
        }

        fn set_authenticated(&mut self, authenticated: bool) {
            self.is_authenticated = authenticated;
        }

        fn auth_flow(&self) -> &AuthenticationFlow {
            &self.auth_flow
        }

        fn set_auth_flow(&mut self, flow: AuthenticationFlow) {
            self.auth_flow = flow;
        }

        fn user_permissions(&self) -> Option<&UserPermissions> {
            self.user_permissions.as_ref()
        }

        fn set_user_permissions(
            &mut self,
            permissions: Option<UserPermissions>,
        ) {
            self.user_permissions = permissions;
        }

        fn set_error(&mut self, error: Option<String>) {
            self.error = error;
        }
    }
}
