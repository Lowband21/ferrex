use crate::domain::demo::config::{DemoLibraryOptions, DemoSeedOptions};
use crate::domain::scan::scanner::{
    GeneratedNode, StructurePlan, TmdbFolderGenerator,
};
use crate::{
    error::{MediaError, Result},
    infra::media::providers::TmdbApiProvider,
    types::library::LibraryType,
};
use rand::Rng;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

/// Plan produced before writing demo structures to disk.
#[derive(Debug, Clone)]
pub struct DemoSeedPlan {
    pub root: PathBuf,
    pub libraries: Vec<DemoLibraryPlan>,
}

/// Description of a single demo library and the directories/files it contains.
#[derive(Debug, Clone)]
pub struct DemoLibraryPlan {
    pub name: String,
    pub library_type: LibraryType,
    pub root_path: PathBuf,
    pub directories: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
}

impl DemoSeedPlan {
    pub fn library_count(&self) -> usize {
        self.libraries.len()
    }
}

/// Generate a plan that can later be materialised on disk.
pub async fn generate_plan(
    root: &Path,
    options: &DemoSeedOptions,
    tmdb: Arc<TmdbApiProvider>,
) -> Result<DemoSeedPlan> {
    let mut libraries = Vec::new();
    let mut used_names = HashSet::new();

    let generator = Arc::new(TmdbFolderGenerator::new(tmdb));

    for (idx, library_option) in options.libraries.iter().enumerate() {
        let base_name =
            library_option
                .name
                .clone()
                .unwrap_or_else(|| match library_option.library_type {
                    LibraryType::Movies => format!("Demo Movies {}", idx + 1),
                    LibraryType::Series => format!("Demo Series {}", idx + 1),
                });
        let name = uniquify_name(base_name, &mut used_names);
        let root_path = root.join(slug_name(&name));

        info!(
            name = %name,
            ?library_option.library_type,
            "Generating TMDB-backed demo library"
        );

        // Resolve effective language/region (per-library override falls back to global setting)
        let effective_language = library_option
            .language
            .as_deref()
            .or(options.language.as_deref());
        let effective_region = library_option
            .region
            .as_deref()
            .or(options.region.as_deref());

        let structure = match library_option.library_type {
            LibraryType::Movies => {
                let count = library_option.movie_count.unwrap_or(12).max(1);
                generator
                    .generate_movies(
                        &root_path,
                        count,
                        effective_language,
                        effective_region,
                    )
                    .await?
            }
            LibraryType::Series => {
                let count = library_option.series_count.unwrap_or(3).max(1);
                let seasons =
                    library_option.seasons_per_series.unwrap_or((1, 2));
                let episodes =
                    library_option.episodes_per_season.unwrap_or((4, 6));
                generator
                    .generate_series(
                        &root_path,
                        count,
                        effective_language,
                        effective_region,
                        seasons.0..=seasons.1,
                        episodes.0..=episodes.1,
                    )
                    .await?
            }
        };

        let mut plan = structure_to_demo_plan(
            name,
            library_option.library_type,
            root_path,
            structure,
        );
        apply_deviations(
            &mut plan,
            library_option,
            options.allow_deviations,
            options.deviation_rate,
        );
        libraries.push(plan);
    }

    Ok(DemoSeedPlan {
        root: root.to_path_buf(),
        libraries,
    })
}

fn structure_to_demo_plan(
    name: String,
    library_type: LibraryType,
    root_path: PathBuf,
    structure: StructurePlan,
) -> DemoLibraryPlan {
    let mut directories = Vec::new();
    let mut files = Vec::new();

    for node in structure.nodes {
        match node {
            GeneratedNode::Dir(path) => directories.push(path),
            GeneratedNode::File { path, .. } => files.push(path),
        }
    }

    DemoLibraryPlan {
        name,
        library_type,
        root_path,
        directories,
        files,
    }
}

/// Materialise the generated structure on disk by creating all planned
/// directories and files.
pub fn apply_plan(plan: &DemoSeedPlan) -> Result<()> {
    for library in &plan.libraries {
        for dir in &library.directories {
            std::fs::create_dir_all(dir).map_err(|err| {
                MediaError::Io(std::io::Error::new(
                    err.kind(),
                    format!(
                        "failed to create demo directory {}: {}",
                        dir.display(),
                        err
                    ),
                ))
            })?;
        }

        for file in &library.files {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    MediaError::Io(std::io::Error::new(
                        err.kind(),
                        format!(
                            "failed to prepare parent {}: {}",
                            parent.display(),
                            err
                        ),
                    ))
                })?;
            }
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(file)
                .map_err(|err| {
                    MediaError::Io(std::io::Error::new(
                        err.kind(),
                        format!(
                            "failed to create demo file {}: {}",
                            file.display(),
                            err
                        ),
                    ))
                })?;
        }
    }

    Ok(())
}

/// Remove library roots that are no longer part of the plan and ensure
/// upcoming roots are clean before materialisation. This keeps the demo tree
/// deterministic between resets and avoids leftover structures skewing scans.
pub fn prepare_plan_roots(
    previous: Option<&DemoSeedPlan>,
    next: &DemoSeedPlan,
) -> Result<()> {
    let mut next_roots: HashSet<PathBuf> = HashSet::new();
    for library in &next.libraries {
        next_roots.insert(library.root_path.clone());
    }

    if let Some(prev) = previous {
        for library in &prev.libraries {
            if next_roots.contains(&library.root_path) {
                continue;
            }

            if library.root_path.exists() {
                std::fs::remove_dir_all(&library.root_path).map_err(|err| {
                    MediaError::Io(std::io::Error::new(
                        err.kind(),
                        format!(
                            "failed to remove stale demo root {}: {}",
                            library.root_path.display(),
                            err
                        ),
                    ))
                })?;
            }
        }
    }

    for root in next_roots {
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|err| {
                MediaError::Io(std::io::Error::new(
                    err.kind(),
                    format!(
                        "failed to reset demo root {}: {}",
                        root.display(),
                        err
                    ),
                ))
            })?;
        }
    }

    Ok(())
}

fn slug_name(value: &str) -> String {
    let mut out = value
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            _ => c,
        })
        .collect::<String>();
    out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.trim().to_string()
}

fn uniquify_name(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }

    let mut counter = 2u32;
    loop {
        let candidate = format!("{} ({})", base, counter);
        if used.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

fn apply_deviations(
    plan: &mut DemoLibraryPlan,
    options: &DemoLibraryOptions,
    global_flag: bool,
    rate: f32,
) {
    if !options.effective_deviation_flag(global_flag) || rate <= 0.0 {
        return;
    }

    match plan.library_type {
        LibraryType::Movies => apply_movie_deviations(plan, rate),
        LibraryType::Series => apply_series_deviations(plan, rate),
    }
}

fn apply_movie_deviations(plan: &mut DemoLibraryPlan, rate: f32) {
    let mut rng = rand::rng();
    let root = plan.root_path.clone();
    let movie_dirs: Vec<PathBuf> = plan
        .directories
        .iter()
        .filter(|dir| dir.parent() == Some(root.as_path()) && dir != &&root)
        .cloned()
        .collect();

    for dir in movie_dirs {
        let mut current_dir = dir.clone();

        if rng.random::<f32>() < rate
            && let Some(name) = dir.file_name().and_then(|n| n.to_str())
        {
            let new_dir = dir.with_file_name(format!("{} - UNSORTED", name));
            rename_subtree(plan, &dir, &new_dir);
            current_dir = new_dir;
        }

        if rng.random::<f32>() < rate / 2.0
            && let Some(target) = plan
                .files
                .iter()
                .find(|file| file.parent() == Some(current_dir.as_path()))
                .cloned()
        {
            plan.files.retain(|file| file != &target);
        }
    }
}

fn apply_series_deviations(plan: &mut DemoLibraryPlan, rate: f32) {
    let mut rng = rand::rng();
    let root = plan.root_path.clone();
    let series_dirs: Vec<PathBuf> = plan
        .directories
        .iter()
        .filter(|dir| dir.parent() == Some(root.as_path()) && dir != &&root)
        .cloned()
        .collect();

    for series_dir in series_dirs {
        let season_dirs: Vec<PathBuf> = plan
            .directories
            .iter()
            .filter(|dir| dir.parent() == Some(series_dir.as_path()))
            .cloned()
            .collect();

        for season_dir in season_dirs {
            let mut current_dir = season_dir.clone();

            if rng.random::<f32>() < rate {
                let season_number = season_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(extract_number)
                    .unwrap_or(1);
                let new_dir =
                    season_dir.with_file_name(format!("S{:02}", season_number));
                rename_subtree(plan, &season_dir, &new_dir);
                current_dir = new_dir;
            }

            if rng.random::<f32>() < rate
                && let Some(target) = plan
                    .files
                    .iter()
                    .find(|file| file.parent() == Some(current_dir.as_path()))
                    .cloned()
            {
                plan.files.retain(|file| file != &target);
            }
        }
    }
}

fn rename_subtree(plan: &mut DemoLibraryPlan, old: &Path, new: &Path) {
    for dir in &mut plan.directories {
        if dir.starts_with(old) {
            let suffix = dir.strip_prefix(old).unwrap();
            if suffix.as_os_str().is_empty() {
                *dir = new.to_path_buf();
            } else {
                *dir = new.join(suffix);
            }
        }
    }

    for file in &mut plan.files {
        if file.starts_with(old) {
            let suffix = file.strip_prefix(old).unwrap();
            *file = new.join(suffix);
        }
    }
}

fn extract_number(name: &str) -> Option<u32> {
    let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use std::{fs, io::Write};
    use tempfile::tempdir;

    #[test]
    fn prepare_plan_roots_removes_obsolete_directories() {
        let tempdir = tempdir().expect("tempdir");
        let root = tempdir.path().to_path_buf();

        let old_library_root = root.join("lib-old");
        let old_file = old_library_root.join("movie.mkv");
        fs::create_dir_all(&old_library_root).expect("create old root");
        fs::File::create(&old_file)
            .and_then(|mut file| file.write_all(b"old"))
            .expect("seed old file");

        let previous = DemoSeedPlan {
            root: root.clone(),
            libraries: vec![DemoLibraryPlan {
                name: "Old".into(),
                library_type: LibraryType::Movies,
                root_path: old_library_root.clone(),
                directories: vec![old_library_root.clone()],
                files: vec![old_file.clone()],
            }],
        };

        let new_library_root = root.join("lib-new");
        let new_file = new_library_root.join("feature.mkv");
        let next = DemoSeedPlan {
            root: root.clone(),
            libraries: vec![DemoLibraryPlan {
                name: "New".into(),
                library_type: LibraryType::Movies,
                root_path: new_library_root.clone(),
                directories: vec![new_library_root.clone()],
                files: vec![new_file.clone()],
            }],
        };

        prepare_plan_roots(Some(&previous), &next).expect("prepare roots");

        assert!(
            !old_library_root.exists(),
            "stale demo library directory should be removed"
        );

        apply_plan(&next).expect("apply new plan");
        assert!(new_library_root.exists());
        assert!(new_file.exists());
    }
}
