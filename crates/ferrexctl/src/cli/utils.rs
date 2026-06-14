use std::path::{Path, PathBuf};

use tracing::{error, info, warn};

use crate::cli::options;

/// Derive a stable compose project name from the env file location.
pub fn derive_compose_project_name(env_file: &Path) -> String {
    let env_dir = env_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let env_parent = env_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    // Slugify: lowercase, non-alnum -> '-'.
    let mut slug = env_parent
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();

    // Collapse multiple '-'.
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug.trim_matches('-').to_string();

    if slug.is_empty() || slug == "env" || slug == "." {
        "ferrex".to_string()
    } else {
        format!("ferrex-{slug}")
    }
}

pub fn resolve_project_name(opts: &options::StackOptions) -> String {
    if let Some(p) = &opts.project_name_override {
        return p.clone();
    }
    derive_compose_project_name(&opts.env_file)
}
pub fn host_pid_file_path(env_file: &Path, project_name: &str) -> PathBuf {
    let dir = env_file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    dir.join(format!(".{project_name}.ferrex-server.pid"))
}

pub fn workspace_root() -> PathBuf {
    match std::env::var("WORKSPACE_ROOT") {
        Ok(root) => PathBuf::from(root),
        Err(_) => match std::env::current_dir() {
            Ok(cwd) => find_workspace_root(&cwd).unwrap_or(cwd),
            Err(err) => {
                error!(
                    "Failed to get current working dir. Check permissions and that path still exists. Error: {:#?}",
                    err
                );
                warn!("Defaulting to path from '.'");
                PathBuf::from(".")
            }
        },
    }
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let manifest = dir.join("Cargo.toml");
        let manifest_contents = std::fs::read_to_string(&manifest).ok()?;
        manifest_contents
            .contains("[workspace]")
            .then(|| dir.to_path_buf())
    })
}

pub fn compose_root() -> PathBuf {
    match std::env::var("FERREX_COMPOSE_ROOT") {
        Ok(root) => {
            info!(
                "Loaded compose root folder from FERREX_COMPOSE_ROOT as {}",
                root
            );
            PathBuf::from(root)
        }
        Err(_) => {
            let workspace_root = workspace_root();
            info!(
                "FERREX_COMPOSE_ROOT not found, defaulting to workspace root at {:#?}.",
                workspace_root
            );
            workspace_root
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::find_workspace_root;

    #[test]
    fn finds_workspace_root_from_crates_ferrexctl_manifest_dir() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = find_workspace_root(&manifest_dir).expect(
            "ferrexctl manifest dir should live under the workspace root",
        );

        assert_eq!(manifest_dir, workspace.join("crates").join("ferrexctl"));
        assert!(workspace.join("Cargo.toml").is_file());
        assert!(
            workspace
                .join("crates")
                .join("ferrexctl")
                .join("Cargo.toml")
                .is_file()
        );
    }
}
