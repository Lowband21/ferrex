use std::fmt;
use std::sync::Arc;

use crate::application::unit_of_work::AppUnitOfWork;
use crate::database::PostgresDatabase;
use crate::error::{MediaError, Result};

/// Bundles the Postgres infra with the application-facing unit of work.
///
/// This replaces the legacy `MediaDatabase` façade so callers can grab the
/// repository_ports they actually need (via `AppUnitOfWork`) while still exposing
/// the raw Postgres adapter for infra wiring.
#[derive(Clone)]
pub struct DatabaseContext {
    postgres: Arc<PostgresDatabase>,
    unit_of_work: Arc<AppUnitOfWork>,
}

impl fmt::Debug for DatabaseContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseContext")
            .field("postgres_ptr", &Arc::as_ptr(&self.postgres))
            .field("unit_of_work_ptr", &Arc::as_ptr(&self.unit_of_work))
            .finish()
    }
}

impl DatabaseContext {
    /// Establish a PostgreSQL connection and compose the default unit of work.
    pub async fn connect_postgres(connection_string: &str) -> Result<Self> {
        let postgres =
            Arc::new(PostgresDatabase::new(connection_string).await?);
        Self::from_postgres(postgres)
    }

    /// Compose a database context from an existing Postgres adapter.
    pub fn from_postgres(postgres: Arc<PostgresDatabase>) -> Result<Self> {
        let unit_of_work = Arc::new(
            AppUnitOfWork::from_postgres(postgres.clone())
                .map_err(MediaError::Internal)?,
        );

        Ok(Self {
            postgres,
            unit_of_work,
        })
    }

    pub fn unit_of_work(&self) -> Arc<AppUnitOfWork> {
        Arc::clone(&self.unit_of_work)
    }

    pub fn postgres(&self) -> Arc<PostgresDatabase> {
        Arc::clone(&self.postgres)
    }

    pub fn into_parts(self) -> (Arc<PostgresDatabase>, Arc<AppUnitOfWork>) {
        (self.postgres, self.unit_of_work)
    }
}
