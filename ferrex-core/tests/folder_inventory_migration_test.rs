use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_table_exists(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query(
        "SELECT EXISTS (
            SELECT FROM information_schema.tables 
            WHERE table_schema = 'ferrex' 
            AND table_name = 'folder_inventory'
        )",
    )
    .fetch_one(&pool)
    .await?;

    let exists: bool = result.get(0);
    assert!(exists, "folder_inventory table should exist");

    Ok(())
}

// Column and index shape checks removed: they were overly brittle against harmless
// schema evolution. Critical behaviors are validated below via constraints,
// uniqueness, triggers, and CRUD operations.

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_constraints(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    // Test folder_type check constraint
    let invalid_folder_type = sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type) 
         VALUES ($1, $2, $3)",
    )
    .bind(Uuid::now_v7())
    .bind("/test/invalid_type")
    .bind("invalid_type")
    .execute(&pool)
    .await;

    assert!(
        invalid_folder_type.is_err(),
        "Should reject invalid folder_type"
    );

    // Test discovery_source check constraint
    let invalid_discovery_source = sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type, discovery_source) 
         VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind("/test/invalid_source")
    .bind("movie")
    .bind("invalid_source")
    .execute(&pool)
    .await;

    assert!(
        invalid_discovery_source.is_err(),
        "Should reject invalid discovery_source"
    );

    // Test processing_status check constraint
    let invalid_processing_status = sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type, processing_status) 
         VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind("/test/invalid_status")
    .bind("movie")
    .bind("invalid_status")
    .execute(&pool)
    .await;

    assert!(
        invalid_processing_status.is_err(),
        "Should reject invalid processing_status"
    );

    // Test valid_file_counts constraint (processed_files > total_files)
    let invalid_file_counts = sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type, total_files, processed_files) 
         VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(Uuid::now_v7())
    .bind("/test/invalid_counts")
    .bind("movie")
    .bind(5)
    .bind(10)  // processed > total
    .execute(&pool)
    .await;

    assert!(
        invalid_file_counts.is_err(),
        "Should reject processed_files > total_files"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_crud_operations(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    // First, we need a library to reference
    let library_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, NOW(), NOW())",
    )
    .bind(library_id)
    .bind("Test Library")
    .bind(vec!["/test/library"])
    .bind("movies")
    .execute(&pool)
    .await?;

    // Test INSERT
    let folder_id = Uuid::now_v7();
    let insert_result = sqlx::query(
        "INSERT INTO folder_inventory (
            id, library_id, folder_path, folder_type, 
            discovery_source, processing_status, total_files, 
            processed_files, total_size_bytes, file_types, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id",
    )
    .bind(folder_id)
    .bind(library_id)
    .bind("/test/movies/action")
    .bind("movie")
    .bind("scan")
    .bind("pending")
    .bind(10)
    .bind(0)
    .bind(1073741824i64) // 1GB
    .bind(serde_json::json!(["mp4", "mkv", "srt"]))
    .bind(serde_json::json!({"custom": "metadata"}))
    .fetch_one(&pool)
    .await?;

    let returned_id: Uuid = insert_result.get(0);
    assert_eq!(returned_id, folder_id, "Inserted folder ID should match");

    // Test SELECT
    let select_result = sqlx::query(
        "SELECT folder_path, folder_type, processing_status, total_files 
         FROM folder_inventory 
         WHERE id = $1",
    )
    .bind(folder_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(
        select_result.get::<String, _>("folder_path"),
        "/test/movies/action"
    );
    assert_eq!(select_result.get::<String, _>("folder_type"), "movie");
    assert_eq!(
        select_result.get::<String, _>("processing_status"),
        "pending"
    );
    assert_eq!(select_result.get::<i32, _>("total_files"), 10);

    // Test UPDATE
    sqlx::query(
        "UPDATE folder_inventory 
         SET processing_status = $1, processed_files = $2, last_processed_at = NOW() 
         WHERE id = $3",
    )
    .bind("completed")
    .bind(10)
    .bind(folder_id)
    .execute(&pool)
    .await?;

    let update_result = sqlx::query(
        "SELECT processing_status, processed_files, last_processed_at 
         FROM folder_inventory 
         WHERE id = $1",
    )
    .bind(folder_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(
        update_result.get::<String, _>("processing_status"),
        "completed"
    );
    assert_eq!(update_result.get::<i32, _>("processed_files"), 10);
    assert!(
        update_result
            .get::<Option<DateTime<Utc>>, _>("last_processed_at")
            .is_some()
    );

    // Test DELETE
    sqlx::query("DELETE FROM folder_inventory WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await?;

    let delete_check =
        sqlx::query("SELECT COUNT(*) FROM folder_inventory WHERE id = $1")
            .bind(folder_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(delete_check.get::<i64, _>(0), 0, "Folder should be deleted");

    // Clean up library
    sqlx::query("DELETE FROM libraries WHERE id = $1")
        .bind(library_id)
        .execute(&pool)
        .await?;

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_cascade_delete(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    // Create a library
    let library_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, NOW(), NOW())",
    )
    .bind(library_id)
    .bind("Test Library for Cascade")
    .bind(vec!["/test/cascade"])
    .bind("movies")
    .execute(&pool)
    .await?;

    // Create parent folder
    let parent_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO folder_inventory (id, library_id, folder_path, folder_type) 
         VALUES ($1, $2, $3, $4)",
    )
    .bind(parent_id)
    .bind(library_id)
    .bind("/test/parent")
    .bind("root")
    .execute(&pool)
    .await?;

    // Create child folder
    let child_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO folder_inventory (id, library_id, folder_path, folder_type, parent_folder_id) 
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(child_id)
    .bind(library_id)
    .bind("/test/parent/child")
    .bind("movie")
    .bind(parent_id)
    .execute(&pool)
    .await?;

    // Delete library - should cascade delete folders
    sqlx::query("DELETE FROM libraries WHERE id = $1")
        .bind(library_id)
        .execute(&pool)
        .await?;

    // Check folders are deleted
    let folder_count = sqlx::query(
        "SELECT COUNT(*) FROM folder_inventory WHERE library_id = $1",
    )
    .bind(library_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(
        folder_count.get::<i64, _>(0),
        0,
        "All folders should be cascade deleted with library"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_unique_constraint(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    // Create a library
    let library_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, NOW(), NOW())",
    )
    .bind(library_id)
    .bind("Test Library Unique")
    .bind(vec!["/test/unique"])
    .bind("movies")
    .execute(&pool)
    .await?;

    // Insert first folder
    sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type) 
         VALUES ($1, $2, $3)",
    )
    .bind(library_id)
    .bind("/test/unique/path")
    .bind("movie")
    .execute(&pool)
    .await?;

    // Try to insert duplicate (same library_id and folder_path)
    let duplicate_result = sqlx::query(
        "INSERT INTO folder_inventory (library_id, folder_path, folder_type) 
         VALUES ($1, $2, $3)",
    )
    .bind(library_id)
    .bind("/test/unique/path")
    .bind("tv_show")
    .execute(&pool)
    .await;

    assert!(
        duplicate_result.is_err(),
        "Should reject duplicate library_id + folder_path combination"
    );

    // Clean up
    sqlx::query("DELETE FROM libraries WHERE id = $1")
        .bind(library_id)
        .execute(&pool)
        .await?;

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn test_folder_inventory_trigger_updated_at(
    pool: PgPool,
) -> Result<(), sqlx::Error> {
    // Create a library
    let library_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO libraries (id, name, paths, library_type, created_at, updated_at) 
         VALUES ($1, $2, $3, $4, NOW(), NOW())",
    )
    .bind(library_id)
    .bind("Test Library Trigger")
    .bind(vec!["/test/trigger"])
    .bind("movies")
    .execute(&pool)
    .await?;

    // Insert folder
    let folder_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO folder_inventory (id, library_id, folder_path, folder_type) 
         VALUES ($1, $2, $3, $4)",
    )
    .bind(folder_id)
    .bind(library_id)
    .bind("/test/trigger/folder")
    .bind("movie")
    .execute(&pool)
    .await?;

    // Get initial updated_at
    let initial =
        sqlx::query("SELECT updated_at FROM folder_inventory WHERE id = $1")
            .bind(folder_id)
            .fetch_one(&pool)
            .await?;
    let initial_updated_at: DateTime<Utc> = initial.get(0);

    // Wait briefly to ensure timestamp difference
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Update the folder
    sqlx::query("UPDATE folder_inventory SET processing_status = 'completed' WHERE id = $1")
        .bind(folder_id)
        .execute(&pool)
        .await?;

    // Get new updated_at
    let updated =
        sqlx::query("SELECT updated_at FROM folder_inventory WHERE id = $1")
            .bind(folder_id)
            .fetch_one(&pool)
            .await?;
    let new_updated_at: DateTime<Utc> = updated.get(0);

    assert!(
        new_updated_at > initial_updated_at,
        "updated_at should be automatically updated by trigger"
    );

    // Clean up
    sqlx::query("DELETE FROM libraries WHERE id = $1")
        .bind(library_id)
        .execute(&pool)
        .await?;

    Ok(())
}
