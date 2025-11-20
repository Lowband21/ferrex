use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::{LibraryID, LibraryReference, Result};

/// Tunable parameters the orchestration layer expects when spinning up a library actor.
#[derive(Debug, Clone)]
pub struct LibraryActorSettings {
    pub maintenance_partitions: u16,
    pub filesystem_debounce_ms: u64,
    pub max_outstanding_jobs: usize,
}

/// Minimal record describing a library and its runtime policies.
#[derive(Debug, Clone)]
pub struct LibraryRecord {
    pub reference: LibraryReference,
    pub actor_settings: LibraryActorSettings,
}

/// Snapshot data that allows the library actor to resume progress across restarts.
#[derive(Debug, Clone, Default)]
pub struct LibraryActorSnapshot {
    pub maintenance_cursor: HashMap<u16, DateTime<Utc>>,
    pub last_seed_at: Option<DateTime<Utc>>,
}

pub trait LibraryRepo<'tx>: Send {
    fn get(&mut self, library_id: LibraryID) -> Result<Option<LibraryRecord>>;
    fn list(&mut self) -> Result<Vec<LibraryRecord>>;

    fn load_actor_snapshot(
        &mut self,
        library_id: LibraryID,
    ) -> Result<Option<LibraryActorSnapshot>>;
    fn persist_actor_snapshot(
        &mut self,
        library_id: LibraryID,
        snapshot: &LibraryActorSnapshot,
    ) -> Result<()>;
}
