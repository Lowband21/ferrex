use std::path::PathBuf;
use std::sync::Arc;

use ferrex_core::database::PostgresDatabase;
use ferrex_core::database::traits::MediaDatabaseTrait;
use ferrex_core::providers::TmdbApiProvider;
use ferrex_core::scanner::{
    FolderMonitor, FolderMonitorConfig, InMemoryFs, TmdbFolderGenerator, apply_plan_to_inmemory_fs,
};
use ferrex_core::scanner::{GeneratedNode, StructurePlan};
use ferrex_core::{Library, LibraryID, LibraryType};
use tokio::sync::RwLock;

fn dir_count(plan: &StructurePlan) -> usize {
    plan.nodes
        .iter()
        .filter(|n| matches!(n, GeneratedNode::Dir(_)))
        .count()
}

fn make_library(name: &str, library_type: LibraryType, root: PathBuf) -> Library {
    let now = chrono::Utc::now();
    Library {
        id: LibraryID::new_uuid(),
        name: name.to_string(),
        library_type,
        paths: vec![root],
        scan_interval_minutes: 60,
        last_scan: None,
        enabled: true,
        auto_scan: false,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        created_at: now,
        updated_at: now,
        media: None,
    }
}

#[tokio::test]
async fn tmdb_generated_folders_are_discovered() -> anyhow::Result<()> {
    // Skip if no TMDB API key is provided
    let tmdb_key = std::env::var("TMDB_API_KEY").unwrap_or_default();
    if tmdb_key.is_empty() {
        eprintln!("Skipping tmdb_generated_folders_are_discovered: TMDB_API_KEY not set");
        return Ok(());
    }

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost/ferrex".to_string());

    // Real Postgres backend
    let db = Arc::new(PostgresDatabase::new(&database_url).await?);
    // Ensure schema is initialized
    let _ = db.initialize_schema().await; // ignore if already migrated

    // TMDB-backed generator
    let tmdb = Arc::new(TmdbApiProvider::new());
    let generator = TmdbFolderGenerator::new(tmdb);

    // 1) Generate movie structure plan (5 entries)
    let movies_root = PathBuf::from("/mem/movies");
    let movies_plan = generator
        .generate_movies(&movies_root, 5, Some("en-US"), Some("US"))
        .await?;
    let movies_expected_dirs = dir_count(&movies_plan);

    // 2) Generate series structure plan (2 series, 1-2 seasons, 3-4 episodes)
    let series_root = PathBuf::from("/mem/series");
    let series_plan = generator
        .generate_series(&series_root, 2, Some("en-US"), 1..=2, 3..=4)
        .await?;
    let series_expected_dirs = dir_count(&series_plan);

    // 3) Apply plans to in-memory filesystem
    let mut fs = InMemoryFs::new();
    apply_plan_to_inmemory_fs(&mut fs, &movies_plan);
    apply_plan_to_inmemory_fs(&mut fs, &series_plan);

    // 4) Create libraries and register in database
    let mut movies_lib = make_library("Test Movies", LibraryType::Movies, movies_root.clone());
    let mut series_lib = make_library("Test Series", LibraryType::Series, series_root.clone());

    // Persist libraries to DB so FolderMonitor can find them
    db.create_library(movies_lib.clone()).await?;
    db.create_library(series_lib.clone()).await?;

    let libraries = Arc::new(RwLock::new(vec![movies_lib.clone(), series_lib.clone()]));

    // 5) Run FolderMonitor discovery with the in-memory FS
    let monitor = Arc::new(FolderMonitor::new_with_fs(
        db.clone() as Arc<dyn MediaDatabaseTrait>,
        libraries.clone(),
        FolderMonitorConfig::default(),
        Arc::new(fs),
    ));

    // Discover folders for each library
    monitor
        .discover_library_folders_immediate(&movies_lib.id)
        .await?;
    monitor
        .discover_library_folders_immediate(&series_lib.id)
        .await?;

    // 6) Verify folder inventory counts match expected directory counts
    let movies_inventory = db.get_folder_inventory(movies_lib.id).await?;
    let series_inventory = db.get_folder_inventory(series_lib.id).await?;

    assert_eq!(
        movies_inventory.len(),
        movies_expected_dirs,
        "Movies folder inventory count should match generated directory count"
    );
    assert_eq!(
        series_inventory.len(),
        series_expected_dirs,
        "Series folder inventory count should match generated directory count"
    );

    Ok(())
}
