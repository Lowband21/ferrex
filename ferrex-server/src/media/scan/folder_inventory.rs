//! Folder inventory monitoring and management endpoints

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use ferrex_core::User;
use ferrex_core::{
    LibraryID,
    database::traits::{FolderInventory, FolderProcessingStatus, MediaDatabaseTrait},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;
use axum::Extension;

/// Query parameters for folder inventory listing
#[derive(Debug, Deserialize)]
pub struct FolderInventoryQuery {
    /// Page number (1-based)
    #[serde(default = "default_page")]
    pub page: u32,

    /// Items per page
    #[serde(default = "default_per_page")]
    pub per_page: u32,

    /// Filter by processing status
    pub status: Option<FolderProcessingStatus>,

    /// Search by path substring
    pub search: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    50
}

/// Response for folder inventory listing
#[derive(Debug, Serialize)]
pub struct FolderInventoryResponse {
    pub folders: Vec<FolderInventoryItem>,
    pub pagination: PaginationInfo,
}

/// Individual folder inventory item
#[derive(Debug, Serialize)]
pub struct FolderInventoryItem {
    pub id: Uuid,
    pub folder_path: String,
    pub folder_type: String,
    pub processing_status: FolderProcessingStatus,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub processing_error: Option<String>,
    pub total_files: i32,
    pub processed_files: i32,
    pub discovered_at: DateTime<Utc>,
    pub parent_folder_id: Option<Uuid>,
}

impl From<FolderInventory> for FolderInventoryItem {
    fn from(folder: FolderInventory) -> Self {
        Self {
            id: folder.id,
            folder_path: folder.folder_path,
            folder_type: format!("{:?}", folder.folder_type),
            processing_status: folder.processing_status,
            last_processed_at: folder.last_processed_at,
            processing_error: folder.processing_error,
            total_files: folder.total_files,
            processed_files: folder.processed_files,
            discovered_at: folder.discovered_at,
            parent_folder_id: folder.parent_folder_id,
        }
    }
}

/// Pagination information
#[derive(Debug, Serialize)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub per_page: u32,
    pub total_items: usize,
    pub total_pages: u32,
}

/// Progress statistics for a library's scan
#[derive(Debug, Serialize)]
pub struct ScanProgressResponse {
    pub library_id: Uuid,
    pub total_folders: usize,
    pub pending_folders: usize,
    pub processing_folders: usize,
    pub completed_folders: usize,
    pub failed_folders: usize,
    pub total_files: i32,
    pub processed_files: i32,
    pub progress_percentage: f32,
    pub errors: Vec<FolderError>,
}

/// Folder error information
#[derive(Debug, Serialize)]
pub struct FolderError {
    pub folder_id: Uuid,
    pub folder_path: String,
    pub error: String,
    pub attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// Response for rescan trigger
#[derive(Debug, Serialize)]
pub struct RescanResponse {
    pub folder_id: Uuid,
    pub status: String,
    pub message: String,
}

/// GET /api/folders/inventory/{library_id}
///
/// List folders in a library with their scanning status
pub async fn get_folder_inventory(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>, // TODO: Pass LibraryID all the way through
    Query(params): Query<FolderInventoryQuery>,
    Extension(_user): Extension<User>,
) -> Result<Json<FolderInventoryResponse>, StatusCode> {
    // Fetch all folders for the library
    let all_folders = state
        .database
        .backend()
        .get_folder_inventory(LibraryID(library_id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get folder inventory: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Apply filters
    let mut filtered = all_folders;

    // Filter by status if provided
    if let Some(status) = params.status {
        filtered.retain(|f| f.processing_status == status);
    }

    // Filter by search term if provided
    if let Some(search) = &params.search {
        let search_lower = search.to_lowercase();
        filtered.retain(|f| f.folder_path.to_lowercase().contains(&search_lower));
    }

    // Calculate pagination
    let total_items = filtered.len();
    let total_pages = ((total_items as f32) / (params.per_page as f32)).ceil() as u32;
    let total_pages = total_pages.max(1);

    // Apply pagination
    let skip = ((params.page - 1) * params.per_page) as usize;
    let take = params.per_page as usize;
    let paginated: Vec<FolderInventory> = filtered.into_iter().skip(skip).take(take).collect();

    // Convert to response items
    let folders: Vec<FolderInventoryItem> = paginated
        .into_iter()
        .map(FolderInventoryItem::from)
        .collect();

    let response = FolderInventoryResponse {
        folders,
        pagination: PaginationInfo {
            current_page: params.page,
            per_page: params.per_page,
            total_items,
            total_pages,
        },
    };

    Ok(Json(response))
}

/// GET /api/folders/progress/{library_id}
///
/// Get scan progress statistics for a library
pub async fn get_scan_progress(
    State(state): State<AppState>,
    Path(library_id): Path<Uuid>,
    Extension(_user): Extension<User>,
) -> Result<Json<ScanProgressResponse>, StatusCode> {
    // Fetch all folders for the library
    let folders = state
        .database
        .backend()
        .get_folder_inventory(LibraryID(library_id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get folder inventory: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Calculate statistics
    let total_folders = folders.len();
    let mut pending_folders = 0;
    let mut processing_folders = 0;
    let mut completed_folders = 0;
    let mut failed_folders = 0;
    let mut total_files = 0i32;
    let mut processed_files = 0i32;
    let mut errors = Vec::new();

    for folder in &folders {
        total_files += folder.total_files;
        processed_files += folder.processed_files;

        match folder.processing_status {
            FolderProcessingStatus::Pending => pending_folders += 1,
            FolderProcessingStatus::Queued => pending_folders += 1, // Count queued as pending
            FolderProcessingStatus::Processing => processing_folders += 1,
            FolderProcessingStatus::Completed => completed_folders += 1,
            FolderProcessingStatus::Failed => {
                failed_folders += 1;
                if let Some(error) = &folder.processing_error {
                    errors.push(FolderError {
                        folder_id: folder.id,
                        folder_path: folder.folder_path.clone(),
                        error: error.clone(),
                        attempts: folder.processing_attempts,
                        next_retry_at: folder.next_retry_at,
                    });
                }
            }
            FolderProcessingStatus::Skipped => {
                // Count skipped as completed for progress purposes
                completed_folders += 1;
            }
        }
    }

    // Calculate progress percentage
    let progress_percentage = if total_folders > 0 {
        (completed_folders as f32 / total_folders as f32) * 100.0
    } else {
        0.0
    };

    let response = ScanProgressResponse {
        library_id,
        total_folders,
        pending_folders,
        processing_folders,
        completed_folders,
        failed_folders,
        total_files,
        processed_files,
        progress_percentage,
        errors,
    };

    Ok(Json(response))
}
