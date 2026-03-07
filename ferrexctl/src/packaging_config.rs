use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use crate::cli::utils::workspace_root;

#[derive(Debug, Error)]
pub enum PackagingConfigError {
    #[error("failed to parse packaging.toml")]
    TomlParse {
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to read packaging.toml at {path}")]
    FileIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("workspace version not found in Cargo.toml")]
    WorkspaceVersionNotFound,
    #[error("failed to read workspace Cargo.toml at {path}")]
    WorkspaceCargoIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FlatpakConfig {
    pub manifest_path: PathBuf,
    pub app_id: String,
    pub output_dir: PathBuf,
}

impl Default for FlatpakConfig {
    fn default() -> Self {
        Self {
            manifest_path: PathBuf::from(
                "flatpak/io.github.lowband21.FerrexPlayer.yml",
            ),
            app_id: "io.github.lowband21.FerrexPlayer".to_string(),
            output_dir: PathBuf::from("dist-release"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleaseConfig {
    pub output_dir: PathBuf,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("dist-release"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PreflightConfig {
    pub run_fmt: bool,
    pub run_clippy: bool,
    pub run_tests: bool,
    pub run_deny: bool,
    pub run_audit: bool,
    pub offline: bool,
    pub scope: PreflightScope,
}

impl Default for PreflightConfig {
    fn default() -> Self {
        Self {
            run_fmt: true,
            run_clippy: true,
            run_tests: true,
            run_deny: true,
            run_audit: false,
            offline: false,
            scope: PreflightScope::Workspace,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PreflightScope {
    Workspace,
    Init,
}

impl Default for PreflightScope {
    fn default() -> Self {
        Self::Workspace
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VersionSource {
    Workspace,
    Manual,
}

impl Default for VersionSource {
    fn default() -> Self {
        Self::Workspace
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionConfig {
    pub source: VersionSource,
}

impl Default for VersionConfig {
    fn default() -> Self {
        Self {
            source: VersionSource::Workspace,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PackagingConfig {
    pub flatpak: FlatpakConfig,
    pub release: ReleaseConfig,
    pub preflight: PreflightConfig,
    pub version: VersionConfig,
}

impl Default for PackagingConfig {
    fn default() -> Self {
        Self {
            flatpak: FlatpakConfig::default(),
            release: ReleaseConfig::default(),
            preflight: PreflightConfig::default(),
            version: VersionConfig::default(),
        }
    }
}

impl PackagingConfig {
    /// Load packaging configuration from packaging.toml at workspace root.
    /// Returns defaults if file doesn't exist.
    /// Returns error only if file exists but is invalid TOML.
    pub fn load() -> Result<Self, PackagingConfigError> {
        let workspace = workspace_root();
        let config_path = workspace.join("packaging.toml");

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content =
            std::fs::read_to_string(&config_path).map_err(|source| {
                PackagingConfigError::FileIo {
                    path: config_path.clone(),
                    source,
                }
            })?;

        toml::from_str(&content)
            .map_err(|source| PackagingConfigError::TomlParse { source })
    }

    /// Resolve version from workspace Cargo.toml.
    /// Returns the version string from [workspace.package] section.
    pub fn resolve_version(&self) -> Result<String, PackagingConfigError> {
        let workspace = workspace_root();
        let cargo_toml_path = workspace.join("Cargo.toml");

        let content =
            std::fs::read_to_string(&cargo_toml_path).map_err(|source| {
                PackagingConfigError::WorkspaceCargoIo {
                    path: cargo_toml_path.clone(),
                    source,
                }
            })?;

        parse_workspace_version(&content)
            .ok_or(PackagingConfigError::WorkspaceVersionNotFound)
    }
}

pub(crate) fn parse_workspace_version(cargo_toml: &str) -> Option<String> {
    let mut in_workspace_package = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();

        if trimmed == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        }

        if in_workspace_package {
            if trimmed.starts_with('[') {
                in_workspace_package = false;
                continue;
            }

            if trimmed.starts_with("version") {
                if let Some(eq_pos) = trimmed.find('=') {
                    let value = trimmed[eq_pos + 1..].trim();
                    let version = value.trim_matches('"').trim();
                    return Some(version.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workspace_version() {
        let cargo_toml = r#"
[workspace]
members = ["ferrexctl"]

[workspace.package]
version = "0.1.0-alpha"
edition = "2024"

[dependencies]
"#;

        let version = parse_workspace_version(cargo_toml);
        assert_eq!(version, Some("0.1.0-alpha".to_string()));
    }

    #[test]
    fn test_parse_workspace_version_not_found() {
        let cargo_toml = r#"
[package]
name = "test"
version = "1.0.0"
"#;

        let version = parse_workspace_version(cargo_toml);
        assert_eq!(version, None);
    }

    #[test]
    fn test_default_config() {
        let config = PackagingConfig::default();
        assert_eq!(
            config.flatpak.manifest_path,
            PathBuf::from("flatpak/io.github.lowband21.FerrexPlayer.yml")
        );
        assert_eq!(config.flatpak.app_id, "io.github.lowband21.FerrexPlayer");
        assert_eq!(config.flatpak.output_dir, PathBuf::from("dist-release"));
        assert_eq!(config.release.output_dir, PathBuf::from("dist-release"));
        assert!(config.preflight.run_fmt);
        assert!(config.preflight.run_clippy);
        assert!(config.preflight.run_tests);
        assert!(!config.preflight.run_audit);
        assert!(!config.preflight.offline);
        assert_eq!(config.version.source, VersionSource::Workspace);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let config = PackagingConfig::load();
        assert!(config.is_ok());
        let config = config.unwrap();
        assert_eq!(config.flatpak.app_id, "io.github.lowband21.FerrexPlayer");
    }
}
