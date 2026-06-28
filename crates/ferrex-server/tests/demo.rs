#![cfg(feature = "demo")]

use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use axum::Router;
use axum_test::TestServer;
use ferrex_core::{
    api::{routes, types::collections::*},
    domain::demo::{DemoLibraryPlan, DemoSeedOptions, DemoSeedPlan},
    types::{LibraryId, library::LibraryType},
};
use ferrex_server::db::{DEMO_DATABASE_NAME, derive_demo_database_url};
use ferrex_server::{
    demo::DemoPlanProvider,
    infra::{app_state::AppState, startup::NoopStartupHooks},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::build_test_demo_app_with_hooks;

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

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn build_server(
    router: Router<AppState>,
    state: AppState,
) -> Result<TestServer> {
    let router: Router<()> = router.with_state(state);
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow!(err.to_string()))
}

async fn register_user(
    server: &TestServer,
    username: &str,
) -> Result<(Uuid, String)> {
    let response = server
        .post(routes::v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": format!("{username} display"),
            "password": "Password#123"
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    let user_id = Uuid::parse_str(
        body["data"]["user_id"]
            .as_str()
            .expect("register returns user_id"),
    )?;
    let access_token = body["data"]["access_token"]
        .as_str()
        .expect("register returns access token")
        .to_string();

    Ok((user_id, access_token))
}

fn demo_scoped_collection_request(
    title: &str,
    owner_user_id: Uuid,
    library_id: LibraryId,
) -> CreateCollectionRequest {
    CreateCollectionRequest {
        title: title.to_string(),
        description: None,
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: CollectionOwner {
            owner_type: CollectionOwnerType::User,
            user_id: Some(owner_user_id),
            device_id: None,
            display_name: Some(format!("owner-{owner_user_id}")),
        },
        scope: CollectionScope::Library,
        visibility: CollectionVisibility::Public,
        presentation: CollectionPresentationMode::Shelf,
        media_scope: CollectionMediaScope::Library {
            library_id,
            media_types: Vec::new(),
        },
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: Default::default(),
        theme: Default::default(),
        provenance: None,
        rule: None,
    }
}

async fn seed_external_movie_library(pool: &PgPool) -> Result<LibraryId> {
    let library_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")?;
    sqlx::query(
        r#"
        INSERT INTO libraries (
            id, name, library_type, paths, scan_interval_minutes, enabled,
            auto_scan, watch_for_changes, analyze_on_scan, max_retry_attempts
        ) VALUES ($1, 'External Demo Filter Movies', 'movies', ARRAY['/fixture/external-demo-filter'], 60, TRUE, TRUE, TRUE, FALSE, 3)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(library_id)
    .execute(pool)
    .await?;
    Ok(LibraryId(library_id))
}

async fn seed_shelf_placement(
    pool: &PgPool,
    collection: &CollectionDetail,
    shelf_key: &str,
    position: u32,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO collection_shelf_placements (
            id, collection_id, collection_stable_key, surface, shelf_key,
            placement_scope, placement_scope_key, visibility, presentation,
            pinned, position, position_key
        ) VALUES (
            $1, $2, $3, 'home', $4, 'global', 'global',
            'public', 'shelf', TRUE, $5, ($5::text)::numeric
        )
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(collection.summary.identity.id.to_uuid())
    .bind(&collection.summary.identity.stable_key)
    .bind(shelf_key)
    .bind(i32::try_from(position)?)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn demo_reset_preserves_libraries_and_cleans_files(
    pool: PgPool,
) -> Result<()> {
    let plan_provider: Arc<dyn DemoPlanProvider> =
        Arc::new(QueuePlanProvider::new(demo_plan_sequences()));

    let options = DemoSeedOptions {
        allow_zero_length_files: true,
        ..DemoSeedOptions::default()
    };

    let app = build_test_demo_app_with_hooks(
        pool.clone(),
        &NoopStartupHooks,
        options,
        plan_provider,
    )
    .await?;
    let (router, state, tempdir) = app.into_parts();
    assert!(
        tempdir.path().join("cache").exists(),
        "test app should create cache directory structure"
    );

    let coordinator = state.demo().expect("demo coordinator is installed");
    let initial_ids = coordinator.library_ids().await;
    assert_eq!(initial_ids.len(), 1);
    let initial_id = initial_ids[0];

    let ids_via_accessor = coordinator.library_ids().await;
    assert_eq!(ids_via_accessor, initial_ids);

    let server = build_server(router, state.clone()).await?;
    let (viewer_id, viewer_token) =
        register_user(&server, "demo_shelf_viewer").await?;
    let external_library_id = seed_external_movie_library(&pool).await?;
    let demo_collection = state
        .unit_of_work()
        .collections
        .create_collection(demo_scoped_collection_request(
            "Demo Library Shelf",
            viewer_id,
            initial_id,
        ))
        .await?;
    seed_shelf_placement(&pool, &demo_collection, "home:demo", 0).await?;
    let external_collection = state
        .unit_of_work()
        .collections
        .create_collection(demo_scoped_collection_request(
            "External Library Shelf",
            viewer_id,
            external_library_id,
        ))
        .await?;
    seed_shelf_placement(&pool, &external_collection, "home:external", 1)
        .await?;

    let demo_shelves = server
        .get(routes::v1::shelves::PLACEMENTS)
        .add_header("Authorization", bearer(&viewer_token))
        .await;
    demo_shelves.assert_status_ok();
    let body: Value = demo_shelves.json();
    let placements = body["data"]["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(
        placements[0]["collection_id"].as_str().unwrap(),
        demo_collection.summary.identity.id.to_string()
    );

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
