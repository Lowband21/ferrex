//! Packaging and release metadata loaded from `packaging.toml`.
//!
//! These structs describe Flatpak, preflight, version, and release settings used
//! by tooling before artifacts are built or published.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use crate::cli::utils::workspace_root;

/// Errors produced while loading packaging configuration.
#[derive(Debug, Error)]
pub enum PackagingConfigError {
    /// `packaging.toml` exists but contains invalid TOML.
    #[error("failed to parse packaging.toml")]
    TomlParse {
        /// Parser error reported by the TOML decoder.
        #[source]
        source: toml::de::Error,
    },
    /// `packaging.toml` could not be read from disk.
    #[error("failed to read packaging.toml at {path}")]
    FileIo {
        /// Path that failed to load.
        path: PathBuf,
        /// I/O error returned by the filesystem.
        #[source]
        source: std::io::Error,
    },
    /// The workspace package version was not present in `Cargo.toml`.
    #[error("workspace version not found in Cargo.toml")]
    WorkspaceVersionNotFound,
    /// The workspace `Cargo.toml` could not be read from disk.
    #[error("failed to read workspace Cargo.toml at {path}")]
    WorkspaceCargoIo {
        /// Workspace manifest path that failed to load.
        path: PathBuf,
        /// I/O error returned by the filesystem.
        #[source]
        source: std::io::Error,
    },
}

/// Flatpak packaging paths and identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FlatpakConfig {
    /// Path to the Flatpak manifest relative to the workspace root.
    pub manifest_path: PathBuf,
    /// Application ID used in generated Flatpak artifacts.
    pub app_id: String,
    /// Directory where Flatpak build output is written.
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

/// Release packaging output settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleaseConfig {
    /// Directory where release artifacts are written.
    pub output_dir: PathBuf,
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("dist-release"),
        }
    }
}

/// Checks to run before producing packaging artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PreflightConfig {
    /// Run `cargo fmt --check`.
    pub run_fmt: bool,
    /// Run Clippy before packaging.
    pub run_clippy: bool,
    /// Run tests before packaging.
    pub run_tests: bool,
    /// Run cargo-deny before packaging.
    pub run_deny: bool,
    /// Run cargo-audit before packaging.
    pub run_audit: bool,
    /// Prefer offline-capable checks when possible.
    pub offline: bool,
    /// Part of the project covered by preflight checks.
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

/// Target scope for packaging preflight checks.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq,
)]
#[serde(rename_all = "lowercase")]
pub enum PreflightScope {
    /// Run checks across the full workspace.
    #[default]
    Workspace,
    /// Limit checks to initialization-related files and commands.
    Init,
}

/// Source used to resolve the package version.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq,
)]
#[serde(rename_all = "lowercase")]
pub enum VersionSource {
    /// Read the version from the workspace manifest.
    #[default]
    Workspace,
    /// Use a manually supplied version value.
    Manual,
}

/// Version resolution settings for package commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionConfig {
    /// Strategy used to determine the package version.
    pub source: VersionSource,
}

impl Default for VersionConfig {
    fn default() -> Self {
        Self {
            source: VersionSource::Workspace,
        }
    }
}

/// Top-level packaging configuration loaded from `packaging.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PackagingConfig {
    /// Flatpak-specific packaging settings.
    pub flatpak: FlatpakConfig,
    /// Release artifact settings.
    pub release: ReleaseConfig,
    /// Preflight check settings.
    pub preflight: PreflightConfig,
    /// Version resolution settings.
    pub version: VersionConfig,
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

            if trimmed.starts_with("version")
                && let Some(eq_pos) = trimmed.find('=')
            {
                let value = trimmed[eq_pos + 1..].trim();
                let version = value.trim_matches('"').trim();
                return Some(version.to_string());
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
