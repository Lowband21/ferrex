#![cfg(feature = "demo")]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ferrex_core::domain::demo::{
    DemoLibraryPlan, DemoSeedOptions, DemoSeedPlan,
};
use ferrex_core::types::library::LibraryType;
use ferrex_server::db::{DEMO_DATABASE_NAME, derive_demo_database_url};
use ferrex_server::demo::{DemoCoordinator, DemoPlanProvider};
use ferrex_server::infra::startup::NoopStartupHooks;
use sqlx::PgPool;

use crate::common::build_test_app_with_hooks;

mod common;

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
    async fn generate_plan(
        &self,
        root: &Path,
        _options: &DemoSeedOptions,
    ) -> Result<DemoSeedPlan> {
        let template = {
            let mut guard = self.queue.lock().expect("lock plan queue");
            guard.pop_front()
        };

        let template = template
            .ok_or_else(|| anyhow!("No demo plans left in provider"))?;

        let libraries = template
            .libraries
            .into_iter()
            .map(|lib| {
                let library_root = root.join(&lib.relative_root);
                let mut directories = vec![library_root.clone()];
                directories.extend(
                    lib.directories.iter().map(|rel| library_root.join(rel)),
                );

                let files = lib
                    .files
                    .iter()
                    .map(|rel| library_root.join(rel))
                    .collect();

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
async fn demo_reset_preserves_libraries_and_cleans_files(
    pool: PgPool,
) -> Result<()> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (_router, state, tempdir) = app.into_parts();
    assert!(
        tempdir.path().join("cache").exists(),
        "test app should create cache directory structure"
    );

    let plan_provider: Arc<dyn DemoPlanProvider> =
        Arc::new(QueuePlanProvider::new(demo_plan_sequences()));

    let options = DemoSeedOptions {
        allow_zero_length_files: true,
        ..DemoSeedOptions::default()
    };

    let mut config = state.config().clone();
    let coordinator = DemoCoordinator::bootstrap_with_provider(
        &mut config,
        options.clone(),
        plan_provider,
    )
    .await?;

    let initial_ids = coordinator
        .sync_database(state.unit_of_work())
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
        .reset(state.unit_of_work(), None)
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

    let policy =
        ferrex_core::domain::demo::policy().expect("demo policy initialised");
    assert!(
        policy.allow_zero_length_files,
        "demo policy should allow zero-length media"
    );

    Ok(())
}

#[test]
fn derive_demo_database_url_rewrites_database_name() -> Result<()> {
    let base_url = "postgresql://user:pass@localhost:5432/ferrex";
    let demo_url = derive_demo_database_url(base_url)?;
    assert!(
        demo_url.ends_with(&format!("/{DEMO_DATABASE_NAME}")),
        "demo url should end with reserved demo database name"
    );
    Ok(())
}
