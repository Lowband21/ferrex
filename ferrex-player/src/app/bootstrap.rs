use std::sync::Arc;

use iced::Task;

use crate::common::messages::DomainMessage;
use crate::domains::auth::{messages as auth_messages, types::AuthenticationFlow};
use crate::state_refactored::State;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub server_url: Arc<str>,
    pub use_test_stubs: bool,
    #[cfg(feature = "demo")]
    pub demo_mode: bool,
    #[cfg(feature = "demo")]
    pub demo_credentials: (Arc<str>, Arc<str>),
}

impl AppConfig {
    pub fn new(server_url: impl Into<String>) -> Self {
        Self {
            server_url: Arc::from(server_url.into()),
            use_test_stubs: false,
            #[cfg(feature = "demo")]
            demo_mode: false,
            #[cfg(feature = "demo")]
            demo_credentials: (Arc::from("demo"), Arc::from("demo")),
        }
    }

    pub fn from_environment() -> Self {
        let server_url = std::env::var("FERREX_SERVER_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());

        #[cfg(feature = "demo")]
        {
            let env_value = std::env::var("FERREX_PLAYER_DEMO_MODE")
                .or_else(|_| std::env::var("FERREX_DEMO_MODE"))
                .unwrap_or_default();
            let demo_mode = matches!(
                env_value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            ) || std::env::args().any(|arg| arg == "--demo");

            let demo_credentials = (
                Arc::from(std::env::var("FERREX_DEMO_USERNAME").unwrap_or_else(|_| "demo".into())),
                Arc::from(std::env::var("FERREX_DEMO_PASSWORD").unwrap_or_else(|_| "demo".into())),
            );

            Self {
                server_url: Arc::from(server_url),
                use_test_stubs: false,
                demo_mode,
                demo_credentials,
            }
        }

        #[cfg(not(feature = "demo"))]
        {
            Self {
                server_url: Arc::from(server_url),
                use_test_stubs: false,
            }
        }
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn use_test_stubs(&self) -> bool {
        self.use_test_stubs
    }

    pub fn with_test_stubs(mut self, enabled: bool) -> Self {
        self.use_test_stubs = enabled;
        self
    }
}

/// Boot logic used both by the runtime application and presets.
pub fn base_state(config: &AppConfig) -> State {
    let mut state = State::new(config.server_url().to_string());

    crate::infrastructure::service_registry::init_registry(state.image_service.clone());

    let lib_id = state
        .domains
        .library
        .state
        .current_library_id
        .map(|library_id| library_id.as_uuid());

    state
        .domains
        .ui
        .state
        .background_shader_state
        .update_depth_lines(
            &state.domains.ui.state.view,
            state.window_size.width,
            state.window_size.height,
            lib_id,
        );

    if config.use_test_stubs() {
        apply_test_stubs(&mut state);
    }

    state
}

#[cfg(any(test, feature = "iced_tester"))]
fn apply_test_stubs(state: &mut State) {
    use crate::domains::search::SearchDomain;
    use crate::infrastructure::services::auth::AuthService;
    use crate::infrastructure::services::settings::SettingsService;
    use crate::infrastructure::testing::stubs::{
        TestApiService, TestAuthService, TestSettingsService,
    };

    let api_stub: Arc<TestApiService> = Arc::new(TestApiService::new(state.server_url.clone()));
    let auth_stub: Arc<TestAuthService> = Arc::new(TestAuthService::new());
    let settings_stub: Arc<TestSettingsService> =
        Arc::new(TestSettingsService::with_default_device());

    let auth_service: Arc<dyn AuthService> = auth_stub.clone();
    let settings_service: Arc<dyn SettingsService> = settings_stub.clone();

    state.api_service = api_stub.clone();

    state.domains.auth.state.api_service = api_stub.clone();
    state.domains.auth.state.auth_service = auth_service.clone();

    state.domains.settings.auth_service = auth_service;
    state.domains.settings.settings_service = settings_service;

    let stub_devices = settings_stub.devices();
    state.domains.settings.device_management_state.devices = stub_devices
        .iter()
        .map(
            |device| crate::domains::ui::views::settings::device_management::UserDevice {
                device_id: device.id.to_string(),
                device_name: device.name.clone(),
                device_type: format!("{:?}", device.platform),
                last_active: device.last_activity,
                is_current_device: false,
                location: device
                    .metadata
                    .get("location")
                    .and_then(|value| value.as_str())
                    .map(|s| s.to_string()),
            },
        )
        .collect();

    api_stub.set_devices(stub_devices);

    state.domains.library.state.api_service = Some(api_stub.clone());
    state.domains.media.state.api_service = Some(api_stub.clone());
    state.domains.metadata.state.api_service = Some(api_stub.clone());
    state.domains.streaming.state.api_service = api_stub.clone();
    state.domains.user_management.state.api_service = Some(api_stub.clone());
    state.domains.player.api_service = Some(api_stub.clone());

    state.domains.search = SearchDomain::new_with_metrics(Some(api_stub.clone()));
}

#[cfg(not(any(test, feature = "iced_tester")))]
fn apply_test_stubs(_state: &mut State) {}

/// Boot logic for the running application, returning the initial state and task batch.
pub fn runtime_boot(config: &AppConfig) -> (State, Task<DomainMessage>) {
    let state = base_state(config);

    let auth_service = state.domains.auth.state.auth_service.clone();

    let auth_task = Task::perform(
        async move {
            log::info!("[Auth] Checking for stored authentication...");

            match auth_service.load_from_keychain().await {
                Ok(Some(stored_auth)) => {
                    log::info!(
                        "[Auth] Found stored auth for user: {}",
                        stored_auth.user.username
                    );

                    let auto_login_enabled = auth_service
                        .is_auto_login_enabled(&stored_auth.user.id)
                        .await
                        .unwrap_or(false)
                        && stored_auth.user.preferences.auto_login_enabled;

                    log::info!("[Auth] Auto-login enabled: {}", auto_login_enabled);

                    if auto_login_enabled {
                        match auth_service.apply_stored_auth(stored_auth).await {
                            Ok(()) => {
                                log::info!("[Auth] Auto-login successful");
                                Ok::<Option<bool>, String>(Some(true))
                            }
                            Err(e) => {
                                log::error!("[Auth] Failed to apply stored auth: {}", e);
                                Ok::<Option<bool>, String>(Some(false))
                            }
                        }
                    } else {
                        log::info!("[Auth] Auto-login disabled");
                        Ok::<Option<bool>, String>(Some(false))
                    }
                }
                Ok(None) => {
                    log::info!("[Auth] No stored auth found");
                    Ok::<Option<bool>, String>(None)
                }
                Err(e) => {
                    log::error!("[Auth] Error loading stored auth: {}", e);
                    Ok::<Option<bool>, String>(None)
                }
            }
        },
        |result| match result {
            Ok(Some(true)) => {
                log::info!("[Auth] Auto-login enabled, sending CheckAuthStatus");
                DomainMessage::Auth(auth_messages::Message::CheckAuthStatus)
            }
            Ok(Some(false)) | Ok(None) => {
                log::info!("[Auth] Auto-login disabled or no stored auth, sending LoadUsers");
                DomainMessage::Auth(auth_messages::Message::LoadUsers)
            }
            Err(e) => {
                log::error!("[Auth] Error during auth check: {}", e);
                DomainMessage::Auth(auth_messages::Message::LoadUsers)
            }
        },
    );

    #[cfg(feature = "demo")]
    let tasks = {
        let mut tasks = vec![auth_task];

        if config.demo_mode {
            let auth_service = state.domains.auth.state.auth_service.clone();
            let (username, password) = config.demo_credentials.clone();
            log::info!("[Demo] Attempting automatic demo login as {}", username);
            tasks.push(Task::perform(
                async move {
                    auth_service
                        .authenticate_device(username.to_string(), password.to_string(), true)
                        .await
                        .map_err(|err| err.to_string())
                },
                |result| DomainMessage::Auth(auth_messages::Message::AuthResult(result)),
            ));
        }

        tasks
    };

    #[cfg(not(feature = "demo"))]
    let tasks = vec![auth_task];

    (state, Task::batch(tasks))
}

/// Utility helper for presets to reset authentication state.
pub fn reset_to_first_run(state: &mut State) {
    state.is_authenticated = false;
    state.domains.auth.state.is_authenticated = false;
    state.domains.auth.state.user_permissions = None;
    state.domains.auth.state.auth_flow = AuthenticationFlow::CheckingSetup;
}
