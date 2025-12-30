use crate::cli::{
    RotateTarget,
    stack::{ServerMode, StackMode},
};

use std::path::PathBuf;

#[derive(Debug, Clone)]
/// User-facing options for stack up/down orchestration.
pub struct StackOptions {
    pub env_file: PathBuf,
    pub mode: StackMode,
    pub profile: String,
    pub rust_log: Option<String>,
    pub wild: Option<bool>,
    pub server_mode: ServerMode,
    /// When running with `server_mode=Host`, keep the server process in the foreground
    /// and stream compile/runtime output immediately (like `docker compose up`).
    ///
    /// When `false` (default), the host server is spawned in the background and a PID
    /// file is written so it can be stopped on `stack down`.
    pub host_attach: bool,
    pub reset_db: bool,
    pub clean: bool,
    pub init_non_interactive: bool,
    pub init_advanced: bool,
    pub init_tui: bool,
    pub force_init: bool,
    pub project_name_override: Option<String>,
    pub tailscale_serve: bool,
    /// Skip confirmation prompts for destructive operations (--yes flag).
    pub skip_confirmation: bool,
}

impl Default for StackOptions {
    fn default() -> Self {
        Self {
            env_file: Default::default(),
            mode: StackMode::Local,
            profile: "release".to_string(),
            rust_log: None,
            wild: Some(true),
            server_mode: ServerMode::Docker,
            host_attach: false,
            reset_db: false,
            clean: false,
            init_non_interactive: false,
            init_advanced: false,
            init_tui: true,
            force_init: false,
            project_name_override: None,
            tailscale_serve: false,
            skip_confirmation: false,
        }
    }
}

#[derive(Debug, Clone)]
/// Options controlling config initialization.
pub struct InitOptions {
    pub env_path: PathBuf,
    pub non_interactive: bool,
    pub advanced: bool,
    pub tailscale: bool,
    pub rotate: RotateTarget,
    pub force: bool,
    pub tui: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            env_path: PathBuf::from(".env"),
            non_interactive: false,
            advanced: false,
            tailscale: false,
            rotate: RotateTarget::None,
            force: false,
            tui: true,
        }
    }
}

impl InitOptions {
    pub fn new(env_path: PathBuf, advanced: bool) -> Self {
        Self {
            env_path,
            advanced,
            tailscale: false,
            rotate: RotateTarget::None,
            force: false,
            tui: true,
            non_interactive: false,
        }
    }
    pub fn new_non_interactive(env_path: PathBuf, advanced: bool) -> Self {
        Self {
            env_path,
            advanced,
            tailscale: false,
            rotate: RotateTarget::None,
            force: false,
            tui: true,
            non_interactive: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Options for configuration validation (`ferrex-init check`).
pub struct CheckOptions {
    pub config_path: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
    pub tls_cert_path: Option<PathBuf>,
    pub tls_key_path: Option<PathBuf>,
}
