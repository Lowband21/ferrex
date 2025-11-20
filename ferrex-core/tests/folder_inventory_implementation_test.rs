use ferrex_core::{
    database::{PostgresDatabase, traits::*},
    MediaError, Result,
};
use uuid::Uuid;
use chrono::Utc;
use sqlx::postgres::PgPoolOptions;

async fn setup_test_db() -> Result<PostgresDatabase> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost/ferrex".to_string());
    
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to connect to database: {}", e)))?;
    
    Ok(PostgresDatabase::from_pool(pool))
}

async fn create_test_library(db: &PostgresDatabase) -> Result<Uuid> {
    let library_id = Uuid::new_v4();
    let library_name = format!("Test Library {}", library_id);
    sqlx::query!(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, NOW(), NOW())",
        library_id,
        library_name,
        &vec!["/test/library".to_string()],
        "movies"
    )
    .execute(db.pool())
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to create test library: {}", e)))?;
    
    Ok(library_id)
}

async fn cleanup_library(db: &PostgresDatabase, library_id: Uuid) -> Result<()> {
    sqlx::query!("DELETE FROM libraries WHERE id = $1", library_id)
        .execute(db.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to cleanup library: {}", e)))?;
    Ok(())
}

#[tokio::test]
async fn test_upsert_folder_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    // Create a test folder
    let folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/movies/action".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Pending,
        last_processed_at: None,
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 0,
        processed_files: 0,
        total_size_bytes: 0,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Test insert
    let folder_id = db.upsert_folder_impl(&folder).await?;
    assert_eq!(folder_id, folder.id);
    
    // Test update (upsert same path)
    let mut updated_folder = folder.clone();
    updated_folder.processing_status = FolderProcessingStatus::Completed;
    updated_folder.total_files = 10;
    
    let updated_id = db.upsert_folder_impl(&updated_folder).await?;
    assert_eq!(updated_id, folder.id);
    
    // Verify update worked
    let retrieved = db.get_folder_by_path_impl(library_id, "/test/movies/action").await?;
    assert!(retrieved.is_some());
    let retrieved_folder = retrieved.unwrap();
    assert_eq!(retrieved_folder.processing_status, FolderProcessingStatus::Completed);
    assert_eq!(retrieved_folder.total_files, 10);
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_get_folders_needing_scan_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    // Create folders with different statuses
    let folders = vec![
        FolderInventory {
            id: Uuid::new_v4(),
            library_id,
            folder_path: "/test/pending".to_string(),
            folder_type: FolderType::Movie,
            parent_folder_id: None,
            discovered_at: Utc::now(),
            last_seen_at: Utc::now(),
            discovery_source: FolderDiscoverySource::Scan,
            processing_status: FolderProcessingStatus::Pending,
            last_processed_at: None,
            processing_error: None,
            processing_attempts: 0,
            next_retry_at: None,
            total_files: 0,
            processed_files: 0,
            total_size_bytes: 0,
            file_types: vec![],
            last_modified: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
        FolderInventory {
            id: Uuid::new_v4(),
            library_id,
            folder_path: "/test/completed".to_string(),
            folder_type: FolderType::Movie,
            parent_folder_id: None,
            discovered_at: Utc::now(),
            last_seen_at: Utc::now(),
            discovery_source: FolderDiscoverySource::Scan,
            processing_status: FolderProcessingStatus::Completed,
            last_processed_at: Some(Utc::now()),
            processing_error: None,
            processing_attempts: 0,
            next_retry_at: None,
            total_files: 5,
            processed_files: 5,
            total_size_bytes: 1000000,
            file_types: vec!["mp4".to_string()],
            last_modified: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
        FolderInventory {
            id: Uuid::new_v4(),
            library_id,
            folder_path: "/test/failed".to_string(),
            folder_type: FolderType::Movie,
            parent_folder_id: None,
            discovered_at: Utc::now(),
            last_seen_at: Utc::now(),
            discovery_source: FolderDiscoverySource::Scan,
            processing_status: FolderProcessingStatus::Failed,
            last_processed_at: None,
            processing_error: Some("Test error".to_string()),
            processing_attempts: 1,
            next_retry_at: None, // Ready for retry
            total_files: 0,
            processed_files: 0,
            total_size_bytes: 0,
            file_types: vec![],
            last_modified: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    ];
    
    // Insert all folders
    for folder in &folders {
        db.upsert_folder_impl(folder).await?;
    }
    
    // Test filter by status
    let filters = FolderScanFilters {
        library_id: Some(library_id),
        processing_status: Some(FolderProcessingStatus::Pending),
        folder_type: None,
        max_attempts: None,
        stale_after_hours: None,
        limit: None,
        priority: None,
        max_batch_size: None,
        error_retry_threshold: None,
    };
    
    let pending_folders = db.get_folders_needing_scan_impl(&filters).await?;
    assert_eq!(pending_folders.len(), 1);
    assert_eq!(pending_folders[0].folder_path, "/test/pending");
    
    // Test filter by failed status with max attempts
    let filters = FolderScanFilters {
        library_id: Some(library_id),
        processing_status: Some(FolderProcessingStatus::Failed),
        folder_type: None,
        max_attempts: Some(5), // Allow up to 5 attempts
        stale_after_hours: None,
        limit: None,
        priority: None,
        max_batch_size: None,
        error_retry_threshold: None,
    };
    
    let failed_folders = db.get_folders_needing_scan_impl(&filters).await?;
    assert_eq!(failed_folders.len(), 1);
    assert_eq!(failed_folders[0].folder_path, "/test/failed");
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_update_folder_status_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    let folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/status".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Pending,
        last_processed_at: None,
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 0,
        processed_files: 0,
        total_size_bytes: 0,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    db.upsert_folder_impl(&folder).await?;
    
    // Update to processing
    db.update_folder_status_impl(folder.id, FolderProcessingStatus::Processing, None).await?;
    
    let updated = db.get_folder_by_path_impl(library_id, "/test/status").await?;
    assert!(updated.is_some());
    assert_eq!(updated.unwrap().processing_status, FolderProcessingStatus::Processing);
    
    // Update to completed
    db.update_folder_status_impl(folder.id, FolderProcessingStatus::Completed, None).await?;
    
    let completed = db.get_folder_by_path_impl(library_id, "/test/status").await?;
    assert!(completed.is_some());
    let completed_folder = completed.unwrap();
    assert_eq!(completed_folder.processing_status, FolderProcessingStatus::Completed);
    assert!(completed_folder.last_processed_at.is_some());
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_record_folder_scan_error_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    let folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/error".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Processing,
        last_processed_at: None,
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 0,
        processed_files: 0,
        total_size_bytes: 0,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    db.upsert_folder_impl(&folder).await?;
    
    // Record an error
    let next_retry = Utc::now() + chrono::Duration::hours(1);
    db.record_folder_scan_error_impl(folder.id, "Failed to access folder", Some(next_retry)).await?;
    
    let errored = db.get_folder_by_path_impl(library_id, "/test/error").await?;
    assert!(errored.is_some());
    let errored_folder = errored.unwrap();
    assert_eq!(errored_folder.processing_status, FolderProcessingStatus::Failed);
    assert_eq!(errored_folder.processing_error.as_deref(), Some("Failed to access folder"));
    assert_eq!(errored_folder.processing_attempts, 1);
    assert!(errored_folder.next_retry_at.is_some());
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_update_folder_stats_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    let folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/stats".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Processing,
        last_processed_at: None,
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 0,
        processed_files: 0,
        total_size_bytes: 0,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    db.upsert_folder_impl(&folder).await?;
    
    // Update stats
    let file_types = vec!["mp4".to_string(), "mkv".to_string(), "srt".to_string()];
    db.update_folder_stats_impl(folder.id, 15, 10, 5_000_000_000, file_types.clone()).await?;
    
    let updated = db.get_folder_by_path_impl(library_id, "/test/stats").await?;
    assert!(updated.is_some());
    let updated_folder = updated.unwrap();
    assert_eq!(updated_folder.total_files, 15);
    assert_eq!(updated_folder.processed_files, 10);
    assert_eq!(updated_folder.total_size_bytes, 5_000_000_000);
    assert_eq!(updated_folder.file_types, file_types);
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_cleanup_stale_folders_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    // Create a stale folder (last seen 3 days ago)
    let stale_folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/stale".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now() - chrono::Duration::days(7),
        last_seen_at: Utc::now() - chrono::Duration::days(3),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Completed,
        last_processed_at: Some(Utc::now() - chrono::Duration::days(3)),
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 5,
        processed_files: 5,
        total_size_bytes: 1000000,
        file_types: vec!["mp4".to_string()],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now() - chrono::Duration::days(7),
        updated_at: Utc::now() - chrono::Duration::days(3),
    };
    
    // Create a recent folder
    let recent_folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/recent".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Completed,
        last_processed_at: Some(Utc::now()),
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 10,
        processed_files: 10,
        total_size_bytes: 2000000,
        file_types: vec!["mkv".to_string()],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Insert both folders directly using SQL to avoid issues with timestamp manipulation
    sqlx::query!(
        r#"
        INSERT INTO folder_inventory (
            id, library_id, folder_path, folder_type, parent_folder_id,
            discovered_at, last_seen_at, discovery_source,
            processing_status, last_processed_at, processing_error,
            processing_attempts, next_retry_at,
            total_files, processed_files, total_size_bytes,
            file_types, last_modified, metadata,
            created_at, updated_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21
        )
        "#,
        stale_folder.id,
        stale_folder.library_id,
        stale_folder.folder_path,
        "movie",
        stale_folder.parent_folder_id,
        stale_folder.discovered_at,
        stale_folder.last_seen_at,
        "scan",
        "completed",
        stale_folder.last_processed_at,
        stale_folder.processing_error,
        stale_folder.processing_attempts,
        stale_folder.next_retry_at,
        stale_folder.total_files,
        stale_folder.processed_files,
        stale_folder.total_size_bytes,
        serde_json::to_value(&stale_folder.file_types).unwrap(),
        stale_folder.last_modified,
        stale_folder.metadata,
        stale_folder.created_at,
        stale_folder.updated_at
    )
    .execute(db.pool())
    .await
    .map_err(|e| MediaError::Internal(format!("Failed to insert stale folder: {}", e)))?;
    
    db.upsert_folder_impl(&recent_folder).await?;
    
    // Cleanup folders older than 48 hours
    let deleted_count = db.cleanup_stale_folders_impl(library_id, 48).await?;
    assert_eq!(deleted_count, 1);
    
    // Verify stale folder is gone
    let stale_check = db.get_folder_by_path_impl(library_id, "/test/stale").await?;
    assert!(stale_check.is_none());
    
    // Verify recent folder still exists
    let recent_check = db.get_folder_by_path_impl(library_id, "/test/recent").await?;
    assert!(recent_check.is_some());
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_get_child_folders_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    let parent_id = Uuid::new_v4();
    
    // Create parent folder
    let parent_folder = FolderInventory {
        id: parent_id,
        library_id,
        folder_path: "/test/parent".to_string(),
        folder_type: FolderType::Root,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Completed,
        last_processed_at: Some(Utc::now()),
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 0,
        processed_files: 0,
        total_size_bytes: 0,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Create child folders
    let child1 = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/parent/child1".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: Some(parent_id),
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Pending,
        last_processed_at: None,
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 5,
        processed_files: 0,
        total_size_bytes: 1000000,
        file_types: vec![],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    let child2 = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/parent/child2".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: Some(parent_id),
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Completed,
        last_processed_at: Some(Utc::now()),
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 10,
        processed_files: 10,
        total_size_bytes: 2000000,
        file_types: vec!["mp4".to_string()],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Insert all folders
    db.upsert_folder_impl(&parent_folder).await?;
    db.upsert_folder_impl(&child1).await?;
    db.upsert_folder_impl(&child2).await?;
    
    // Get child folders
    let children = db.get_child_folders_impl(parent_id).await?;
    assert_eq!(children.len(), 2);
    
    // Check they're sorted by path
    assert_eq!(children[0].folder_path, "/test/parent/child1");
    assert_eq!(children[1].folder_path, "/test/parent/child2");
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_mark_folder_processed_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    
    let folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id,
        folder_path: "/test/process".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Scan,
        processing_status: FolderProcessingStatus::Processing,
        last_processed_at: None,
        processing_error: Some("Previous error".to_string()),
        processing_attempts: 2,
        next_retry_at: None,
        total_files: 5,
        processed_files: 3,
        total_size_bytes: 1000000,
        file_types: vec!["mp4".to_string()],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    db.upsert_folder_impl(&folder).await?;
    
    // Mark as processed
    db.mark_folder_processed_impl(folder.id).await?;
    
    let processed = db.get_folder_by_path_impl(library_id, "/test/process").await?;
    assert!(processed.is_some());
    let processed_folder = processed.unwrap();
    assert_eq!(processed_folder.processing_status, FolderProcessingStatus::Completed);
    assert!(processed_folder.last_processed_at.is_some());
    assert!(processed_folder.processing_error.is_none()); // Error should be cleared
    
    cleanup_library(&db, library_id).await?;
    Ok(())
}

#[tokio::test]
async fn test_get_folder_inventory_impl() -> Result<()> {
    let db = setup_test_db().await?;
    let library_id = create_test_library(&db).await?;
    let other_library_id = create_test_library(&db).await?;
    
    // Create folders for the target library
    let folders = vec![
        FolderInventory {
            id: Uuid::new_v4(),
            library_id,
            folder_path: "/test/a".to_string(),
            folder_type: FolderType::Root,
            parent_folder_id: None,
            discovered_at: Utc::now(),
            last_seen_at: Utc::now(),
            discovery_source: FolderDiscoverySource::Scan,
            processing_status: FolderProcessingStatus::Completed,
            last_processed_at: Some(Utc::now()),
            processing_error: None,
            processing_attempts: 0,
            next_retry_at: None,
            total_files: 10,
            processed_files: 10,
            total_size_bytes: 1000000,
            file_types: vec!["mp4".to_string()],
            last_modified: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
        FolderInventory {
            id: Uuid::new_v4(),
            library_id,
            folder_path: "/test/b".to_string(),
            folder_type: FolderType::Movie,
            parent_folder_id: None,
            discovered_at: Utc::now(),
            last_seen_at: Utc::now(),
            discovery_source: FolderDiscoverySource::Watch,
            processing_status: FolderProcessingStatus::Pending,
            last_processed_at: None,
            processing_error: None,
            processing_attempts: 0,
            next_retry_at: None,
            total_files: 5,
            processed_files: 0,
            total_size_bytes: 500000,
            file_types: vec![],
            last_modified: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
    ];
    
    // Create a folder for a different library
    let other_folder = FolderInventory {
        id: Uuid::new_v4(),
        library_id: other_library_id,
        folder_path: "/other/folder".to_string(),
        folder_type: FolderType::Movie,
        parent_folder_id: None,
        discovered_at: Utc::now(),
        last_seen_at: Utc::now(),
        discovery_source: FolderDiscoverySource::Manual,
        processing_status: FolderProcessingStatus::Completed,
        last_processed_at: Some(Utc::now()),
        processing_error: None,
        processing_attempts: 0,
        next_retry_at: None,
        total_files: 20,
        processed_files: 20,
        total_size_bytes: 2000000,
        file_types: vec!["mkv".to_string()],
        last_modified: None,
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    
    // Insert all folders
    for folder in &folders {
        db.upsert_folder_impl(folder).await?;
    }
    db.upsert_folder_impl(&other_folder).await?;
    
    // Get inventory for target library
    let inventory = db.get_folder_inventory_impl(library_id).await?;
    assert_eq!(inventory.len(), 2);
    
    // Check they're sorted by path
    assert_eq!(inventory[0].folder_path, "/test/a");
    assert_eq!(inventory[1].folder_path, "/test/b");
    
    // Verify the other library's folder is not included
    assert!(!inventory.iter().any(|f| f.folder_path == "/other/folder"));
    
    cleanup_library(&db, library_id).await?;
    cleanup_library(&db, other_library_id).await?;
    Ok(())
}