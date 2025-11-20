use std::sync::Arc;

use ferrex_player::infrastructure::api_client::ApiClient;
use ferrex_player::infrastructure::services::settings::SettingsApiAdapter;
use ferrex_player::infrastructure::services::user_management::UserAdminApiAdapter;

#[test]
fn settings_and_user_mgmt_adapters_construct() {
    let client = Arc::new(ApiClient::new("http://localhost:8000".to_string()));
    let _settings = SettingsApiAdapter::new(client.clone());
    let _users = UserAdminApiAdapter::new(client.clone());
}
