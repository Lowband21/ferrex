use crate::database::repository_ports::media_references::MediaReferencesRepository;
use crate::domain::scan::AnalyzeScanHierarchy;
use crate::domain::scan::IndexUpsertJob;
use crate::domain::scan::actors::metadata::MediaReadyForIndex;
use crate::error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrex_model::{LibraryId, Media, MediaID};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct IndexCommand {
    pub job: IndexUpsertJob,
    pub ready: MediaReadyForIndex,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexingChange {
    Created,
    Updated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexingOutcome {
    pub library_id: LibraryId,
    pub path_norm: String,
    #[serde(default)]
    pub media_id: MediaID,
    pub hierarchy: AnalyzeScanHierarchy,
    pub indexed_at: DateTime<Utc>,
    pub upserted: bool,
    pub media: Option<Media>,
    #[serde(default = "IndexingOutcome::default_change")]
    pub change: IndexingChange,
}

impl IndexingOutcome {
    fn default_change() -> IndexingChange {
        IndexingChange::Created
    }
}

#[async_trait]
pub trait IndexerActor: Send + Sync {
    async fn index(
        &self,
        command: IndexCommand,
    ) -> error::Result<IndexingOutcome>;
}

pub struct DefaultIndexerActor {
    media_refs: Arc<dyn MediaReferencesRepository>,
}

impl fmt::Debug for DefaultIndexerActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultIndexerActor")
            .finish_non_exhaustive()
    }
}

impl DefaultIndexerActor {
    pub fn new(media_refs: Arc<dyn MediaReferencesRepository>) -> Self {
        Self { media_refs }
    }

    async fn hydrate_media(&self, media_id: MediaID) -> error::Result<Media> {
        self.media_refs.get_media_reference(&media_id).await
    }
}

#[async_trait]
impl IndexerActor for DefaultIndexerActor {
    async fn index(
        &self,
        command: IndexCommand,
    ) -> error::Result<IndexingOutcome> {
        let IndexCommand { job, ready } = command;
        let media = self.hydrate_media(ready.media_id).await?;

        Ok(IndexingOutcome {
            library_id: job.library_id,
            path_norm: job.path_norm,
            media_id: ready.media_id,
            hierarchy: ready.hierarchy,
            indexed_at: Utc::now(),
            upserted: true,
            media: Some(media),
            change: IndexingOutcome::default_change(),
        })
    }
}
