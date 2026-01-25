use ferrex_player::state::State;

/// Setup authentication for realistic server communication using autologin
/// This attempts to use the application's autologin system for benchmarking
pub async fn setup_benchmark_authentication(
    state: &State,
) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("🔐 Attempting autologin for benchmark authentication...");

    // Get the auth service from the state
    let auth_service = &state.domains.auth.state.auth_service;

    // Step 1: Try to load stored authentication from keychain
    match auth_service.load_from_keychain().await {
        Ok(Some(stored_auth)) => {
            log::info!(
                "📁 Found stored auth for user: {}",
                stored_auth.user.username
            );

            // Step 2: Check if auto-login is enabled
            let auto_login_enabled = auth_service
                .is_auto_login_enabled(&stored_auth.user.id)
                .await
                .unwrap_or(false)
                && stored_auth.user.preferences.auto_login_enabled;

            if auto_login_enabled {
                log::info!(
                    "✅ Auto-login enabled, applying stored authentication..."
                );

                // Step 3: Apply the stored auth
                match auth_service.apply_stored_auth(stored_auth).await {
                    Ok(()) => {
                        log::info!(
                            "✅ Benchmark authentication successful via autologin"
                        );
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(format!(
                            "Failed to apply stored auth: {}",
                            e
                        )
                        .into());
                    }
                }
            } else {
                return Err("Auto-login is disabled. Please enable auto-login in the application settings and try again.".into());
            }
        }
        Ok(None) => {
            return Err("No stored authentication found. Please login to the application with auto-login enabled and try again.".into());
        }
        Err(e) => {
            return Err(
                format!("Failed to load stored authentication: {}", e).into()
            );
        }
    }
}
