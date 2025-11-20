use std::{
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use axum::{
    extract::{Query, State},
    response::Json,
};
use ferrex_core::api::types::{
    ApiResponse,
    admin::{
        MediaRootBreadcrumb, MediaRootBrowseRequest, MediaRootBrowseResponse,
        MediaRootEntry, MediaRootEntryKind,
    },
};
use tokio::fs;

use crate::infra::{
    app_state::AppState,
    errors::{AppError, AppResult},
};

/// List folders/files beneath the configured media root for admin-oriented UX.
///
/// Returns entries relative to the media root to make container path mappings
/// transparent to the UI without leaking host-only locations.
pub async fn browse_media_root(
    State(state): State<AppState>,
    Query(request): Query<MediaRootBrowseRequest>,
) -> AppResult<Json<ApiResponse<MediaRootBrowseResponse>>> {
    let configured_root =
        state.config().media.root.clone().ok_or_else(|| {
            AppError::bad_request(
                "Media root is not configured. Set MEDIA_ROOT in the environment before browsing.",
            )
        })?;

    let media_root =
        canonicalize_media_root(&configured_root)
            .await
            .map_err(|_| {
                AppError::bad_request(format!(
                    "Media root {:?} is not accessible. Ensure the path exists \
                 inside the container and is mounted correctly.",
                    configured_root
                ))
            })?;

    let relative_segments = normalise_relative_path(request.path.as_deref())?;
    let current_relative = relative_segments.join("/");
    let display_path = if current_relative.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", &current_relative)
    };
    let parent_path = if relative_segments.is_empty() {
        None
    } else {
        Some(relative_segments[..relative_segments.len() - 1].join("/"))
    };

    let mut current_dir = media_root.clone();
    for segment in &relative_segments {
        current_dir.push(segment);
    }

    let mut entries = match fs::read_dir(&current_dir).await {
        Ok(dir) => collect_entries(dir, &relative_segments).await?,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(AppError::bad_request(format!(
                "Path '{}' does not exist within the media root.",
                display_path
            )));
        }
        Err(err) => {
            return Err(AppError::internal(format!(
                "Failed to read directory {:?}: {}",
                current_dir, err
            )));
        }
    };

    entries.sort_by(|a, b| match (a.kind, b.kind) {
        (MediaRootEntryKind::Directory, MediaRootEntryKind::File)
        | (MediaRootEntryKind::Directory, MediaRootEntryKind::Other)
        | (MediaRootEntryKind::File, MediaRootEntryKind::Other) => {
            std::cmp::Ordering::Less
        }
        (MediaRootEntryKind::File, MediaRootEntryKind::Directory)
        | (MediaRootEntryKind::Other, MediaRootEntryKind::Directory)
        | (MediaRootEntryKind::Other, MediaRootEntryKind::File) => {
            std::cmp::Ordering::Greater
        }
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let mut breadcrumbs = Vec::with_capacity(relative_segments.len() + 1);
    breadcrumbs.push(MediaRootBreadcrumb {
        label: "/".to_string(),
        relative_path: String::new(),
    });
    for idx in 0..relative_segments.len() {
        breadcrumbs.push(MediaRootBreadcrumb {
            label: relative_segments[idx].clone(),
            relative_path: relative_segments[..=idx].join("/"),
        });
    }

    let response = MediaRootBrowseResponse {
        media_root: media_root.display().to_string(),
        current_path: current_relative,
        parent_path,
        display_path,
        breadcrumbs,
        entries,
    };

    Ok(Json(ApiResponse::success(response)))
}

async fn canonicalize_media_root(root: &Path) -> Result<PathBuf, ()> {
    if root.is_absolute() {
        fs::canonicalize(root).await.map_err(|_| ())
    } else {
        let joined = std::env::current_dir().map_err(|_| ())?.join(root);
        fs::canonicalize(joined).await.map_err(|_| ())
    }
}

fn normalise_relative_path(
    path: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let mut segments = Vec::new();
    let Some(raw) = path else {
        return Ok(segments);
    };

    if raw.trim().is_empty() {
        return Ok(segments);
    }

    for component in Path::new(raw).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let segment = part.to_string_lossy();
                if segment.contains('/') {
                    return Err(AppError::bad_request(
                        "Path components must not contain '/' separators.",
                    ));
                }
                segments.push(segment.to_string());
            }
            Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(AppError::bad_request(
                    "Only relative paths within the media root are allowed.",
                ));
            }
        }
    }

    Ok(segments)
}

async fn collect_entries(
    mut dir: fs::ReadDir,
    current_segments: &[String],
) -> AppResult<Vec<MediaRootEntry>> {
    let mut entries = Vec::new();
    while let Some(entry) = dir.next_entry().await.map_err(|err| {
        AppError::internal(format!("Failed to enumerate directory: {}", err))
    })? {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy().to_string();

        // hidden dot dirs? still include? yes.
        let metadata = entry.metadata().await.map_err(|err| {
            AppError::internal(format!(
                "Failed to fetch metadata for {:?}: {}",
                entry.path(),
                err
            ))
        })?;
        let file_type = metadata.file_type();

        let mut entry_segments = current_segments.to_vec();
        entry_segments.push(file_name.clone());
        let relative_path = entry_segments.join("/");

        let kind = if file_type.is_dir() {
            MediaRootEntryKind::Directory
        } else if file_type.is_file() {
            MediaRootEntryKind::File
        } else {
            MediaRootEntryKind::Other
        };

        entries.push(MediaRootEntry {
            name: file_name,
            relative_path,
            kind,
            is_symlink: file_type.is_symlink(),
        });
    }

    // Add synthetic `..` entry for navigation when not at root.
    if !current_segments.is_empty() {
        entries.push(MediaRootEntry {
            name: "..".into(),
            relative_path: current_segments[..current_segments.len() - 1]
                .join("/"),
            kind: MediaRootEntryKind::Directory,
            is_symlink: false,
        });
    }

    Ok(entries)
}
