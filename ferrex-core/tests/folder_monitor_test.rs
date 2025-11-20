use ferrex_core::{
    scanner::folder_monitor::{FolderMonitor, FolderMonitorConfig},
    database::{PostgresDatabase, traits::*},
    Library, LibraryType, MediaError, Result,
};
use uuid::Uuid;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use sqlx::postgres::PgPoolOptions;
use tempfile::TempDir;
use std::fs;
use std::path::PathBuf;

async fn setup_test_db() -> Result<Arc<dyn MediaDatabaseTrait>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost/ferrex".to_string());
    
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to connect to database: {}", e)))?;
    
    Ok(Arc::new(PostgresDatabase::from_pool(pool)) as Arc<dyn MediaDatabaseTrait>)
}

async fn create_test_library(db: &Arc<dyn MediaDatabaseTrait>, name: &str, library_type: LibraryType, paths: Vec<PathBuf>) -> Result<Library> {
    let library_id = Uuid::new_v4();
    let library_name = format!("{} {}", name, library_id);
    
    // Cast to PostgresDatabase to access pool
    let postgres_db = db.as_any()
        .downcast_ref::<PostgresDatabase>()
        .ok_or_else(|| MediaError::Internal("Failed to downcast to PostgresDatabase".to_string()))?;
    
    let paths_strings: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let library_type_str = match library_type {
        LibraryType::Movies => "movies",
        LibraryType::TvShows => "tvshows",
    };
    
    sqlx::query!(
        "INSERT INTO libraries (id, name, paths, library_type, enabled, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, true, NOW(), NOW())",
        library_id,
        library_name,
        &paths_strings,
        library_type_str
    )
    .execute(postgres_db.pool())
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to create test library: {}", e)))?;
    
    Ok(Library {
        id: library_id,
        name: library_name,
        paths,
        library_type,
        enabled: true,
        scan_interval_minutes: 60,
        last_scan: None,
        auto_scan: true,
        watch_for_changes: false,
        analyze_on_scan: true,
        max_retry_attempts: 3,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
    })
}

async fn cleanup_library(db: &Arc<dyn MediaDatabaseTrait>, library_id: Uuid) -> Result<()> {
    let postgres_db = db.as_any()
        .downcast_ref::<PostgresDatabase>()
        .ok_or_else(|| MediaError::Internal("Failed to downcast to PostgresDatabase".to_string()))?;
    
    sqlx::query!("DELETE FROM libraries WHERE id = $1", library_id)
        .execute(postgres_db.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to cleanup library: {}", e)))?;
    Ok(())
}

fn setup_test_movie_directory(temp_dir: &TempDir) -> PathBuf {
    let library_path = temp_dir.path().join("movies");
    
    // Create movie directory structure
    fs::create_dir_all(&library_path).unwrap();
    
    // Create some movie folders with video files
    let movie1 = library_path.join("The Matrix (1999)");
    fs::create_dir_all(&movie1).unwrap();
    fs::write(movie1.join("The.Matrix.1999.1080p.BluRay.mp4"), b"fake video content").unwrap();
    fs::write(movie1.join("The.Matrix.1999.1080p.BluRay.srt"), b"fake subtitle").unwrap();
    
    let movie2 = library_path.join("Inception (2010)");
    fs::create_dir_all(&movie2).unwrap();
    fs::write(movie2.join("Inception.2010.1080p.mkv"), b"fake video content").unwrap();
    
    // Create extras folder
    let extras = movie2.join("Extras");
    fs::create_dir_all(&extras).unwrap();
    fs::write(extras.join("Behind.The.Scenes.mp4"), b"fake video content").unwrap();
    
    // Create empty folder (should be ignored)
    let empty = library_path.join("Empty Folder");
    fs::create_dir_all(&empty).unwrap();
    
    library_path
}

fn setup_test_tv_directory(temp_dir: &TempDir) -> PathBuf {
    let library_path = temp_dir.path().join("tv_shows");
    
    // Create TV show directory structure
    fs::create_dir_all(&library_path).unwrap();
    
    // Create TV show with seasons
    let show1 = library_path.join("Breaking Bad");
    fs::create_dir_all(&show1).unwrap();
    
    let season1 = show1.join("Season 1");
    fs::create_dir_all(&season1).unwrap();
    fs::write(season1.join("Breaking.Bad.S01E01.Pilot.mkv"), b"fake video").unwrap();
    fs::write(season1.join("Breaking.Bad.S01E02.mkv"), b"fake video").unwrap();
    
    let season2 = show1.join("Season 2");
    fs::create_dir_all(&season2).unwrap();
    fs::write(season2.join("Breaking.Bad.S02E01.mkv"), b"fake video").unwrap();
    
    // Create show with specials
    let show2 = library_path.join("Doctor Who");
    fs::create_dir_all(&show2).unwrap();
    
    let specials = show2.join("Specials");
    fs::create_dir_all(&specials).unwrap();
    fs::write(specials.join("Doctor.Who.S00E01.Christmas.Special.mp4"), b"fake video").unwrap();
    
    library_path
}

#[tokio::test]
async fn test_folder_monitor_creation() -> Result<()> {
    let db = setup_test_db().await?;
    let libraries = Arc::new(RwLock::new(Vec::new()));
    let config = FolderMonitorConfig::default();
    
    let monitor = FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config.clone(),
    );
    
    // Test that monitor was created with correct configuration
    assert_eq!(config.scan_interval_secs, 300);
    assert_eq!(config.max_retry_attempts, 3);
    assert_eq!(config.stale_folder_hours, 24);
    assert_eq!(config.batch_size, 100);
    
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_movie_inventory() -> Result<()> {
    let db = setup_test_db().await?;
    let temp_dir = TempDir::new().unwrap();
    let library_path = setup_test_movie_directory(&temp_dir);
    
    let library = create_test_library(&db, "Test Movies", LibraryType::Movies, vec![library_path.clone()]).await?;
    let libraries = Arc::new(RwLock::new(vec![library.clone()]));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 1, // Fast for testing
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 100,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Wait for scan cycle to complete
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Stop the monitor
    monitor.stop().await;
    
    // Verify folders were inventoried
    let inventory = db.get_folder_inventory(library.id).await?;
    
    // Should have root folder, 2 movie folders, and 1 extras folder
    assert!(inventory.len() >= 3, "Expected at least 3 folders, got {}", inventory.len());
    
    // Check for specific folders
    let has_matrix = inventory.iter().any(|f| f.folder_path.contains("The Matrix"));
    let has_inception = inventory.iter().any(|f| f.folder_path.contains("Inception"));
    let has_extras = inventory.iter().any(|f| f.folder_path.contains("Extras"));
    
    assert!(has_matrix, "Matrix folder not found");
    assert!(has_inception, "Inception folder not found");
    assert!(has_extras, "Extras folder not found");
    
    // Check folder types
    let movie_folders: Vec<_> = inventory.iter()
        .filter(|f| f.folder_type == FolderType::Movie)
        .collect();
    assert!(movie_folders.len() >= 2, "Expected at least 2 movie folders");
    
    // Check file counts
    let matrix_folder = inventory.iter()
        .find(|f| f.folder_path.contains("The Matrix"))
        .expect("Matrix folder should exist");
    assert_eq!(matrix_folder.total_files, 1, "Matrix folder should have 1 video file");
    
    cleanup_library(&db, library.id).await?;
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_tv_inventory() -> Result<()> {
    let db = setup_test_db().await?;
    let temp_dir = TempDir::new().unwrap();
    let library_path = setup_test_tv_directory(&temp_dir);
    
    let library = create_test_library(&db, "Test TV Shows", LibraryType::TvShows, vec![library_path.clone()]).await?;
    let libraries = Arc::new(RwLock::new(vec![library.clone()]));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 1, // Fast for testing
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 100,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Wait for scan cycle to complete
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Stop the monitor
    monitor.stop().await;
    
    // Verify folders were inventoried
    let inventory = db.get_folder_inventory(library.id).await?;
    
    // Should have root, show folders, season folders
    assert!(inventory.len() >= 5, "Expected at least 5 folders, got {}", inventory.len());
    
    // Check for TV show folders
    let has_breaking_bad = inventory.iter().any(|f| f.folder_path.contains("Breaking Bad"));
    let has_doctor_who = inventory.iter().any(|f| f.folder_path.contains("Doctor Who"));
    
    assert!(has_breaking_bad, "Breaking Bad folder not found");
    assert!(has_doctor_who, "Doctor Who folder not found");
    
    // Check for season folders
    let season_folders: Vec<_> = inventory.iter()
        .filter(|f| f.folder_type == FolderType::Season)
        .collect();
    assert!(season_folders.len() >= 2, "Expected at least 2 season folders");
    
    // Check TV show folder types
    let tv_show_folders: Vec<_> = inventory.iter()
        .filter(|f| f.folder_type == FolderType::TvShow)
        .collect();
    assert!(tv_show_folders.len() >= 2, "Expected at least 2 TV show folders");
    
    // Verify season folder has correct file count
    let season1_folder = inventory.iter()
        .find(|f| f.folder_path.contains("Season 1"))
        .expect("Season 1 folder should exist");
    assert_eq!(season1_folder.total_files, 2, "Season 1 should have 2 video files");
    
    cleanup_library(&db, library.id).await?;
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_processing_cycle() -> Result<()> {
    let db = setup_test_db().await?;
    let temp_dir = TempDir::new().unwrap();
    let library_path = setup_test_movie_directory(&temp_dir);
    
    let library = create_test_library(&db, "Test Processing", LibraryType::Movies, vec![library_path.clone()]).await?;
    let libraries = Arc::new(RwLock::new(vec![library.clone()]));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 1, // Fast for testing
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 10,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Wait for two scan cycles to allow processing
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Stop the monitor
    monitor.stop().await;
    
    // Check that folders were processed
    let filters = FolderScanFilters {
        library_id: Some(library.id),
        processing_status: Some(FolderProcessingStatus::Completed),
        folder_type: None,
        max_attempts: None,
        stale_after_hours: None,
        limit: None,
        priority: None,
        max_batch_size: None,
        error_retry_threshold: None,
    };
    
    let completed_folders = db.get_folders_needing_scan(&filters).await?;
    
    // Should have some completed folders after processing
    // Note: get_folders_needing_scan might filter out completed folders
    // So let's check the inventory directly
    let inventory = db.get_folder_inventory(library.id).await?;
    let completed_count = inventory.iter()
        .filter(|f| f.processing_status == FolderProcessingStatus::Completed)
        .count();
    
    assert!(completed_count > 0, "Expected some completed folders after processing");
    
    cleanup_library(&db, library.id).await?;
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_multiple_libraries() -> Result<()> {
    let db = setup_test_db().await?;
    let temp_dir = TempDir::new().unwrap();
    
    // Setup both movie and TV directories
    let movie_path = setup_test_movie_directory(&temp_dir);
    let tv_path = setup_test_tv_directory(&temp_dir);
    
    let movie_library = create_test_library(&db, "Multi Movies", LibraryType::Movies, vec![movie_path]).await?;
    let tv_library = create_test_library(&db, "Multi TV", LibraryType::TvShows, vec![tv_path]).await?;
    
    let libraries = Arc::new(RwLock::new(vec![movie_library.clone(), tv_library.clone()]));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 1,
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 100,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Wait for scan cycle to complete
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Stop the monitor
    monitor.stop().await;
    
    // Verify both libraries were inventoried
    let movie_inventory = db.get_folder_inventory(movie_library.id).await?;
    let tv_inventory = db.get_folder_inventory(tv_library.id).await?;
    
    assert!(movie_inventory.len() >= 3, "Expected movie folders");
    assert!(tv_inventory.len() >= 5, "Expected TV folders");
    
    // Verify correct folder types for each library
    let movie_types: Vec<_> = movie_inventory.iter()
        .filter(|f| f.folder_type == FolderType::Movie)
        .collect();
    assert!(!movie_types.is_empty(), "Movie library should have movie folders");
    
    let tv_types: Vec<_> = tv_inventory.iter()
        .filter(|f| f.folder_type == FolderType::TvShow || f.folder_type == FolderType::Season)
        .collect();
    assert!(!tv_types.is_empty(), "TV library should have TV/Season folders");
    
    cleanup_library(&db, movie_library.id).await?;
    cleanup_library(&db, tv_library.id).await?;
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_disabled_library() -> Result<()> {
    let db = setup_test_db().await?;
    let temp_dir = TempDir::new().unwrap();
    let library_path = setup_test_movie_directory(&temp_dir);
    
    // Create an enabled and a disabled library
    let enabled_library = create_test_library(&db, "Enabled", LibraryType::Movies, vec![library_path.clone()]).await?;
    
    // Create disabled library manually
    let disabled_id = Uuid::new_v4();
    let disabled_name = format!("Disabled {}", disabled_id);
    let disabled_path = temp_dir.path().join("disabled");
    fs::create_dir_all(&disabled_path).unwrap();
    
    let postgres_db = db.as_any()
        .downcast_ref::<PostgresDatabase>()
        .ok_or_else(|| MediaError::Internal("Failed to downcast".to_string()))?;
    
    sqlx::query!(
        "INSERT INTO libraries (id, name, paths, library_type, enabled, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, false, NOW(), NOW())",
        disabled_id,
        disabled_name,
        &vec![disabled_path.to_string_lossy().to_string()],
        "movies"
    )
    .execute(postgres_db.pool())
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to create disabled library: {}", e)))?;
    
    let disabled_library = Library {
        id: disabled_id,
        name: disabled_name,
        paths: vec![disabled_path],
        library_type: LibraryType::Movies,
        enabled: false,
        scan_interval_minutes: 60,
        last_scan: None,
        auto_scan: false,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        media: None,
    };
    
    let libraries = Arc::new(RwLock::new(vec![enabled_library.clone(), disabled_library.clone()]));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 1,
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 100,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Wait for scan cycle to complete
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Stop the monitor
    monitor.stop().await;
    
    // Verify only enabled library was inventoried
    let enabled_inventory = db.get_folder_inventory(enabled_library.id).await?;
    let disabled_inventory = db.get_folder_inventory(disabled_library.id).await?;
    
    assert!(enabled_inventory.len() >= 3, "Enabled library should have folders");
    assert_eq!(disabled_inventory.len(), 0, "Disabled library should not have folders");
    
    cleanup_library(&db, enabled_library.id).await?;
    cleanup_library(&db, disabled_library.id).await?;
    Ok(())
}

#[tokio::test]
async fn test_folder_monitor_stop_signal() -> Result<()> {
    let db = setup_test_db().await?;
    let libraries = Arc::new(RwLock::new(Vec::new()));
    
    let config = FolderMonitorConfig {
        scan_interval_secs: 10, // Longer interval
        max_retry_attempts: 3,
        stale_folder_hours: 24,
        batch_size: 100,
        error_retry_threshold: 3,
    };
    
    let monitor = Arc::new(FolderMonitor::new(
        Arc::clone(&db),
        Arc::clone(&libraries),
        config,
    ));
    
    // Start the monitor
    monitor.clone().start().await?;
    
    // Stop immediately
    monitor.stop().await;
    
    // Give some time for the stop signal to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Test passes if no panic occurs
    Ok(())
}