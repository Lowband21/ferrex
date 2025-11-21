use std::path::{Path, PathBuf};

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
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn compose_root() -> PathBuf {
    if let Ok(root) = std::env::var("FERREX_COMPOSE_ROOT") {
        PathBuf::from(root)
    } else {
        workspace_root()
    }
}
