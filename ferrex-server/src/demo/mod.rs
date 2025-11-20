#![cfg(feature = "demo")]

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ferrex_core::application::unit_of_work::AppUnitOfWork;
use ferrex_core::demo::{self, DemoSeedOptions};
use ferrex_core::providers::TmdbApiProvider;
use ferrex_core::rbac::roles;
use ferrex_core::types::LibraryID;
use ferrex_core::types::library::LibraryType;
use sqlx::{Connection, PgConnection};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::infra::app_state::AppState;
use crate::infra::config::Config;
use crate::users::UserService;
use crate::users::user_service::CreateUserParams;

#[async_trait]
pub trait DemoPlanProvider: Send + Sync {
    async fn generate_plan(
        &self,
        root: &Path,
        options: &DemoSeedOptions,
    ) -> Result<demo::DemoSeedPlan>;
}

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
}

pub struct DemoCoordinator {
    options: DemoSeedOptions,
    plan: Mutex<demo::DemoSeedPlan>,
    root: PathBuf,
    pub username: String,
    pub password: String,
    library_ids: Mutex<Vec<LibraryID>>,
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
    pub async fn bootstrap(config: &mut Config, tmdb: Arc<TmdbApiProvider>) -> Result<Self> {
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
        let root = resolve_root(&options, config);

        std::fs::create_dir_all(&root).context("failed to create demo root directory")?;

        let plan = plan_provider
            .generate_plan(&root, &options)
            .await
            .context("failed to plan demo structure")?;
        demo::prepare_plan_roots(None, &plan)
            .context("failed to prepare demo filesystem for bootstrap")?;
        demo::apply_plan(&plan).context("failed to materialise demo file tree")?;

        // Ensure server points at the demo root
        config.media_root = Some(root.clone());

        // Initialise shared context for downstream components
        demo::init_demo_context(root.clone(), options.policy())?;

        let username = std::env::var("FERREX_DEMO_USERNAME").unwrap_or_else(|_| "demo".into());
        let password = std::env::var("FERREX_DEMO_PASSWORD").unwrap_or_else(|_| "demo".into());

        Ok(Self {
            options,
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

    pub async fn library_ids(&self) -> Vec<LibraryID> {
        self.library_ids.lock().await.clone()
    }

    pub async fn sync_database(&self, unit_of_work: Arc<AppUnitOfWork>) -> Result<Vec<LibraryID>> {
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
            if let Some(existing) = existing.iter().find(|lib| lib.name == plan.name) {
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

    pub async fn reset(&self, unit_of_work: Arc<AppUnitOfWork>) -> Result<()> {
        let previous_plan = {
            let guard = self.plan.lock().await;
            guard.clone()
        };

        let new_plan = self
            .plan_provider
            .generate_plan(&self.root, &self.options)
            .await
            .context("failed to regenerate demo plan")?;

        demo::prepare_plan_roots(Some(&previous_plan), &new_plan)
            .context("failed to prepare demo filesystem for reset")?;
        demo::apply_plan(&new_plan).context("failed to reapply demo folder structure")?;

        {
            let mut guard = self.plan.lock().await;
            *guard = new_plan;
        }

        self.sync_database(unit_of_work).await.map(|_| ())
    }

    pub fn credentials(&self) -> (&str, &str) {
        (&self.username, &self.password)
    }

    pub async fn describe(&self) -> DemoStatus {
        let plan = self.plan.lock().await.clone();
        DemoStatus {
            root: self.root.clone(),
            libraries: plan
                .libraries
                .iter()
                .map(|lib| DemoLibraryStatus {
                    name: lib.name.clone(),
                    library_type: lib.library_type,
                    root: lib.root_path.clone(),
                    file_count: lib.files.len(),
                    directory_count: lib.directories.len(),
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

        if service
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
            .unit_of_work
            .rbac
            .get_all_roles()
            .await
            .context("failed to list roles for demo user")?;

        if let Some(admin_role) = roles.into_iter().find(|role| role.name == roles::ADMIN) {
            service
                .assign_role(user.id, admin_role.id, user.id)
                .await
                .context("failed to assign admin role to demo user")?;
        }

        Ok(())
    }
}

fn resolve_root(options: &DemoSeedOptions, config: &Config) -> PathBuf {
    if let Some(explicit) = &options.root {
        return explicit.clone();
    }
    config.cache_dir.join("demo-media")
}

/// Lightweight status payload surfaced via the admin API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DemoStatus {
    pub root: PathBuf,
    pub libraries: Vec<DemoLibraryStatus>,
    pub username: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DemoLibraryStatus {
    pub name: String,
    pub library_type: LibraryType,
    pub root: PathBuf,
    pub file_count: usize,
    pub directory_count: usize,
}

impl DemoStatus {
    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }
}

pub fn derive_demo_database_url(base: &str) -> Result<String> {
    let mut url = Url::parse(base).context("invalid PostgreSQL URL")?;
    let current_path = url.path().trim_start_matches('/');
    if current_path.is_empty() {
        return Err(anyhow!("database URL must include database name"));
    }
    if current_path.ends_with("_demo") {
        return Ok(base.to_string());
    }
    let new_name = format!("{}_demo", current_path);
    url.set_path(&format!("/{}", new_name));
    Ok(url.into())
}

/// Drop and recreate the demo database derived from the provided base URL before use.
///
/// Ensures every demo boot starts from a clean database while leaving the user's
/// primary database untouched.
pub async fn prepare_demo_database(base: &str) -> Result<String> {
    let base_url = Url::parse(base).context("invalid PostgreSQL URL")?;
    let base_name = base_url.path().trim_start_matches('/');
    if base_name.is_empty() {
        return Err(anyhow!("database URL must include database name"));
    }
    if base_name.ends_with("_demo") {
        return Err(anyhow!(
            "Refusing to prepare demo database because the base URL already points at a demo database"
        ));
    }

    let demo_url = derive_demo_database_url(base)?;
    let demo_name = Url::parse(&demo_url)
        .context("invalid derived demo database URL")?
        .path()
        .trim_start_matches('/')
        .to_string();

    let mut admin_url = base_url.clone();
    admin_url.set_path("/postgres");
    let admin_url = admin_url.into_string();

    let mut connection = PgConnection::connect(&admin_url)
        .await
        .with_context(|| format!("failed to connect to admin database via {}", admin_url))?;

    sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1")
        .bind(&demo_name)
        .execute(&mut connection)
        .await
        .context("failed to terminate active demo connections")?;

    let quoted_name = quote_ident(&demo_name);
    let drop_stmt = format!("DROP DATABASE IF EXISTS {}", quoted_name);
    sqlx::query(&drop_stmt)
        .execute(&mut connection)
        .await
        .context("failed to drop existing demo database")?;

    let create_stmt = format!("CREATE DATABASE {}", quoted_name);
    sqlx::query(&create_stmt)
        .execute(&mut connection)
        .await
        .context("failed to create demo database")?;

    Ok(demo_url)
}

fn quote_ident(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}
