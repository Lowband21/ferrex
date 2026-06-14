use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub volume: f64,
    pub last_view: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "https://localhost:3000".to_string(),
            volume: 1.0,
            last_view: "library".to_string(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        // First check for environment variable
        let mut config =
            if let Ok(server_url) = std::env::var("FERREX_SERVER_URL") {
                Self {
                    server_url,
                    ..Self::default()
                }
            } else {
                Self::default()
            };

        // Then load from config file (which can override env var)
        if let Some(config_dir) = dirs::config_dir() {
            let config_path =
                config_dir.join("ferrex-player").join("config.json");
            if config_path.exists()
                && let Ok(content) = std::fs::read_to_string(&config_path)
                && let Ok(loaded_config) =
                    serde_json::from_str::<Config>(&content)
            {
                config = loaded_config;
            }
        }

        // Allow env var to override config file for server URL
        if let Ok(server_url) = std::env::var("FERREX_SERVER_URL") {
            config.server_url = server_url;
        }

        config
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(config_dir) = dirs::config_dir() {
            let app_dir = config_dir.join("ferrex-player");
            std::fs::create_dir_all(&app_dir)?;
            let config_path = app_dir.join("config.json");
            let content = serde_json::to_string_pretty(self)?;
            std::fs::write(config_path, content)?;
        }
        Ok(())
    }
}
