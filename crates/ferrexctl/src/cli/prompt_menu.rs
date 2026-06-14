use crate::cli::{
    Term,
    state::{self, MenuItem},
    validation,
};

use dialoguer::{Confirm, Input, Select};

use anyhow::{Result, anyhow};

use std::path::PathBuf;

pub fn run_prompt_menu(
    state: &mut state::PromptState,
    advanced: bool,
) -> Result<()> {
    let mut items = vec![
        MenuItem::Finish,
        MenuItem::DevMode,
        MenuItem::ServerHost,
        MenuItem::ServerPort,
        MenuItem::ServerUrl,
        MenuItem::MediaRoot,
        MenuItem::TmdbApiKey,
    ];
    if advanced {
        items.extend([
            MenuItem::CorsOrigins,
            MenuItem::CorsAllowCredentials,
            MenuItem::EnforceHttps,
            MenuItem::TrustProxy,
            MenuItem::HstsMaxAge,
            MenuItem::HstsIncludeSub,
            MenuItem::HstsPreload,
            MenuItem::TlsMinVersion,
            MenuItem::TlsCipherSuites,
            MenuItem::RateLimitsPath,
            MenuItem::RateLimitsJson,
            MenuItem::ScannerPath,
            MenuItem::ScannerJson,
            MenuItem::FfmpegPath,
            MenuItem::FfprobePath,
            MenuItem::DemoMode,
            MenuItem::DemoOptions,
            MenuItem::DemoUsername,
            MenuItem::DemoPassword,
            MenuItem::DemoAllowDeviations,
            MenuItem::DemoDeviationRate,
            MenuItem::DemoMovieCount,
            MenuItem::DemoSeriesCount,
            MenuItem::DemoSkipMetadata,
            MenuItem::DemoZeroLength,
        ]);
    }

    loop {
        let labels: Vec<String> =
            items.iter().map(|item| menu_label(state, *item)).collect();
        let choice = Select::new()
            .with_prompt(
                "Configure Ferrex (use arrows/enter; Finish to continue)",
            )
            .items(&labels)
            .default(0)
            .interact_on(&Term::stderr())?;
        match items[choice] {
            MenuItem::Finish => break,
            MenuItem::DevMode => {
                state.dev_mode = Confirm::new()
                    .with_prompt("Use development mode defaults?")
                    .default(state.dev_mode)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::ServerHost => {
                state.server_host = Input::new()
                    .with_prompt(
                        "Server host (0.0.0.0 for containers/Tailscale; 127.0.0.1 for localhost)",
                    )
                    .default(state.server_host.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::ServerPort => {
                let port_str: String = Input::new()
                    .with_prompt("Server port")
                    .default(state.server_port.to_string())
                    .interact_text_on(&Term::stderr())?;
                if let Ok(p) = port_str.parse() {
                    state.server_port = p;
                }
            }
            MenuItem::ServerUrl => {
                state.ferrex_server_url = Input::new()
                    .with_prompt("Public server URL")
                    .default(state.ferrex_server_url.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::MediaRoot => {
                let current = state
                    .media_root
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let answer: String = Input::new()
                    .with_prompt("Media root (blank to skip)")
                    .allow_empty(true)
                    .default(current)
                    .interact_text_on(&Term::stderr())?;
                if answer.trim().is_empty() {
                    state.media_root = None;
                } else {
                    validation::validate_media_root(&answer).map_err(|e| {
                        anyhow!("Invalid media root path: {}", e)
                    })?;
                    state.media_root = Some(PathBuf::from(answer.trim()));
                }
            }
            MenuItem::TmdbApiKey => {
                let answer: String = Input::new()
                    .with_prompt("TMDB API key (blank disables metadata)")
                    .allow_empty(true)
                    .default(state.tmdb_api_key.clone())
                    .interact_text_on(&Term::stderr())?;
                validation::validate_tmdb_api_key(&answer)
                    .map_err(|e| anyhow!("Invalid TMDB API key: {}", e))?;
                state.tmdb_api_key = answer;
            }
            MenuItem::CorsOrigins => {
                state.cors_allowed_origins = Input::new()
                    .with_prompt("Allowed CORS origins (comma separated)")
                    .default(state.cors_allowed_origins.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::CorsAllowCredentials => {
                state.cors_allow_credentials = Confirm::new()
                    .with_prompt("Allow CORS credentials?")
                    .default(state.cors_allow_credentials)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::EnforceHttps => {
                state.enforce_https = Confirm::new()
                    .with_prompt("Enforce HTTPS?")
                    .default(state.enforce_https)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::TrustProxy => {
                state.trust_proxy_headers = Confirm::new()
                    .with_prompt("Trust proxy headers (X-Forwarded-Proto)?")
                    .default(state.trust_proxy_headers)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::HstsMaxAge => {
                let age: String = Input::new()
                    .with_prompt("HSTS max_age (seconds, 0 to disable)")
                    .default(state.hsts_max_age.to_string())
                    .interact_text_on(&Term::stderr())?;
                if let Ok(v) = age.parse() {
                    state.hsts_max_age = v;
                }
            }
            MenuItem::HstsIncludeSub => {
                state.hsts_include_subdomains = Confirm::new()
                    .with_prompt("HSTS includeSubDomains?")
                    .default(state.hsts_include_subdomains)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::HstsPreload => {
                state.hsts_preload = Confirm::new()
                    .with_prompt("HSTS preload?")
                    .default(state.hsts_preload)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::TlsMinVersion => {
                state.tls_min_version = Input::new()
                    .with_prompt("TLS minimum version (1.2 or 1.3)")
                    .default(state.tls_min_version.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::TlsCipherSuites => {
                state.tls_cipher_suites = Input::new()
                    .with_prompt("TLS cipher suites (comma separated, blank for defaults)")
                    .allow_empty(true)
                    .default(state.tls_cipher_suites.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::RateLimitsPath => {
                state.rate_limits_path = Input::new()
                    .with_prompt("Rate limiter config path (optional)")
                    .allow_empty(true)
                    .default(state.rate_limits_path.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::RateLimitsJson => {
                state.rate_limits_json = Input::new()
                    .with_prompt("Rate limiter inline JSON (optional)")
                    .allow_empty(true)
                    .default(state.rate_limits_json.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::ScannerPath => {
                state.scanner_config_path = Input::new()
                    .with_prompt("Scanner config path (optional)")
                    .allow_empty(true)
                    .default(state.scanner_config_path.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::ScannerJson => {
                state.scanner_config_json = Input::new()
                    .with_prompt("Scanner config inline JSON (optional)")
                    .allow_empty(true)
                    .default(state.scanner_config_json.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::FfmpegPath => {
                state.ffmpeg_path = Input::new()
                    .with_prompt("ffmpeg binary path")
                    .default(state.ffmpeg_path.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::FfprobePath => {
                state.ffprobe_path = Input::new()
                    .with_prompt("ffprobe binary path")
                    .default(state.ffprobe_path.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoMode => {
                state.demo_mode = Confirm::new()
                    .with_prompt("Enable demo mode?")
                    .default(state.demo_mode)
                    .interact_on(&Term::stderr())?;
            }
            MenuItem::DemoOptions => {
                state.demo_options = Input::new()
                    .with_prompt("Demo options JSON (optional)")
                    .allow_empty(true)
                    .default(state.demo_options.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoUsername => {
                state.demo_username = Input::new()
                    .with_prompt("Demo username (optional)")
                    .allow_empty(true)
                    .default(state.demo_username.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoPassword => {
                state.demo_password = Input::new()
                    .with_prompt("Demo password (optional)")
                    .allow_empty(true)
                    .default(state.demo_password.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoAllowDeviations => {
                state.demo_allow_deviations = Input::new()
                    .with_prompt("Demo allow deviations (true/false)")
                    .allow_empty(true)
                    .default(state.demo_allow_deviations.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoDeviationRate => {
                state.demo_deviation_rate = Input::new()
                    .with_prompt("Demo deviation rate (e.g., 0.1)")
                    .allow_empty(true)
                    .default(state.demo_deviation_rate.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoMovieCount => {
                state.demo_movie_count = Input::new()
                    .with_prompt("Demo movie count (optional)")
                    .allow_empty(true)
                    .default(state.demo_movie_count.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoSeriesCount => {
                state.demo_series_count = Input::new()
                    .with_prompt("Demo series count (optional)")
                    .allow_empty(true)
                    .default(state.demo_series_count.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoSkipMetadata => {
                state.demo_skip_metadata = Input::new()
                    .with_prompt("Demo skip metadata (true/false)")
                    .allow_empty(true)
                    .default(state.demo_skip_metadata.clone())
                    .interact_text_on(&Term::stderr())?;
            }
            MenuItem::DemoZeroLength => {
                state.demo_zero_length = Input::new()
                    .with_prompt("Demo zero length files (true/false)")
                    .allow_empty(true)
                    .default(state.demo_zero_length.clone())
                    .interact_text_on(&Term::stderr())?;
            }
        }
    }
    Ok(())
}

pub fn menu_label(state: &state::PromptState, item: MenuItem) -> String {
    match item {
        MenuItem::Finish => "Finish and write .env  — save & exit".into(),
        MenuItem::DevMode => {
            format!("Dev mode: {}  — dev-safe defaults", state.dev_mode)
        }
        MenuItem::ServerHost => {
            format!("Server host: {}  — bind addr", state.server_host)
        }
        MenuItem::ServerPort => {
            format!("Server port: {}  — listen port", state.server_port)
        }
        MenuItem::ServerUrl => format!(
            "Public URL: {}  — clients reach here",
            state.ferrex_server_url
        ),
        MenuItem::MediaRoot => format!(
            "Media root: {}  — path to your library",
            state
                .media_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unset)".to_string())
        ),
        MenuItem::TmdbApiKey => {
            let set = if state.tmdb_api_key.is_empty() {
                "(unset)"
            } else {
                "***"
            };
            format!("TMDB API key: {set}  — metadata lookups")
        }
        MenuItem::CorsOrigins => format!(
            "CORS origins: {}  — allowed frontends",
            state.cors_allowed_origins
        ),
        MenuItem::CorsAllowCredentials => {
            format!(
                "CORS allow credentials: {}  — cookies/headers",
                state.cors_allow_credentials
            )
        }
        MenuItem::EnforceHttps => {
            format!("Enforce HTTPS: {}  — redirect HTTP", state.enforce_https)
        }
        MenuItem::TrustProxy => format!(
            "Trust proxy headers: {}  — needed behind TLS proxy",
            state.trust_proxy_headers
        ),
        MenuItem::HstsMaxAge => {
            format!("HSTS max_age: {}  — seconds", state.hsts_max_age)
        }
        MenuItem::HstsIncludeSub => {
            format!(
                "HSTS includeSubDomains: {}  — extend to subdomains",
                state.hsts_include_subdomains
            )
        }
        MenuItem::HstsPreload => format!(
            "HSTS preload: {}  — preload list opt-in",
            state.hsts_preload
        ),
        MenuItem::TlsMinVersion => format!(
            "TLS min version: {}  — keep 1.3 for prod",
            state.tls_min_version
        ),
        MenuItem::TlsCipherSuites => format!(
            "TLS cipher suites: {}  — blank = rustls defaults",
            if state.tls_cipher_suites.is_empty() {
                "(defaults)".into()
            } else {
                state.tls_cipher_suites.clone()
            }
        ),
        MenuItem::RateLimitsPath => format!(
            "Rate limits path: {}  — file with rate rules",
            if state.rate_limits_path.is_empty() {
                "(unset)"
            } else {
                &state.rate_limits_path
            }
        )
        .to_string(),
        MenuItem::RateLimitsJson => format!(
            "Rate limits inline JSON: {}  — overrides path",
            if state.rate_limits_json.is_empty() {
                "(unset)"
            } else {
                "(set)"
            }
        ),
        MenuItem::ScannerPath => format!(
            "Scanner config path: {}  — tuning file",
            if state.scanner_config_path.is_empty() {
                "(unset)"
            } else {
                &state.scanner_config_path
            }
        )
        .to_string(),
        MenuItem::ScannerJson => format!(
            "Scanner config inline JSON: {}  — overrides path",
            if state.scanner_config_json.is_empty() {
                "(unset)"
            } else {
                "(set)"
            }
        ),
        MenuItem::FfmpegPath => {
            format!("FFmpeg path: {}  — transcoder binary", state.ffmpeg_path)
        }
        MenuItem::FfprobePath => {
            format!("ffprobe path: {}  — probe binary", state.ffprobe_path)
        }
        MenuItem::DemoMode => {
            format!("Demo mode: {}  — seed demo content", state.demo_mode)
        }
        MenuItem::DemoOptions => format!(
            "Demo options JSON: {}  — advanced demo settings",
            if state.demo_options.is_empty() {
                "(unset)"
            } else {
                "(set)"
            }
        ),
        MenuItem::DemoUsername => format!(
            "Demo username: {}  — demo login",
            if state.demo_username.is_empty() {
                "(unset)"
            } else {
                &state.demo_username
            }
        )
        .to_string(),
        MenuItem::DemoPassword => format!(
            "Demo password: {}  — demo password",
            if state.demo_password.is_empty() {
                "(unset)"
            } else {
                "(set)"
            }
        ),
        MenuItem::DemoAllowDeviations => format!(
            "Demo allow deviations: {}  — imperfect layouts",
            if state.demo_allow_deviations.is_empty() {
                "(unset)"
            } else {
                &state.demo_allow_deviations
            }
        )
        .to_string(),
        MenuItem::DemoDeviationRate => format!(
            "Demo deviation rate: {}  — fraction 0-1",
            if state.demo_deviation_rate.is_empty() {
                "(unset)"
            } else {
                &state.demo_deviation_rate
            }
        )
        .to_string(),
        MenuItem::DemoMovieCount => format!(
            "Demo movie count: {}  — seed size",
            if state.demo_movie_count.is_empty() {
                "(unset)"
            } else {
                &state.demo_movie_count
            }
        )
        .to_string(),
        MenuItem::DemoSeriesCount => format!(
            "Demo series count: {}  — seed size",
            if state.demo_series_count.is_empty() {
                "(unset)"
            } else {
                &state.demo_series_count
            }
        )
        .to_string(),
        MenuItem::DemoSkipMetadata => format!(
            "Demo skip metadata: {}  — speed demo ingest",
            if state.demo_skip_metadata.is_empty() {
                "(unset)"
            } else {
                &state.demo_skip_metadata
            }
        )
        .to_string(),
        MenuItem::DemoZeroLength => format!(
            "Demo zero-length files: {}  — generate placeholders",
            if state.demo_zero_length.is_empty() {
                "(unset)"
            } else {
                &state.demo_zero_length
            }
        )
        .to_string(),
    }
}
