use std::path::PathBuf;

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::database::ports::library::LibraryRepository;
use crate::{Library, LibraryID, LibraryReference, LibraryType, MediaError, Result};

#[derive(Clone, Debug)]
pub struct PostgresLibraryRepository {
    pool: PgPool,
}

impl PostgresLibraryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn encode_type(library_type: LibraryType) -> &'static str {
        match library_type {
            LibraryType::Movies => "movies",
            LibraryType::Series => "tvshows",
        }
    }

    fn decode_type(value: &str) -> Option<LibraryType> {
        match value {
            "movies" => Some(LibraryType::Movies),
            "tvshows" => Some(LibraryType::Series),
            _ => None,
        }
    }
}

#[async_trait]
impl LibraryRepository for PostgresLibraryRepository {
    async fn create_library(&self, library: Library) -> Result<LibraryID> {
        let paths: Vec<String> = library
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let library_type = Self::encode_type(library.library_type);

        sqlx::query!(
            r#"
            INSERT INTO libraries (
                id,
                name,
                library_type,
                paths,
                scan_interval_minutes,
                enabled,
                auto_scan,
                watch_for_changes,
                analyze_on_scan,
                max_retry_attempts
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            library.id.as_uuid(),
            library.name,
            library_type,
            &paths,
            library.scan_interval_minutes as i32,
            library.enabled,
            library.auto_scan,
            library.watch_for_changes,
            library.analyze_on_scan,
            library.max_retry_attempts as i32,
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to create library: {}", e)))?;

        Ok(library.id)
    }

    async fn get_library(&self, id: LibraryID) -> Result<Option<Library>> {
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                name,
                library_type,
                paths,
                scan_interval_minutes,
                last_scan,
                enabled,
                auto_scan,
                watch_for_changes,
                analyze_on_scan,
                max_retry_attempts,
                created_at,
                updated_at
            FROM libraries
            WHERE id = $1
            "#,
            id.as_uuid()
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let library_type = Self::decode_type(&row.library_type)
            .ok_or_else(|| MediaError::InvalidMedia("Unknown library type".to_string()))?;

        Ok(Some(Library {
            id: LibraryID(row.id),
            name: row.name,
            library_type,
            paths: row.paths.into_iter().map(PathBuf::from).collect(),
            scan_interval_minutes: row.scan_interval_minutes as u32,
            last_scan: row.last_scan,
            enabled: row.enabled,
            auto_scan: row.auto_scan,
            watch_for_changes: row.watch_for_changes,
            analyze_on_scan: row.analyze_on_scan,
            max_retry_attempts: row.max_retry_attempts as u32,
            created_at: row.created_at,
            updated_at: row.updated_at,
            media: None,
        }))
    }

    async fn list_libraries(&self) -> Result<Vec<Library>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                name,
                library_type,
                paths,
                scan_interval_minutes,
                last_scan,
                enabled,
                auto_scan,
                watch_for_changes,
                analyze_on_scan,
                max_retry_attempts,
                created_at,
                updated_at
            FROM libraries
            ORDER BY name
            "#
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut libraries = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(library_type) = Self::decode_type(&row.library_type) else {
                continue;
            };

            libraries.push(Library {
                id: LibraryID(row.id),
                name: row.name,
                library_type,
                paths: row.paths.into_iter().map(PathBuf::from).collect(),
                scan_interval_minutes: row.scan_interval_minutes as u32,
                last_scan: row.last_scan,
                enabled: row.enabled,
                auto_scan: row.auto_scan,
                watch_for_changes: row.watch_for_changes,
                analyze_on_scan: row.analyze_on_scan,
                max_retry_attempts: row.max_retry_attempts as u32,
                created_at: row.created_at,
                updated_at: row.updated_at,
                media: None,
            });
        }

        Ok(libraries)
    }

    async fn update_library(&self, id: LibraryID, library: Library) -> Result<()> {
        let paths: Vec<String> = library
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let library_type = Self::encode_type(library.library_type);

        sqlx::query!(
            r#"
            UPDATE libraries
            SET
                name = $1,
                library_type = $2,
                paths = $3,
                scan_interval_minutes = $4,
                enabled = $5,
                auto_scan = $6,
                watch_for_changes = $7,
                analyze_on_scan = $8,
                max_retry_attempts = $9,
                updated_at = NOW()
            WHERE id = $10
            "#,
            library.name,
            library_type,
            &paths,
            library.scan_interval_minutes as i32,
            library.enabled,
            library.auto_scan,
            library.watch_for_changes,
            library.analyze_on_scan,
            library.max_retry_attempts as i32,
            id.as_uuid(),
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Failed to update library: {}", e)))?;

        Ok(())
    }

    async fn delete_library(&self, id: LibraryID) -> Result<()> {
        sqlx::query!("DELETE FROM libraries WHERE id = $1", id.as_uuid())
            .execute(self.pool())
            .await
            .map_err(|e| MediaError::Internal(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    async fn update_library_last_scan(&self, id: LibraryID) -> Result<()> {
        sqlx::query!(
            "UPDATE libraries SET last_scan = NOW(), updated_at = NOW() WHERE id = $1",
            id.as_uuid()
        )
        .execute(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Update failed: {}", e)))?;

        Ok(())
    }

    async fn list_library_references(&self) -> Result<Vec<LibraryReference>> {
        let rows = sqlx::query!(
            "SELECT id, name, library_type, paths FROM libraries WHERE enabled = true ORDER BY name"
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let mut libraries = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(library_type) = Self::decode_type(&row.library_type) else {
                continue;
            };

            libraries.push(LibraryReference {
                id: LibraryID(row.id),
                name: row.name,
                library_type,
                paths: row.paths.into_iter().map(PathBuf::from).collect(),
            });
        }

        Ok(libraries)
    }

    async fn get_library_reference(&self, id: Uuid) -> Result<LibraryReference> {
        let row = sqlx::query!(
            "SELECT id, name, library_type, paths FROM libraries WHERE id = $1",
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|e| MediaError::Internal(format!("Database query failed: {}", e)))?;

        let Some(row) = row else {
            return Err(MediaError::NotFound("Library not found".to_string()));
        };

        let library_type = Self::decode_type(&row.library_type)
            .ok_or_else(|| MediaError::InvalidMedia("Unknown library type".to_string()))?;

        Ok(LibraryReference {
            id: LibraryID(row.id),
            name: row.name,
            library_type,
            paths: row.paths.into_iter().map(PathBuf::from).collect(),
        })
    }
}
