use async_trait::async_trait;

use crate::error::Result;
use crate::types::details::LibraryReference;
use crate::types::ids::LibraryId;
use crate::types::library::Library;

/// Repository port for library management within the catalog bounded context.
///
/// Implementations live in infra adapters (e.g., Postgres) and
/// must not leak infra types into domain/application layers.
#[async_trait]
pub trait LibraryRepository: Send + Sync {
    /// Create a library and return its identifier.
    async fn create_library(&self, library: Library) -> Result<LibraryId>;

    /// Fetch a library by id.
    async fn get_library(&self, id: LibraryId) -> Result<Option<Library>>;

    /// List all libraries.
    async fn list_libraries(&self) -> Result<Vec<Library>>;

    /// Update a library by id.
    async fn update_library(
        &self,
        id: LibraryId,
        library: Library,
    ) -> Result<()>;

    /// Delete a library by id.
    async fn delete_library(&self, id: LibraryId) -> Result<()>;

    /// Update the library's last_scan timestamp to now.
    async fn update_library_last_scan(&self, id: LibraryId) -> Result<()>;

    /// Lightweight library references for navigation and APIs.
    async fn list_library_references(&self) -> Result<Vec<LibraryReference>>;

    /// Fetch a single library reference by id (UUID of the library).
    async fn get_library_reference(
        &self,
        id: uuid::Uuid,
    ) -> Result<LibraryReference>;
}
