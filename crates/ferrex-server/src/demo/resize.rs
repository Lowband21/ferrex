use anyhow::{Context, Result};
use ferrex_core::domain::demo::DemoLibraryPlan;
use ferrex_core::domain::scan::scanner::{GeneratedNode, StructurePlan};
use ferrex_core::types::library::LibraryType;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn is_ignored_primary_child_dir(name: &str) -> bool {
    // Demo libraries should only contain media folders at the top-level.
    // Ignore hidden/system directories created by OS tooling or scanners.
    name.starts_with('.')
}

fn is_supported_movie_file_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mkv"
            | "mp4"
            | "avi"
            | "mov"
            | "webm"
            | "flv"
            | "wmv"
            | "mpg"
            | "mpeg"
            | "m4v"
            | "3gp"
            | "ts"
    )
}

pub fn primary_item_roots_on_disk(library_root: &Path) -> Result<Vec<PathBuf>> {
    if !library_root.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let entries = std::fs::read_dir(library_root).with_context(|| {
        format!(
            "failed to read demo library directory {}",
            library_root.display()
        )
    })?;

    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to read demo library directory entry in {}",
                library_root.display()
            )
        })?;

        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_ignored_primary_child_dir(name) {
            continue;
        }

        items.push(path);
    }

    items.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    Ok(items)
}

/// Enumerate primary items currently on disk for a demo library.
///
/// - Movies: movie folders under the library root.
/// - Series: series folders under the library root.
pub fn primary_item_paths_on_disk(
    library_type: LibraryType,
    library_root: &Path,
) -> Result<Vec<PathBuf>> {
    match library_type {
        LibraryType::Movies => {
            // Movies must be directories directly under the library root.
            // If media files are found at the root, treat it as invalid demo structure.
            if library_root.exists() {
                let entries =
                    std::fs::read_dir(library_root).with_context(|| {
                        format!(
                            "failed to read demo library directory {}",
                            library_root.display()
                        )
                    })?;

                for entry in entries {
                    let entry = entry.with_context(|| {
                        format!(
                            "failed to read demo library directory entry in {}",
                            library_root.display()
                        )
                    })?;
                    let Ok(file_type) = entry.file_type() else {
                        continue;
                    };
                    if !file_type.is_file() {
                        continue;
                    }

                    let path = entry.path();
                    let Some(name) = path.file_name().and_then(|n| n.to_str())
                    else {
                        continue;
                    };
                    if name.starts_with('.') {
                        continue;
                    }

                    if let Some(ext) = path.extension().and_then(|e| e.to_str())
                        && is_supported_movie_file_ext(ext)
                    {
                        anyhow::bail!(
                            "invalid demo movie library structure: found media file {} directly under library root {}",
                            path.display(),
                            library_root.display()
                        );
                    }
                }
            }

            primary_item_roots_on_disk(library_root)
        }
        LibraryType::Series => primary_item_roots_on_disk(library_root),
    }
}

pub fn current_folder_names_on_disk(
    library_root: &Path,
) -> Result<HashSet<String>> {
    Ok(primary_item_roots_on_disk(library_root)?
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .collect())
}

pub fn remove_fs_item(item: &Path) -> Result<()> {
    if !item.exists() {
        return Ok(());
    }

    let metadata = std::fs::metadata(item).with_context(|| {
        format!("failed to stat demo item {}", item.display())
    })?;

    if metadata.is_dir() {
        std::fs::remove_dir_all(item).with_context(|| {
            format!("failed to remove demo directory {}", item.display())
        })?;
        return Ok(());
    }

    if metadata.is_file() {
        std::fs::remove_file(item).with_context(|| {
            format!("failed to remove demo file {}", item.display())
        })?;
        return Ok(());
    }

    Ok(())
}

pub fn remove_item_subtree(plan: &mut DemoLibraryPlan, item_root: &Path) {
    plan.directories.retain(|dir| !dir.starts_with(item_root));
    plan.files.retain(|file| !file.starts_with(item_root));
}

pub fn structure_nodes_to_paths(
    structure: &StructurePlan,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for node in &structure.nodes {
        match node {
            GeneratedNode::Dir(path) => dirs.push(path.clone()),
            GeneratedNode::File { path, .. } => files.push(path.clone()),
        }
    }

    (dirs, files)
}

pub fn structure_item_roots(
    structure: &StructurePlan,
    library_root: &Path,
) -> Vec<PathBuf> {
    let mut items = Vec::new();
    for node in &structure.nodes {
        let GeneratedNode::Dir(path) = node else {
            continue;
        };
        if path.parent() == Some(library_root) && path != library_root {
            items.push(path.clone());
        }
    }

    items.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    items
}

pub fn merge_structure_into_plan(
    plan: &mut DemoLibraryPlan,
    structure: &StructurePlan,
) -> Result<Vec<PathBuf>> {
    let (mut dirs, mut files) = structure_nodes_to_paths(structure);
    let added_items = structure_item_roots(structure, &plan.root_path);

    let mut existing_dirs: HashSet<PathBuf> =
        plan.directories.iter().cloned().collect();
    let mut existing_files: HashSet<PathBuf> =
        plan.files.iter().cloned().collect();

    dirs.retain(|dir| existing_dirs.insert(dir.clone()));
    files.retain(|file| existing_files.insert(file.clone()));

    plan.directories.extend(dirs);
    plan.files.extend(files);

    Ok(added_items)
}

pub fn ensure_within_root(root: &Path, candidate: &Path) -> Result<()> {
    if !candidate.starts_with(root) {
        anyhow::bail!(
            "refusing to mutate path outside demo root: {} (root={})",
            candidate.display(),
            root.display()
        );
    }
    Ok(())
}

pub fn create_zero_length_files(
    directories: &[PathBuf],
    files: &[PathBuf],
) -> Result<()> {
    for dir in directories {
        std::fs::create_dir_all(dir).with_context(|| {
            format!("failed to create demo directory {}", dir.display())
        })?;
    }

    for file in files {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to prepare parent directory {}",
                    parent.display()
                )
            })?;
        }

        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(file)
            .with_context(|| {
                format!("failed to create demo file {}", file.display())
            })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn primary_item_roots_on_disk_returns_sorted_directories() {
        let tempdir = tempdir().expect("tempdir");
        let root = tempdir.path();

        std::fs::create_dir_all(root.join("B Movie")).expect("mkdir");
        std::fs::create_dir_all(root.join("A Movie")).expect("mkdir");
        std::fs::write(root.join("not-a-dir.txt"), "x").expect("write file");

        let items = primary_item_roots_on_disk(root).expect("list items");
        let names: Vec<String> = items
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(names, vec!["A Movie".to_string(), "B Movie".to_string()]);
    }

    #[test]
    fn primary_item_roots_on_disk_ignores_hidden_directories() {
        let tempdir = tempdir().expect("tempdir");
        let root = tempdir.path();

        std::fs::create_dir_all(root.join(".scanner")).expect("mkdir");
        std::fs::create_dir_all(root.join("Visible")).expect("mkdir");

        let items = primary_item_roots_on_disk(root).expect("list items");
        let names: Vec<String> = items
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(names, vec!["Visible".to_string()]);
    }
}
