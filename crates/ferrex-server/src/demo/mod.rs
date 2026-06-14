pub use crate::db::derive_demo_database_url;
use crate::infra::scan::scan_manager::ScanControlPlane;
use crate::infra::{app_state::AppState, config::Config};

use ferrex_core::domain::scan::normalize_path;
use ferrex_core::{
    api::types::{DemoLibraryStatus, DemoResetRequest, DemoStatus},
    application::unit_of_work::AppUnitOfWork,
    domain::scan::scanner::StructurePlan,
    domain::users::rbac::roles,
    infra::providers::TmdbApiProvider,
    types::{LibraryId, library::LibraryType},
};

use crate::handlers::users::{UserService, user_service::CreateUserParams};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ferrex_core::domain::demo::{self, DemoSeedOptions};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::warn;

mod resize;
use resize::{
    create_zero_length_files, current_folder_names_on_disk, ensure_within_root,
    merge_structure_into_plan, primary_item_paths_on_disk, remove_fs_item,
    remove_item_subtree, structure_nodes_to_paths,
};

#[async_trait]
pub trait DemoPlanProvider: Send + Sync {
    async fn generate_plan(
        &self,
        root: &Path,
        options: &DemoSeedOptions,
    ) -> Result<demo::DemoSeedPlan>;

    async fn generate_movie_structure(
        &self,
        library_root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<StructurePlan> {
        let _ = (
            library_root,
            count,
            language,
            region,
            forbidden_folder_names,
        );
        Err(anyhow!("demo plan provider does not support movie deltas"))
    }

    async fn generate_series_structure(
        &self,
        library_root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        seasons_per_series: std::ops::RangeInclusive<u8>,
        episodes_per_season: std::ops::RangeInclusive<u16>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<StructurePlan> {
        let _ = (
            library_root,
            count,
            language,
            region,
            seasons_per_series,
            episodes_per_season,
            forbidden_folder_names,
        );
        Err(anyhow!("demo plan provider does not support series deltas"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct DemoSizeOverrides {
    pub movie_count: Option<usize>,
    pub series_count: Option<usize>,
}

impl From<DemoResetRequest> for DemoSizeOverrides {
    fn from(value: DemoResetRequest) -> Self {
        Self {
            movie_count: value.movie_count,
            series_count: value.series_count,
        }
    }
}

#[derive(Debug)]
pub struct TmdbPlanProvider {
    tmdb: Arc<TmdbApiProvider>,
}

impl TmdbPlanProvider {
    pub fn new(tmdb: Arc<TmdbApiProvider>) -> Self {
        Self { tmdb }
    }
}

#[async_trait]
impl DemoPlanProvider for TmdbPlanProvider {
    async fn generate_plan(
        &self,
        root: &Path,
        options: &DemoSeedOptions,
    ) -> Result<demo::DemoSeedPlan> {
        demo::generate_plan(root, options, self.tmdb.clone())
            .await
            .context("failed to plan demo structure via TMDB")
    }

    async fn generate_movie_structure(
        &self,
        library_root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<StructurePlan> {
        let generator =
            ferrex_core::domain::scan::scanner::TmdbFolderGenerator::new(
                self.tmdb.clone(),
            );
        generator
            .generate_movies_excluding(
                library_root,
                count,
                language,
                region,
                forbidden_folder_names,
            )
            .await
            .context("failed to generate movie delta structure")
    }

    async fn generate_series_structure(
        &self,
        library_root: &Path,
        count: usize,
        language: Option<&str>,
        region: Option<&str>,
        seasons_per_series: std::ops::RangeInclusive<u8>,
        episodes_per_season: std::ops::RangeInclusive<u16>,
        forbidden_folder_names: &std::collections::HashSet<String>,
    ) -> Result<StructurePlan> {
        let generator =
            ferrex_core::domain::scan::scanner::TmdbFolderGenerator::new(
                self.tmdb.clone(),
            );
        generator
            .generate_series_excluding(
                library_root,
                count,
                language,
                region,
                seasons_per_series,
                episodes_per_season,
                forbidden_folder_names,
            )
            .await
            .context("failed to generate series delta structure")
    }
}

pub struct DemoCoordinator {
    options: Arc<Mutex<DemoSeedOptions>>,
    plan: Mutex<demo::DemoSeedPlan>,
    root: PathBuf,
    pub username: String,
    pub password: String,
    library_ids: Mutex<Vec<LibraryId>>,
    plan_provider: Arc<dyn DemoPlanProvider>,
}

impl std::fmt::Debug for DemoCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DemoCoordinator")
            .field("root", &self.root)
            .field("username", &self.username)
            .finish()
    }
}

impl DemoCoordinator {
    pub async fn bootstrap(
        config: &mut Config,
        tmdb: Arc<TmdbApiProvider>,
    ) -> Result<Self> {
        if std::env::var("TMDB_API_KEY")
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            return Err(anyhow!(
                "TMDB_API_KEY must be configured to materialise demo media. Supply a key or stub the provider before enabling demo mode."
            ));
        }

        let options = DemoSeedOptions::from_env();
        let provider = Arc::new(TmdbPlanProvider::new(tmdb));
        Self::bootstrap_with_provider(config, options, provider).await
    }

    pub async fn bootstrap_with_provider(
        config: &mut Config,
        options: DemoSeedOptions,
        plan_provider: Arc<dyn DemoPlanProvider>,
    ) -> Result<Self> {
        let root = absolutize_demo_root(resolve_root(&options, config))
            .context("failed to resolve demo root path")?;

        // In demo mode, scope caches under the demo root so demo runs are
        // fully self-contained and writable even in constrained environments.
        config.cache.root = root.join("cache");
        config.cache.transcode = config.cache.root.join("transcode");
        config.cache.thumbnails = config.cache.root.join("thumbnails");

        // Ensure cache directories exist and normalize to absolute paths
        config
            .ensure_directories()
            .context("failed to create demo cache directories")?;
        config
            .normalize_paths()
            .context("failed to normalize demo cache paths")?;

        std::fs::create_dir_all(&root)
            .context("failed to create demo root directory")?;

        let options_shared = Arc::new(Mutex::new(options));
        let initial_options = {
            let guard = options_shared.lock().await;
            guard.clone()
        };

        let plan = plan_provider
            .generate_plan(&root, &initial_options)
            .await
            .context("failed to plan demo structure")?;

        ensure_demo_root_clean(&root, &plan)
            .context("failed to clean existing demo filesystem")?;

        demo::prepare_plan_roots(None, &plan)
            .context("failed to prepare demo filesystem for bootstrap")?;
        demo::apply_plan(&plan)
            .context("failed to materialise demo file tree")?;

        // Ensure server points at the demo root
        config.media.root = Some(root.clone());

        // Initialise shared context for downstream components
        demo::init_demo_context(root.clone(), initial_options.policy())?;

        let username = env_nonempty_trimmed("FERREX_DEMO_USERNAME")
            .unwrap_or_else(|| "demo".into());
        let password = env_nonempty_trimmed("FERREX_DEMO_PASSWORD")
            .unwrap_or_else(|| "demodemo".into());

        Ok(Self {
            options: options_shared,
            plan: Mutex::new(plan),
            root,
            username,
            password,
            library_ids: Mutex::new(Vec::new()),
            plan_provider,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn library_ids(&self) -> Vec<LibraryId> {
        self.library_ids.lock().await.clone()
    }

    async fn apply_overrides(&self, overrides: &DemoSizeOverrides) {
        if overrides.movie_count.is_none() && overrides.series_count.is_none() {
            return;
        }

        let mut options = self.options.lock().await;
        for library in &mut options.libraries {
            match library.library_type {
                LibraryType::Movies => {
                    if let Some(count) = overrides.movie_count {
                        library.movie_count = Some(count.max(1));
                    }
                }
                LibraryType::Series => {
                    if let Some(count) = overrides.series_count {
                        library.series_count = Some(count.max(1));
                    }
                }
            }
        }
    }

    pub async fn sync_database(
        &self,
        unit_of_work: Arc<AppUnitOfWork>,
    ) -> Result<Vec<LibraryId>> {
        let mut registered_ids = Vec::new();

        let plans = {
            let guard = self.plan.lock().await;
            guard.libraries.clone()
        };
        let existing = unit_of_work
            .libraries
            .list_libraries()
            .await
            .context("failed to list libraries during demo bootstrap")?;

        demo::clear_registered_libraries();

        for plan in &plans {
            if let Some(existing) =
                existing.iter().find(|lib| lib.name == plan.name)
            {
                demo::register_demo_library(existing);
                registered_ids.push(existing.id);
                continue;
            }

            let library = demo::library_from_plan(plan);
            unit_of_work
                .libraries
                .create_library(library.clone())
                .await
                .context("failed to create demo library")?;
            demo::register_demo_library(&library);
            registered_ids.push(library.id);
        }

        // Track for future resets
        *self.library_ids.lock().await = registered_ids.clone();
        Ok(registered_ids)
    }

    pub async fn reset(
        &self,
        unit_of_work: Arc<AppUnitOfWork>,
        overrides: Option<DemoSizeOverrides>,
    ) -> Result<()> {
        let previous_plan = {
            let guard = self.plan.lock().await;
            guard.clone()
        };

        if let Some(overrides) = overrides {
            self.apply_overrides(&overrides).await;
        }

        let options_snapshot = {
            let guard = self.options.lock().await;
            guard.clone()
        };

        let new_plan = self
            .plan_provider
            .generate_plan(&self.root, &options_snapshot)
            .await
            .context("failed to regenerate demo plan")?;

        demo::prepare_plan_roots(Some(&previous_plan), &new_plan)
            .context("failed to prepare demo filesystem for reset")?;
        demo::apply_plan(&new_plan)
            .context("failed to reapply demo folder structure")?;

        {
            let mut guard = self.plan.lock().await;
            *guard = new_plan;
        }

        self.sync_database(unit_of_work).await.map(|_| ())
    }

    pub async fn resize(
        &self,
        unit_of_work: Arc<AppUnitOfWork>,
        scan_control: &ScanControlPlane,
        overrides: DemoSizeOverrides,
    ) -> Result<()> {
        self.apply_overrides(&overrides).await;

        let options_snapshot = {
            let guard = self.options.lock().await;
            guard.clone()
        };

        let library_ids = self.library_ids.lock().await.clone();

        let mut plan_guard = self.plan.lock().await;
        if plan_guard.libraries.len() != options_snapshot.libraries.len() {
            return Err(anyhow!(
                "demo plan/options mismatch (plan_libraries={}, option_libraries={})",
                plan_guard.libraries.len(),
                options_snapshot.libraries.len()
            ));
        }

        // We inject synthetic FS events for any newly added item roots so the
        // scanner can pick up additions without a full library rescan.
        let mut scan_bursts: Vec<(LibraryId, Vec<PathBuf>)> = Vec::new();

        for (idx, (plan_lib, opts_lib)) in plan_guard
            .libraries
            .iter_mut()
            .zip(options_snapshot.libraries.iter())
            .enumerate()
        {
            let Some(library_id) = library_ids.get(idx).copied() else {
                return Err(anyhow!("demo library ids not initialised"));
            };

            // Resolve effective language/region (per-library override falls back to global setting)
            let effective_language = opts_lib
                .language
                .as_deref()
                .or(options_snapshot.language.as_deref());
            let effective_region = opts_lib
                .region
                .as_deref()
                .or(options_snapshot.region.as_deref());

            let target_primary = match plan_lib.library_type {
                LibraryType::Movies => {
                    opts_lib.movie_count.unwrap_or(12).max(1)
                }
                LibraryType::Series => {
                    opts_lib.series_count.unwrap_or(3).max(1)
                }
            };

            let current_items = primary_item_paths_on_disk(
                plan_lib.library_type,
                &plan_lib.root_path,
            )
            .with_context(|| {
                format!(
                    "failed to enumerate demo roots for {}",
                    plan_lib.root_path.display()
                )
            })?;
            let current_primary = current_items.len();

            if current_primary > target_primary {
                let remove_count = current_primary - target_primary;
                let to_remove = current_items
                    .into_iter()
                    .rev()
                    .take(remove_count)
                    .collect::<Vec<_>>();

                let mut media_prefixes: Vec<String> = Vec::new();
                let mut inventory_prefixes: Vec<String> = Vec::new();

                for item_root in &to_remove {
                    ensure_within_root(&self.root, item_root)?;
                    remove_fs_item(item_root)?;
                    remove_item_subtree(plan_lib, item_root);

                    media_prefixes.push(prefix_for_like(item_root));
                    inventory_prefixes.push(normalize_path(item_root)?);
                }

                if !media_prefixes.is_empty() {
                    let _deleted = unit_of_work
                        .media_files_write
                        .delete_by_path_prefixes(library_id, media_prefixes)
                        .await
                        .context("failed to delete demo media file rows")?;

                    let _deleted_folders = unit_of_work
                        .folder_inventory
                        .delete_by_path_prefixes(library_id, inventory_prefixes)
                        .await
                        .context(
                            "failed to delete demo folder inventory rows",
                        )?;

                    if matches!(plan_lib.library_type, LibraryType::Series) {
                        let _ = unit_of_work
                            .media_refs
                            .cleanup_orphan_tv_references(library_id)
                            .await
                            .context(
                                "failed to cleanup orphan TV references",
                            )?;
                    }
                }
            } else if current_primary < target_primary {
                let add_count = target_primary - current_primary;
                let forbidden =
                    current_folder_names_on_disk(&plan_lib.root_path)?;

                let structure = match plan_lib.library_type {
                    LibraryType::Movies => {
                        self.plan_provider
                            .generate_movie_structure(
                                &plan_lib.root_path,
                                add_count,
                                effective_language,
                                effective_region,
                                &forbidden,
                            )
                            .await?
                    }
                    LibraryType::Series => {
                        let seasons =
                            opts_lib.seasons_per_series.unwrap_or((1, 2));
                        let episodes =
                            opts_lib.episodes_per_season.unwrap_or((4, 6));
                        self.plan_provider
                            .generate_series_structure(
                                &plan_lib.root_path,
                                add_count,
                                effective_language,
                                effective_region,
                                seasons.0..=seasons.1,
                                episodes.0..=episodes.1,
                                &forbidden,
                            )
                            .await?
                    }
                };

                let (dirs, files) = structure_nodes_to_paths(&structure);
                for dir in &dirs {
                    ensure_within_root(&self.root, dir)?;
                }
                for file in &files {
                    ensure_within_root(&self.root, file)?;
                }

                create_zero_length_files(&dirs, &files)?;
                let added_items =
                    merge_structure_into_plan(plan_lib, &structure)?;
                if !added_items.is_empty() {
                    scan_bursts.push((library_id, added_items));
                }
            }
        }

        // Ensure the scan runtime is in maintenance mode and enqueue minimal
        // folder scans for any newly-added demo items.
        for (library_id, folders) in scan_bursts {
            scan_control
                .inject_created_folders(library_id, folders)
                .await
                .context("failed to enqueue demo delta scans")?;
        }

        Ok(())
    }

    pub fn credentials(&self) -> (&str, &str) {
        (&self.username, &self.password)
    }

    pub async fn describe(&self) -> DemoStatus {
        use std::collections::HashMap;

        let plan = self.plan.lock().await.clone();

        let registered = demo::context()
            .map(|ctx| ctx.libraries())
            .unwrap_or_default();

        let mut id_by_root: HashMap<PathBuf, LibraryId> = HashMap::new();
        let mut id_by_name: HashMap<String, LibraryId> = HashMap::new();
        for (id, meta) in registered {
            id_by_root.insert(meta.root.clone(), id);
            id_by_name.insert(meta.name.clone(), id);
        }

        DemoStatus {
            root: self.root.clone(),
            libraries: plan
                .libraries
                .iter()
                .map(|lib| {
                    let planned_primary_item_count = lib
                        .directories
                        .iter()
                        .filter(|dir| {
                            dir.parent() == Some(lib.root_path.as_path())
                                && dir != &&lib.root_path
                        })
                        .count();
                    let primary_item_count =
                        primary_item_paths_on_disk(lib.library_type, &lib.root_path)
                            .map(|items| items.len())
                            .unwrap_or(planned_primary_item_count);

                    let library_id = id_by_root
                        .get(&lib.root_path)
                        .copied()
                        .or_else(|| id_by_name.get(&lib.name).copied())
                        .unwrap_or_else(|| {
                            warn!(
                                "demo library {} not found in registered context; falling back to synthetic id",
                                lib.name
                            );
                            LibraryId::new()
                        });

                    DemoLibraryStatus {
                        library_id,
                        name: lib.name.clone(),
                        library_type: lib.library_type,
                        root: lib.root_path.clone(),
                        primary_item_count,
                        file_count: lib.files.len(),
                        directory_count: lib.directories.len(),
                    }
                })
                .collect(),
            username: self.username.clone(),
        }
    }

    pub async fn ensure_demo_user(&self, state: &AppState) -> Result<()> {
        let service = UserService::new(state);
        service
            .ensure_admin_role_exists()
            .await
            .context("failed to ensure admin role exists")?;

        if state
            .unit_of_work()
            .users
            .get_user_by_username(&self.username)
            .await
            .context("failed to check demo user existence")?
            .is_some()
        {
            return Ok(());
        }

        let user = service
            .create_user(CreateUserParams {
                username: self.username.clone(),
                display_name: "Demo User".into(),
                password: self.password.clone(),
                email: None,
                avatar_url: None,
                role_ids: Vec::new(),
                is_active: true,
                created_by: None,
            })
            .await
            .context("failed to create demo user")?;

        let roles = state
            .unit_of_work()
            .rbac
            .get_all_roles()
            .await
            .context("failed to list roles for demo user")?;

        if let Some(admin_role) =
            roles.into_iter().find(|role| role.name == roles::ADMIN)
        {
            service
                .assign_role(user.id, admin_role.id, user.id)
                .await
                .context("failed to assign admin role to demo user")?;
        }

        Ok(())
    }
}

fn prefix_for_like(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn resolve_root(options: &DemoSeedOptions, config: &Config) -> PathBuf {
    if let Some(explicit) = &options.root {
        return explicit.clone();
    }
    config.cache_root().join("demo-media")
}

fn env_nonempty_trimmed(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .and_then(|value| normalize_env_override(&value))
}

fn normalize_env_override(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ensure_demo_root_clean(
    root: &Path,
    plan: &demo::DemoSeedPlan,
) -> Result<()> {
    for library in &plan.libraries {
        if !library.root_path.starts_with(root) {
            return Err(anyhow!(
                "refusing to clean demo library root outside demo root: {}",
                library.root_path.display()
            ));
        }

        if let Err(err) = fs::remove_dir_all(&library.root_path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(anyhow!(err).context(format!(
                "failed to remove demo library directory {}",
                library.root_path.display()
            )));
        }
    }

    Ok(())
}

fn absolutize_demo_root(root: PathBuf) -> Result<PathBuf> {
    if root.is_absolute() {
        return Ok(root);
    }

    let cwd = std::env::current_dir()
        .context("failed to resolve current working directory")?;
    Ok(cwd.join(root))
}

#[cfg(test)]
mod tests {
    use super::normalize_env_override;

    #[test]
    fn normalize_env_override_treats_blank_as_unset() {
        assert_eq!(normalize_env_override(""), None);
        assert_eq!(normalize_env_override("   "), None);
    }

    #[test]
    fn normalize_env_override_returns_trimmed_value() {
        assert_eq!(
            normalize_env_override("  demodemo  "),
            Some("demodemo".to_string())
        );
    }
}
