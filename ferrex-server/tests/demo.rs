#![cfg(feature = "demo")]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ferrex_core::demo::{DemoLibraryPlan, DemoSeedOptions, DemoSeedPlan};
use ferrex_core::types::library::LibraryType;
use ferrex_server::db::{DEMO_DATABASE_NAME, prepare_demo_database};
use ferrex_server::demo::{DemoCoordinator, DemoPlanProvider};
use reqwest::Url;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[path = "support/mod.rs"]
mod support;
use support::build_test_app;

#[derive(Clone)]
struct LibraryTemplate {
    name: String,
    library_type: LibraryType,
    relative_root: PathBuf,
    directories: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

#[derive(Clone)]
struct PlanTemplate {
    libraries: Vec<LibraryTemplate>,
}

struct QueuePlanProvider {
    queue: Mutex<VecDeque<PlanTemplate>>,
}

impl QueuePlanProvider {
    fn new(plans: Vec<PlanTemplate>) -> Self {
        Self {
            queue: Mutex::new(plans.into()),
        }
    }
}

#[async_trait]
impl DemoPlanProvider for QueuePlanProvider {
    async fn generate_plan(&self, root: &Path, _options: &DemoSeedOptions) -> Result<DemoSeedPlan> {
        let template = {
            let mut guard = self.queue.lock().expect("lock plan queue");
            guard.pop_front()
        };

        let template = template.ok_or_else(|| anyhow!("No demo plans left in provider"))?;

        let libraries = template
            .libraries
            .into_iter()
            .map(|lib| {
                let library_root = root.join(&lib.relative_root);
                let mut directories = vec![library_root.clone()];
                directories.extend(lib.directories.iter().map(|rel| library_root.join(rel)));

                let files = lib.files.iter().map(|rel| library_root.join(rel)).collect();

                DemoLibraryPlan {
                    name: lib.name,
                    library_type: lib.library_type,
                    root_path: library_root,
                    directories,
                    files,
                }
            })
            .collect();

        Ok(DemoSeedPlan {
            root: root.to_path_buf(),
            libraries,
        })
    }
}

fn demo_plan_sequences() -> Vec<PlanTemplate> {
    vec![
        PlanTemplate {
            libraries: vec![LibraryTemplate {
                name: "Demo Movies".into(),
                library_type: LibraryType::Movies,
                relative_root: PathBuf::from("demo-movies"),
                directories: vec![PathBuf::from("First Feature")],
                files: vec![PathBuf::from("First Feature/feature.mkv")],
            }],
        },
        PlanTemplate {
            libraries: vec![LibraryTemplate {
                name: "Demo Movies".into(),
                library_type: LibraryType::Movies,
                relative_root: PathBuf::from("demo-movies"),
                directories: vec![PathBuf::from("Second Feature")],
                files: vec![PathBuf::from("Second Feature/feature.mkv")],
            }],
        },
    ]
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn demo_reset_preserves_libraries_and_cleans_files(pool: PgPool) -> Result<()> {
    let app = build_test_app(pool).await?;
    let (_router, state, tempdir) = app.into_parts();
    let _tempdir = tempdir; // keep alive

    let plan_provider: Arc<dyn DemoPlanProvider> =
        Arc::new(QueuePlanProvider::new(demo_plan_sequences()));

    let mut options = DemoSeedOptions::default();
    options.allow_zero_length_files = true;

    let mut config = state.config.as_ref().clone();
    let coordinator =
        DemoCoordinator::bootstrap_with_provider(&mut config, options.clone(), plan_provider)
            .await?;

    let initial_ids = coordinator
        .sync_database(state.unit_of_work.clone())
        .await
        .context("failed to sync demo libraries")?;
    assert_eq!(initial_ids.len(), 1);
    let initial_id = initial_ids[0];

    let ids_via_accessor = coordinator.library_ids().await;
    assert_eq!(ids_via_accessor, initial_ids);

    let demo_root = coordinator.root().to_path_buf();
    let first_file = demo_root
        .join("demo-movies")
        .join("First Feature")
        .join("feature.mkv");
    assert!(first_file.exists(), "initial demo file should exist");

    coordinator
        .reset(state.unit_of_work.clone(), None)
        .await
        .context("demo reset should succeed")?;

    let post_reset_ids = coordinator.library_ids().await;
    assert_eq!(
        post_reset_ids,
        vec![initial_id],
        "demo reset should retain library id"
    );

    assert!(
        !first_file.exists(),
        "stale demo files should be removed after reset"
    );

    let second_file = demo_root
        .join("demo-movies")
        .join("Second Feature")
        .join("feature.mkv");
    assert!(second_file.exists(), "new demo file should be created");
    assert_eq!(
        std::fs::metadata(&second_file)
            .context("read new demo file metadata")?
            .len(),
        0,
        "demo files remain zero-length to support fake filesystem"
    );

    let policy = ferrex_core::demo::policy().expect("demo policy initialised");
    assert!(
        policy.allow_zero_length_files,
        "demo policy should allow zero-length media"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn prepare_demo_database_recreates_database(_pool: PgPool) -> Result<()> {
    let raw_base_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL should be set for sqlx::test")?;
    let mut base_url = Url::parse(&raw_base_url).context("DATABASE_URL must be valid URL")?;
    let current_db = base_url.path().trim_start_matches('/');

    if current_db.is_empty() || current_db.eq_ignore_ascii_case(DEMO_DATABASE_NAME) {
        base_url.set_path("/ferrex_demo_test_primary");
    }

    let base_url: String = base_url.into();

    let demo_url = prepare_demo_database(&base_url).await?;

    let demo_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&demo_url)
        .await
        .context("connect to freshly created demo database")?;
    sqlx::query("CREATE TABLE demo_marker(id INT)")
        .execute(&demo_pool)
        .await
        .context("seed marker table in demo database")?;
    demo_pool.close().await;

    prepare_demo_database(&base_url).await?;

    let demo_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&demo_url)
        .await
        .context("reconnect to recreated demo database")?;
    let marker_exists = sqlx::query("SELECT 1 FROM pg_tables WHERE tablename = 'demo_marker'")
        .fetch_optional(&demo_pool)
        .await
        .context("probe demo database for marker table")?;
    demo_pool.close().await;

    assert!(
        marker_exists.is_none(),
        "demo database should be recreated from scratch"
    );

    Ok(())
}
